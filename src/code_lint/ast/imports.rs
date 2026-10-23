//! Per-file import map: resolves names through Python `import` / `from … import` and Rust `use`
//! declarations, with file-level definitions shadowing imports.

use super::tree::ParsedFile;
use crate::diagnostic::Language;
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
pub(super) struct ImportMap {
    /// Bound name to canonical path (`c` to `a.b` for `import a.b as c`).
    pub(super) imports: HashMap<String, String>,
    /// `(line, canonical_path)` of every Python `import` / `from … import` binding.
    pub(super) import_lines: Vec<(usize, String)>,
    /// Names defined in the file, which shadow imports.
    pub(super) locals: HashSet<String>,
}

/// A dotted name together with its last segment (`typing.List` and `List`).
#[derive(Clone)]
pub(super) struct QualifiedName {
    /// The full dotted or `::`-separated path.
    pub(super) path: String,
    /// The last segment of `path`.
    pub(super) terminal: String,
}

impl QualifiedName {
    /// Builds a name whose terminal is the part of `path` after its last `separator`.
    pub(super) fn from_path(path: String, separator: &str) -> Self {
        let terminal = path
            .rsplit_once(separator)
            .map_or(path.as_str(), |(_, terminal)| terminal)
            .to_owned();
        Self { path, terminal }
    }

    /// Resolves `path` through the imports of `file` (`t.List` after `import typing as t` is
    /// `typing.List` with terminal `List`), or `None` if it names a definition of `file`
    /// (see [`resolve_name`]).
    pub(super) fn resolve(self, file: &ParsedFile) -> Option<Self> {
        match resolve_name(file, &self.path) {
            ResolvedName::Imported(resolved) => {
                Some(Self::from_path(resolved, path_separator(file.lang())))
            }
            ResolvedName::Local => None,
            ResolvedName::Unbound => Some(self),
        }
    }
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
    let separator = path_separator(file.lang());
    let (head, rest) = name
        .split_once(separator)
        .map_or((name, None), |(head, rest)| (head, Some(rest)));
    if !is_identifier(head) {
        return ResolvedName::Unbound;
    }
    let map = file
        .imports
        .get_or_init(|| dispatch_lang!(file.lang(), collect_import_map(file)));
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

/// Returns `(line, canonical_path)` for every file-level `import` / `from … import` in `file`.
#[must_use]
pub(super) fn import_lines(file: &ParsedFile) -> &[(usize, String)] {
    &file
        .imports
        .get_or_init(|| dispatch_lang!(file.lang(), collect_import_map(file)))
        .import_lines
}

/// The separator between the segments of a path in `lang` (`.` or `::`).
const fn path_separator(lang: Language) -> &'static str {
    match lang {
        Language::Python => ".",
        Language::Rust => "::",
    }
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
        let source = indoc::indoc! {r"
            use std::thread::sleep;
            fn sleep() {}
            struct Config;
        "};
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
