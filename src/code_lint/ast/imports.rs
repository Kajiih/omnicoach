//! Per-file import map: resolves names through Python `import` / `from … import` and Rust `use`
//! declarations, with file-level definitions shadowing imports.

use super::{CodeLintAst, ParsedFile};
use crate::diagnostic::Language;
use ra_ap_syntax::AstNode as _;
use ra_ap_syntax::ast::{self, HasModuleItem as _, HasName as _};
use ruff_python_ast::{ExceptHandler, Stmt};
use std::collections::{HashMap, HashSet};

/// What a dotted (Python) or `::` (Rust) name refers to, judged by its first segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedName {
    /// The first segment is bound by an import; holds the canonical path of the whole name
    /// (`t.List` after `import typing as t` is `typing.List`).
    Imported(String),
    /// The first segment is defined in the file itself (see [`resolve_name`]).
    Local,
    /// The first segment is neither imported nor defined in the file; the name stands as written.
    Unbound,
}

/// Names bound by file-level imports and definitions.
#[derive(Default)]
pub(in crate::code_lint::ast) struct ImportMap {
    /// Bound name to canonical path (`c` to `a.b` for `import a.b as c`).
    imports: HashMap<String, String>,
    /// Names defined in the file, which shadow imports.
    locals: HashSet<String>,
}

/// Resolves `name` (`os.getenv`, `t.List`, `thread::sleep`) through the file-level imports and
/// definitions of `file`.
///
/// Considered bindings:
/// - Python: `import a.b` (binds `a`), `import a.b as c`, `from a.b import x [as y]` (relative
///   modules keep their leading dots, so they never equal an absolute path; `*` is ignored) and
///   `def` / `class` names, in the module body and in top-level `if` / `try` / `with` blocks.
///   Assignment targets are not definitions here, so variables keep their written name.
/// - Rust: root-level `use` trees (`use a::b::{self, c as d}`; globs and `as _` are ignored, a
///   leading `::` is dropped) and root-level item names (`fn`, `struct`, `enum`, `union`,
///   `trait`, `type`, `const`, `static`, `mod`, `macro_rules!`).
///
/// A definition takes precedence over an import of the same name. A first segment that is not a
/// plain identifier (`foo()`, `x[0]`, a Rust method callee `a.b`) is [`ResolvedName::Unbound`].
#[must_use]
pub fn resolve_name(file: &ParsedFile, name: &str) -> ResolvedName {
    let separator = match file.lang() {
        Language::Python => ".",
        Language::Rust => "::",
    };
    let (head, rest) = name
        .split_once(separator)
        .map_or((name, None), |(head, rest)| (head, Some(rest)));
    if !is_identifier(head) {
        return ResolvedName::Unbound;
    }
    let map = file.imports.get_or_init(|| build_import_map(file));
    if map.locals.contains(head) {
        return ResolvedName::Local;
    }
    map.imports.get(head).map_or(ResolvedName::Unbound, |path| {
        ResolvedName::Imported(
            rest.map_or_else(|| path.clone(), |rest| format!("{path}{separator}{rest}")),
        )
    })
}

/// Returns true if `text` is a non-empty identifier (`[A-Za-z_][A-Za-z0-9_]*`, Unicode letters
/// included).
fn is_identifier(text: &str) -> bool {
    let mut characters = text.chars();
    characters
        .next()
        .is_some_and(|first| first == '_' || first.is_alphabetic())
        && characters.all(|character| character == '_' || character.is_alphanumeric())
}

fn build_import_map(file: &ParsedFile) -> ImportMap {
    let mut map = ImportMap::default();
    match &file.ast {
        CodeLintAst::Python(Ok(parsed)) => collect_python_bindings(&parsed.syntax().body, &mut map),
        CodeLintAst::Python(Err(_)) => {}
        CodeLintAst::Rust(parsed) => {
            for item in parsed.tree().items() {
                match item {
                    ast::Item::Use(use_item) => {
                        if let Some(use_tree) = use_item.use_tree() {
                            collect_use_tree(&use_tree, "", &mut map);
                        }
                    }
                    item => {
                        if let Some(name) = rust_item_name(&item) {
                            map.locals.insert(name);
                        }
                    }
                }
            }
        }
    }
    map
}

/// Records the imports and definitions of a Python module body or top-level block `body`.
fn collect_python_bindings(body: &[Stmt], map: &mut ImportMap) {
    for statement in body {
        match statement {
            Stmt::Import(import) => {
                for alias in &import.names {
                    let module = alias.name.as_str();
                    if let Some(asname) = &alias.asname {
                        map.imports.insert(asname.to_string(), module.to_owned());
                    } else {
                        let package = module.split('.').next().unwrap_or(module);
                        map.imports.insert(package.to_owned(), package.to_owned());
                    }
                }
            }
            Stmt::ImportFrom(import_from) => {
                let dots = ".".repeat(import_from.level as usize);
                let module = import_from
                    .module
                    .as_ref()
                    .map_or_else(|| dots.clone(), |module| format!("{dots}{module}"));
                for alias in &import_from.names {
                    let name = alias.name.as_str();
                    if name == "*" {
                        continue;
                    }
                    let path = if module.ends_with('.') {
                        format!("{module}{name}")
                    } else {
                        format!("{module}.{name}")
                    };
                    let bound = alias.asname.as_ref().unwrap_or(&alias.name);
                    map.imports.insert(bound.to_string(), path);
                }
            }
            Stmt::FunctionDef(function) => {
                map.locals.insert(function.name.to_string());
            }
            Stmt::ClassDef(class) => {
                map.locals.insert(class.name.to_string());
            }
            Stmt::If(if_statement) => {
                collect_python_bindings(&if_statement.body, map);
                for clause in &if_statement.elif_else_clauses {
                    collect_python_bindings(&clause.body, map);
                }
            }
            Stmt::Try(try_statement) => {
                collect_python_bindings(&try_statement.body, map);
                for ExceptHandler::ExceptHandler(handler) in &try_statement.handlers {
                    collect_python_bindings(&handler.body, map);
                }
                collect_python_bindings(&try_statement.orelse, map);
                collect_python_bindings(&try_statement.finalbody, map);
            }
            Stmt::With(with_statement) => collect_python_bindings(&with_statement.body, map),
            _ => {}
        }
    }
}

/// Records the bindings of Rust `use_tree`, nested under the `::`-joined `prefix`.
fn collect_use_tree(use_tree: &ast::UseTree, prefix: &str, map: &mut ImportMap) {
    let path_text = use_tree.path().map(|path| {
        path.syntax()
            .text()
            .to_string()
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>()
    });
    let full_path = match path_text.as_deref() {
        None => prefix.to_owned(),
        Some("self") if !prefix.is_empty() => prefix.to_owned(),
        Some(path) if prefix.is_empty() => path.trim_start_matches("::").to_owned(),
        Some(path) => format!("{prefix}::{path}"),
    };
    if let Some(sub_trees) = use_tree.use_tree_list() {
        for child_tree in sub_trees.use_trees() {
            collect_use_tree(&child_tree, &full_path, map);
        }
        return;
    }
    if use_tree.star_token().is_some() || full_path.is_empty() {
        return;
    }
    let bound = match use_tree.rename() {
        Some(rename) => match rename.name() {
            Some(name) => name.text().to_string(),
            None => return,
        },
        None => full_path
            .rsplit("::")
            .next()
            .unwrap_or(&full_path)
            .to_owned(),
    };
    map.imports.insert(bound, full_path);
}

/// The name a root-level Rust item defines, if it is a named definition.
fn rust_item_name(item: &ast::Item) -> Option<String> {
    let name = match item {
        ast::Item::Fn(item) => item.name(),
        ast::Item::Struct(item) => item.name(),
        ast::Item::Enum(item) => item.name(),
        ast::Item::Union(item) => item.name(),
        ast::Item::Trait(item) => item.name(),
        ast::Item::TypeAlias(item) => item.name(),
        ast::Item::Const(item) => item.name(),
        ast::Item::Static(item) => item.name(),
        ast::Item::Module(item) => item.name(),
        ast::Item::MacroRules(item) => item.name(),
        _ => None,
    }?;
    Some(name.text().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(source: &str, lang: Language, name: &str) -> ResolvedName {
        resolve_name(&ParsedFile::new(source, lang), name)
    }

    fn imported(path: &str) -> ResolvedName {
        ResolvedName::Imported(path.to_owned())
    }

    #[test]
    fn test_python_module_alias() {
        let source = "import typing as t\n";
        assert_eq!(
            resolve(source, Language::Python, "t.List"),
            imported("typing.List")
        );
        assert_eq!(
            resolve(source, Language::Python, "typing.List"),
            ResolvedName::Unbound
        );
    }

    #[test]
    fn test_python_from_import_alias() {
        let source = "from collections.abc import Set as ReadOnlySet\nfrom typing import cast\n";
        assert_eq!(
            resolve(source, Language::Python, "ReadOnlySet"),
            imported("collections.abc.Set")
        );
        assert_eq!(
            resolve(source, Language::Python, "cast"),
            imported("typing.cast")
        );
    }

    #[test]
    fn test_python_submodule_import_binds_package() {
        let source = "import os.path\nimport a.b as c\n";
        assert_eq!(
            resolve(source, Language::Python, "os.getenv"),
            imported("os.getenv")
        );
        assert_eq!(resolve(source, Language::Python, "c.d"), imported("a.b.d"));
    }

    #[test]
    fn test_python_relative_import_keeps_dots() {
        let source = "from . import cast\nfrom .util import sleep\n";
        assert_eq!(resolve(source, Language::Python, "cast"), imported(".cast"));
        assert_eq!(
            resolve(source, Language::Python, "sleep"),
            imported(".util.sleep")
        );
    }

    #[test]
    fn test_python_local_definitions_shadow_imports() {
        let source = indoc::indoc! {r"
            from typing import cast

            def cast(value):
                return value

            if TYPE_CHECKING:
                class Set:
                    pass
        "};
        assert_eq!(
            resolve(source, Language::Python, "cast"),
            ResolvedName::Local
        );
        assert_eq!(
            resolve(source, Language::Python, "Set"),
            ResolvedName::Local
        );
    }

    #[test]
    fn test_python_unbound_and_nested_names() {
        let source = indoc::indoc! {r"
            loop = get_loop()

            def run():
                from gevent import sleep
                sleep(1)
        "};
        assert_eq!(
            resolve(source, Language::Python, "sleep"),
            ResolvedName::Unbound
        );
        assert_eq!(
            resolve(source, Language::Python, "loop.create_task"),
            ResolvedName::Unbound
        );
        assert_eq!(
            resolve(source, Language::Python, "get().x"),
            ResolvedName::Unbound
        );
    }

    #[test]
    fn test_rust_use_tree_forms() {
        let source = "use std::{env::{self, var as get_var}, thread::sleep};\nuse ::tokio::time;\n";
        assert_eq!(
            resolve(source, Language::Rust, "env::var"),
            imported("std::env::var")
        );
        assert_eq!(
            resolve(source, Language::Rust, "get_var"),
            imported("std::env::var")
        );
        assert_eq!(
            resolve(source, Language::Rust, "sleep"),
            imported("std::thread::sleep")
        );
        assert_eq!(
            resolve(source, Language::Rust, "time::sleep"),
            imported("tokio::time::sleep")
        );
    }

    #[test]
    fn test_rust_globs_underscore_and_nested_uses_bind_nothing() {
        let source = indoc::indoc! {r"
            use std::thread::*;
            use std::io::Write as _;

            mod tests {
                use std::env::var;
            }
        "};
        assert_eq!(
            resolve(source, Language::Rust, "sleep"),
            ResolvedName::Unbound
        );
        assert_eq!(
            resolve(source, Language::Rust, "Write"),
            ResolvedName::Unbound
        );
        assert_eq!(
            resolve(source, Language::Rust, "var"),
            ResolvedName::Unbound
        );
    }

    #[test]
    fn test_rust_local_items_shadow_imports() {
        let source = "use std::thread::sleep;\nfn sleep() {}\nstruct Config;\n";
        assert_eq!(
            resolve(source, Language::Rust, "sleep"),
            ResolvedName::Local
        );
        assert_eq!(
            resolve(source, Language::Rust, "Config::new"),
            ResolvedName::Local
        );
        assert_eq!(
            resolve(source, Language::Rust, "clock.sleep"),
            ResolvedName::Unbound
        );
    }
}
