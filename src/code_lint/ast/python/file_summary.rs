//! Owned, body-free structural summary of a Python module (`CodeLintAst`).

use super::classes::{PythonClassInfo, unpacked_targets};
use super::docstrings::OwnedDocstring;
use super::imports::{is_negated_type_checking_guard, is_type_checking_guard};
use crate::code_lint::ast::tree::{ParsedFile, span_from_ruff_range};
use crate::diagnostic::SourceSpan;
use ruff_python_ast::{ExceptHandler, Expr, Operator, Stmt};
use ruff_text_size::Ranged as _;
use std::collections::{HashMap, HashSet};

const DUNDER_ALL: &str = "__all__";

/// Top-level `import` or `from ... import ...` binding in a Python module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonImportBinding {
    /// Bound local name in module scope (`Store`, `interfaces`, `alias`).
    pub local_name: String,
    /// Relative import dot count (`0` for absolute, `1` for `.`, `2` for `..`).
    pub level: u32,
    /// Dotted module specifier (`Some("pkg.interfaces")` or `None` for `from . import foo`).
    pub module: Option<String>,
    /// Imported attribute name (`Some("Store")` for `from m import Store`, `None` for `import m`).
    pub imported_name: Option<String>,
    /// True when the binding only appears inside `if TYPE_CHECKING:` (never imported at runtime).
    pub type_checking_only: bool,
}

/// Top-level `from ... import *` wildcard re-export target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonStarImport {
    /// Relative import dot count (`0` for absolute).
    pub level: u32,
    /// Dotted module specifier (`Some("sub")` or `None` for `from . import *`).
    pub module: Option<String>,
}

/// Explicit `__all__` export policy of a Python module.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ExportedAll {
    /// Module has no static `__all__` assignment; public names (`!starts_with('_')`) are exported.
    #[default]
    Implicit,
    /// Module defines `__all__ = [...]` / `(...)` with these exported symbol names.
    Explicit(Vec<String>),
}

/// Owned summary of a single base class expression on a top-level Python class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonBaseSummary {
    /// Unsubscripted written base expression (`Store`, `interfaces.Store`, `Protocol`).
    pub unsubscripted_name: String,
    /// True when the base resolves locally to `object`, `Generic`, `Protocol`, or `ABC`.
    pub is_structural_marker: bool,
}

/// Owned, body-free summary of a top-level Python `class` definition in source order.
#[expect(
    clippy::struct_excessive_bools,
    reason = "four orthogonal class base-kind predicates (Protocol, ABC, Enum, NamedTuple)"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonClassSummary {
    /// Class identifier.
    pub name: String,
    /// Byte span of the class name, identifying the class among same-named classes of its file.
    pub name_span: SourceSpan,
    /// Direct base class expressions in declaration order.
    pub bases: Vec<PythonBaseSummary>,
    /// True if a direct base resolves to `typing.Protocol` or `typing_extensions.Protocol`.
    pub is_protocol: bool,
    /// True if the class directly inherits `abc.ABC` or `metaclass=ABCMeta`.
    pub is_abc: bool,
    /// True if a direct base is a standard `enum` base.
    pub is_enum: bool,
    /// True if a direct base is `NamedTuple`.
    pub is_named_tuple: bool,
    /// Public non-property method names declared directly in the class body.
    pub public_non_property_methods: HashSet<String>,
    /// Last documented `def <name>` docstring per method name (matching `inspect.getdoc`).
    pub documented_methods: HashMap<String, OwnedDocstring>,
}

/// A non-routing top-level Python definition (`def`, `class`, type alias, or non-`__all__` assignment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonTopLevelItem {
    /// Defined identifier name.
    pub name: String,
    /// Byte span of the top-level statement.
    pub span: SourceSpan,
}

/// Owned, body-free summary of a Python module's top-level surface.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PythonFileSummary {
    /// Top-level import bindings keyed by bound local name (last binding wins).
    pub imports: HashMap<String, PythonImportBinding>,
    /// Dotted submodule paths explicitly imported via `import a.b.c` without `as`
    /// (`"a.b"`, `"a.b.c"`), allowing dotted attribute traversal through submodules.
    pub imported_submodules: HashSet<String>,
    /// Top-level `from ... import *` declarations in source order.
    pub star_imports: Vec<PythonStarImport>,
    /// Explicit `__all__` export specification when statically assigned at top level.
    pub exported_all: ExportedAll,
    /// Names defined at module top level (`def`, `class`, assignments), excluding imports.
    pub defined_names: HashSet<String>,
    /// Top-level `class` declarations in source order.
    pub classes: Vec<PythonClassSummary>,
    /// Non-routing top-level items (`def`, `class`, non-`__all__` assignments) in source order.
    pub non_routing_items: Vec<PythonTopLevelItem>,
}

/// Extracts an owned, body-free [`PythonFileSummary`] from `file` without inspecting function bodies.
#[must_use]
pub fn summarize_python_file(file: &ParsedFile) -> PythonFileSummary {
    let mut summary = PythonFileSummary::default();
    let Some(parsed) = file.py_module() else {
        return summary;
    };
    collect_top_level_statements(&parsed.syntax().body, file, false, &mut summary);
    summary
}

fn collect_top_level_statements(
    body: &[Stmt],
    file: &ParsedFile,
    in_type_checking: bool,
    out: &mut PythonFileSummary,
) {
    for statement in body {
        match statement {
            Stmt::Import(import_statement) => {
                record_top_level_import(import_statement, in_type_checking, out);
            }
            Stmt::ImportFrom(import_from) => {
                record_top_level_import_from(import_from, in_type_checking, out);
            }
            Stmt::FunctionDef(function_def) => {
                let name = function_def.name.to_string();
                out.defined_names.insert(name.clone());
                out.non_routing_items.push(PythonTopLevelItem {
                    name,
                    span: span_from_ruff_range(function_def.range()),
                });
            }
            Stmt::ClassDef(class_def) => {
                let name = class_def.name.to_string();
                out.defined_names.insert(name.clone());
                out.non_routing_items.push(PythonTopLevelItem {
                    name,
                    span: span_from_ruff_range(class_def.range()),
                });
                out.classes.push(summarize_class(class_def, file));
            }
            Stmt::Assign(assign) => {
                let span = span_from_ruff_range(assign.range());
                for target in &assign.targets {
                    record_assigned_names(
                        target,
                        span,
                        true,
                        &mut out.defined_names,
                        &mut out.non_routing_items,
                    );
                    if matches!(target, Expr::Name(name) if name.id == DUNDER_ALL) {
                        out.exported_all = parse_exported_all(&assign.value);
                    }
                }
            }
            Stmt::AnnAssign(annotated_assign) => {
                let span = span_from_ruff_range(annotated_assign.range());
                record_assigned_names(
                    &annotated_assign.target,
                    span,
                    annotated_assign.value.is_some(),
                    &mut out.defined_names,
                    &mut out.non_routing_items,
                );
                if matches!(annotated_assign.target.as_ref(), Expr::Name(name) if name.id == DUNDER_ALL)
                    && let Some(value) = &annotated_assign.value
                {
                    out.exported_all = parse_exported_all(value);
                }
            }
            Stmt::AugAssign(augmented_assign) => {
                if matches!(augmented_assign.target.as_ref(), Expr::Name(name) if name.id == DUNDER_ALL)
                {
                    update_augmented_exported_all(augmented_assign, &mut out.exported_all);
                }
            }
            Stmt::TypeAlias(type_alias) => {
                if let Expr::Name(name) = type_alias.name.as_ref() {
                    let ident = name.id.to_string();
                    out.defined_names.insert(ident.clone());
                    out.non_routing_items.push(PythonTopLevelItem {
                        name: ident,
                        span: span_from_ruff_range(type_alias.range()),
                    });
                }
            }
            Stmt::If(if_statement) => {
                collect_top_level_statements(
                    &if_statement.body,
                    file,
                    in_type_checking || is_type_checking_guard(&if_statement.test),
                    out,
                );
                let else_is_type_checking = is_negated_type_checking_guard(&if_statement.test)
                    && if_statement.elif_else_clauses.len() == 1;
                for clause in &if_statement.elif_else_clauses {
                    let clause_type_checking = clause
                        .test
                        .as_ref()
                        .map_or(else_is_type_checking, is_type_checking_guard);
                    collect_top_level_statements(
                        &clause.body,
                        file,
                        in_type_checking || clause_type_checking,
                        out,
                    );
                }
            }
            _ => {
                for block in module_scope_blocks(statement) {
                    collect_top_level_statements(block, file, in_type_checking, out);
                }
            }
        }
    }
}

/// The statement blocks nested in `statement` whose bindings stay in module scope: the bodies of
/// `if`, `try`, `with`, `for`, `while`, and `match`, but not of `def` or `class`.
fn module_scope_blocks(statement: &Stmt) -> Vec<&[Stmt]> {
    match statement {
        Stmt::Try(try_statement) => std::iter::once(try_statement.body.as_slice())
            .chain(
                try_statement
                    .handlers
                    .iter()
                    .map(|ExceptHandler::ExceptHandler(handler)| handler.body.as_slice()),
            )
            .chain([
                try_statement.orelse.as_slice(),
                try_statement.finalbody.as_slice(),
            ])
            .collect(),
        Stmt::With(with_statement) => vec![with_statement.body.as_slice()],
        Stmt::For(for_statement) => vec![
            for_statement.body.as_slice(),
            for_statement.orelse.as_slice(),
        ],
        Stmt::While(while_statement) => vec![
            while_statement.body.as_slice(),
            while_statement.orelse.as_slice(),
        ],
        Stmt::Match(match_statement) => match_statement
            .cases
            .iter()
            .map(|case| case.body.as_slice())
            .collect(),
        _ => Vec::new(),
    }
}

fn record_top_level_import(
    import_statement: &ruff_python_ast::StmtImport,
    in_type_checking: bool,
    out: &mut PythonFileSummary,
) {
    for alias in &import_statement.names {
        let full_module = alias.name.to_string();
        let binding = if let Some(asname) = &alias.asname {
            PythonImportBinding {
                local_name: asname.to_string(),
                level: 0,
                module: Some(full_module),
                imported_name: None,
                type_checking_only: in_type_checking,
            }
        } else {
            for (dot_index, _) in full_module.match_indices('.') {
                let prefix = &full_module[..dot_index];
                if prefix.contains('.') {
                    out.imported_submodules.insert(prefix.to_owned());
                }
            }
            if full_module.contains('.') {
                out.imported_submodules.insert(full_module.clone());
            }
            let root_package = full_module
                .split('.')
                .next()
                .unwrap_or(full_module.as_str())
                .to_owned();
            PythonImportBinding {
                local_name: root_package.clone(),
                level: 0,
                module: Some(root_package),
                imported_name: None,
                type_checking_only: in_type_checking,
            }
        };
        insert_import_binding(out, binding);
    }
}

fn record_top_level_import_from(
    import_from: &ruff_python_ast::StmtImportFrom,
    in_type_checking: bool,
    out: &mut PythonFileSummary,
) {
    let module = import_from.module.as_ref().map(ToString::to_string);
    for alias in &import_from.names {
        if alias.name.as_str() == "*" {
            out.star_imports.push(PythonStarImport {
                level: import_from.level,
                module: module.clone(),
            });
            continue;
        }
        let local_name = alias.asname.as_ref().unwrap_or(&alias.name).to_string();
        let binding = PythonImportBinding {
            local_name,
            level: import_from.level,
            module: module.clone(),
            imported_name: Some(alias.name.to_string()),
            type_checking_only: in_type_checking,
        };
        insert_import_binding(out, binding);
    }
}

fn insert_import_binding(out: &mut PythonFileSummary, binding: PythonImportBinding) {
    if binding.type_checking_only
        && out
            .imports
            .get(&binding.local_name)
            .is_some_and(|existing| !existing.type_checking_only)
    {
        return;
    }
    out.imports.insert(binding.local_name.clone(), binding);
}

fn summarize_class(
    class_def: &ruff_python_ast::StmtClassDef,
    file: &ParsedFile,
) -> PythonClassSummary {
    let info = PythonClassInfo::from_ast(class_def, file);
    let public_non_property_methods = info.public_non_property_method_names();
    let documented_methods = info.documented_methods();
    let bases = info
        .bases
        .iter()
        .map(|base| PythonBaseSummary {
            unsubscripted_name: base.unsubscripted_name().to_owned(),
            is_structural_marker: base.is_structural_marker(),
        })
        .collect();
    PythonClassSummary {
        name: info.name.clone(),
        name_span: info.name_node.span(),
        bases,
        is_protocol: info.is_protocol(),
        is_abc: info.is_abc(),
        is_enum: info.is_enum(),
        is_named_tuple: info.is_named_tuple(),
        public_non_property_methods,
        documented_methods,
    }
}

fn record_assigned_names(
    target: &Expr,
    span: SourceSpan,
    has_value: bool,
    names: &mut HashSet<String>,
    non_routing_items: &mut Vec<PythonTopLevelItem>,
) {
    for leaf in unpacked_targets(target) {
        if let Expr::Name(name) = leaf {
            let ident = name.id.to_string();
            if has_value {
                names.insert(ident.clone());
            }
            if ident != DUNDER_ALL {
                non_routing_items.push(PythonTopLevelItem { name: ident, span });
            }
        }
    }
}

fn update_augmented_exported_all(
    augmented_assign: &ruff_python_ast::StmtAugAssign,
    exported_all: &mut ExportedAll,
) {
    if augmented_assign.op == Operator::Add
        && let ExportedAll::Explicit(ref mut existing) = *exported_all
        && let ExportedAll::Explicit(extra) = parse_exported_all(&augmented_assign.value)
    {
        existing.extend(extra);
    } else {
        *exported_all = ExportedAll::Implicit;
    }
}

fn parse_exported_all(expr: &Expr) -> ExportedAll {
    let elements = match expr {
        Expr::List(list) => &list.elts,
        Expr::Tuple(tuple) => &tuple.elts,
        _ => return ExportedAll::Implicit,
    };
    let mut strings = Vec::with_capacity(elements.len());
    for element in elements {
        let Expr::StringLiteral(literal) = element else {
            return ExportedAll::Implicit;
        };
        strings.push(literal.value.to_str().to_owned());
    }
    ExportedAll::Explicit(strings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;

    #[test]
    fn summarize_python_file_ignores_function_local_classes() {
        let source = indoc::indoc! {r#"
            from typing import Protocol

            class Store(Protocol):
                def load(self, key: str) -> str:
                    """Load a value."""
                    ...

            def build_local():
                class Store:
                    def unrelated(self) -> None:
                        pass
                return Store()
        "#};
        let file = ParsedFile::new(source, Language::Python);
        let summary = summarize_python_file(&file);

        assert_eq!(summary.classes.len(), 1);
        assert!(summary.classes[0].is_protocol);
        assert!(
            summary.classes[0]
                .public_non_property_methods
                .contains("load")
        );
        assert!(summary.classes[0].documented_methods.contains_key("load"));
    }

    #[test]
    fn summarize_python_file_collects_classes_of_every_module_level_block() {
        let source = indoc::indoc! {r"
            if FLAG:
                class InIf: ...
            try:
                class InTry: ...
            except ImportError:
                class InExcept: ...
            with context():
                class InWith: ...
            for item in items:
                class InFor: ...
            while running:
                class InWhile: ...
            class Outer:
                class Nested: ...
        "};
        let file = ParsedFile::new(source, Language::Python);
        let summary = summarize_python_file(&file);
        let names: Vec<&str> = summary
            .classes
            .iter()
            .map(|class| class.name.as_str())
            .collect();

        assert_eq!(
            names,
            [
                "InIf", "InTry", "InExcept", "InWith", "InFor", "InWhile", "Outer"
            ]
        );
    }

    #[test]
    fn summarize_python_file_captures_exports_and_imports() {
        let source = indoc::indoc! {r#"
            from .sub import Helper as AliasedHelper
            from .wild import *

            __all__ = ["Store"]
        "#};
        let file = ParsedFile::new(source, Language::Python);
        let summary = summarize_python_file(&file);

        assert_eq!(
            summary.exported_all,
            ExportedAll::Explicit(vec!["Store".to_owned()])
        );
        assert_eq!(summary.star_imports.len(), 1);
        assert!(summary.imports.contains_key("AliasedHelper"));
        assert_eq!(summary.non_routing_items, []);
    }

    #[rstest::rstest]
    #[case::non_literal_element("__all__ = [\"Store\", dynamic_name()]")]
    #[case::augmented_assign("__all__ = [\"Store\"]\n__all__ += extra_exports")]
    #[case::reassigned_dynamic("__all__ = [\"Store\"]\n__all__ = compute_exports()")]
    fn summarize_python_file_falls_back_to_implicit_exports_on_dynamic_dunder_all(
        #[case] source: &str,
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let summary = summarize_python_file(&file);
        assert_eq!(summary.exported_all, ExportedAll::Implicit);
    }

    #[test]
    fn augmented_dunder_all_with_static_list_extends_explicit_exports() {
        let source = indoc::indoc! {r#"
            __all__ = ["Store"]
            __all__ += ["Extra"]
        "#};
        let file = ParsedFile::new(source, Language::Python);
        let summary = summarize_python_file(&file);
        assert_eq!(
            summary.exported_all,
            ExportedAll::Explicit(vec!["Store".to_owned(), "Extra".to_owned()])
        );
    }

    #[test]
    fn bare_type_annotation_without_value_does_not_define_name() {
        let source = indoc::indoc! {r"
            from pkg.models import User
            User: type
            assigned: int = 1
        "};
        let file = ParsedFile::new(source, Language::Python);
        let summary = summarize_python_file(&file);
        assert!(!summary.defined_names.contains("User"));
        assert!(summary.defined_names.contains("assigned"));
    }

    #[test]
    fn summarize_python_file_tracks_type_checking_only_imports() {
        let source = indoc::indoc! {r"
            from typing import TYPE_CHECKING
            if TYPE_CHECKING:
                from .interfaces import OnlyTyped, Both
            if not TYPE_CHECKING:
                from .runtime import RuntimeOnly, Both
            else:
                from .interfaces import NegatedTyped
        "};
        let file = ParsedFile::new(source, Language::Python);
        let summary = summarize_python_file(&file);
        assert!(summary.imports["OnlyTyped"].type_checking_only);
        assert!(summary.imports["NegatedTyped"].type_checking_only);
        assert!(!summary.imports["RuntimeOnly"].type_checking_only);
        assert!(!summary.imports["Both"].type_checking_only);
    }
}
