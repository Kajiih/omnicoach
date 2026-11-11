//! Python file-level import and definition map extraction.

use crate::code_lint::ast::imports::ImportMap;
use crate::code_lint::ast::tree::{ParsedFile, span_from_ruff_range};
use crate::diagnostic::{LineIndex, SourceSpan};
use ruff_python_ast::statement_visitor::{StatementVisitor, walk_body, walk_stmt};
use ruff_python_ast::{ExceptHandler, Expr, Stmt};

/// A Python `import` or `from ... import ...` reference extracted from a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonImportReference {
    /// Byte range of the enclosing import statement.
    pub span: SourceSpan,
    /// Relative import dot level (`0` for absolute imports, `1` for `from . import ...`, etc.).
    pub level: u32,
    /// Referenced module path (e.g. `Some("pkg.mod")` for `import pkg.mod` or `from pkg.mod import x`,
    /// `None` for `from . import x`).
    pub module: Option<String>,
    /// Imported symbol names (`vec!["x", "y"]` for `from pkg.mod import x, y`, `vec!["*"]` for
    /// wildcard, empty `vec![]` for `import pkg.mod`).
    pub imported_names: Vec<String>,
    /// Whether this import appears inside a `def` / `async def` body.
    pub in_function: bool,
    /// Whether this import appears inside the true branch of an `if TYPE_CHECKING:` guard.
    pub in_type_checking: bool,
}

/// Collects all `import` and `from ... import ...` references across `file`, annotating each
/// with its statement span, relative level, function scope flag, and `if TYPE_CHECKING:` flag.
#[must_use]
pub fn collect_python_import_references(file: &ParsedFile) -> Vec<PythonImportReference> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = PythonImportVisitor {
        _file: file,
        function_depth: 0,
        type_checking_depth: 0,
        references: Vec::new(),
    };
    walk_body(&mut visitor, &parsed.syntax().body);
    visitor.references
}

/// Collects the file-level imports and `def` / `class` definitions of `file` (see
/// [`crate::code_lint::ast::resolve_name`]).
#[must_use]
pub fn collect_import_map(file: &ParsedFile) -> ImportMap {
    let mut map = ImportMap::default();
    if let Some(parsed) = file.py_module() {
        collect_python_import_bindings(&parsed.syntax().body, &file.line_index, &mut map);
    }
    map
}

struct PythonImportVisitor<'a> {
    _file: &'a ParsedFile,
    function_depth: usize,
    type_checking_depth: usize,
    references: Vec<PythonImportReference>,
}

impl PythonImportVisitor<'_> {
    fn visit_guarded_body(&mut self, is_type_checking: bool, body: &[Stmt]) {
        if is_type_checking {
            self.type_checking_depth += 1;
        }
        walk_body(self, body);
        if is_type_checking {
            self.type_checking_depth -= 1;
        }
    }
}

impl StatementVisitor<'_> for PythonImportVisitor<'_> {
    fn visit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Import(import) => {
                let span = span_from_ruff_range(import.range);
                for alias in &import.names {
                    self.references.push(PythonImportReference {
                        span,
                        level: 0,
                        module: Some(alias.name.to_string()),
                        imported_names: Vec::new(),
                        in_function: self.function_depth > 0,
                        in_type_checking: self.type_checking_depth > 0,
                    });
                }
            }
            Stmt::ImportFrom(import_from) => {
                let span = span_from_ruff_range(import_from.range);
                let module = import_from.module.as_ref().map(ToString::to_string);
                let imported_names = import_from
                    .names
                    .iter()
                    .map(|alias| alias.name.to_string())
                    .collect();
                self.references.push(PythonImportReference {
                    span,
                    level: import_from.level,
                    module,
                    imported_names,
                    in_function: self.function_depth > 0,
                    in_type_checking: self.type_checking_depth > 0,
                });
            }
            Stmt::FunctionDef(_) => {
                self.function_depth += 1;
                walk_stmt(self, stmt);
                self.function_depth -= 1;
            }
            Stmt::If(if_statement) => {
                self.visit_guarded_body(
                    is_type_checking_guard(&if_statement.test),
                    &if_statement.body,
                );
                for clause in &if_statement.elif_else_clauses {
                    let clause_is_type_checking =
                        clause.test.as_ref().is_some_and(is_type_checking_guard);
                    self.visit_guarded_body(clause_is_type_checking, &clause.body);
                }
            }
            _ => walk_stmt(self, stmt),
        }
    }
}

const TYPE_CHECKING_GUARD: &str = "TYPE_CHECKING";

fn is_type_checking_guard(expr: &Expr) -> bool {
    match expr {
        Expr::Name(name) => name.id.as_str() == TYPE_CHECKING_GUARD,
        Expr::Attribute(attribute) => attribute.attr.as_str() == TYPE_CHECKING_GUARD,
        Expr::BoolOp(bool_op) if bool_op.op == ruff_python_ast::BoolOp::And => {
            bool_op.values.iter().any(is_type_checking_guard)
        }
        _ => false,
    }
}

/// Records the imports and definitions of a Python module body or top-level block `body`.
fn collect_python_import_bindings(body: &[Stmt], line_index: &LineIndex, map: &mut ImportMap) {
    for statement in body {
        match statement {
            Stmt::Import(import) => {
                let line = line_index.line(import.range.start().into());
                for alias in &import.names {
                    let module = alias.name.as_str();
                    map.import_lines.push((line, module.to_owned()));
                    if let Some(asname) = &alias.asname {
                        map.imports.insert(asname.to_string(), module.to_owned());
                    } else {
                        let package = module.split('.').next().unwrap_or(module);
                        map.imports.insert(package.to_owned(), package.to_owned());
                    }
                }
            }
            Stmt::ImportFrom(import_from) => {
                let line = line_index.line(import_from.range.start().into());
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
                    map.import_lines.push((line, path.clone()));
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
                collect_python_import_bindings(&if_statement.body, line_index, map);
                for clause in &if_statement.elif_else_clauses {
                    collect_python_import_bindings(&clause.body, line_index, map);
                }
            }
            Stmt::Try(try_statement) => {
                collect_python_import_bindings(&try_statement.body, line_index, map);
                for ExceptHandler::ExceptHandler(handler) in &try_statement.handlers {
                    collect_python_import_bindings(&handler.body, line_index, map);
                }
                collect_python_import_bindings(&try_statement.orelse, line_index, map);
                collect_python_import_bindings(&try_statement.finalbody, line_index, map);
            }
            Stmt::With(with_statement) => {
                collect_python_import_bindings(&with_statement.body, line_index, map);
            }
            Stmt::Match(match_statement) => {
                for case in &match_statement.cases {
                    collect_python_import_bindings(&case.body, line_index, map);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;

    #[test]
    fn collect_python_import_references_tracks_function_and_type_checking_scopes() {
        type ImportEntry<'a> = (&'a Option<String>, &'a [String], u32, bool, bool);
        let source = indoc::indoc! {"
            from typing import TYPE_CHECKING
            import pkg.core

            if TYPE_CHECKING:
                from ..models import User
            else:
                from .fallback import Guest

            def load():
                from .lazy import Loader
        "};
        let file = ParsedFile::new(source, Language::Python);
        let refs = collect_python_import_references(&file);
        let summary: Vec<ImportEntry<'_>> = refs
            .iter()
            .map(|reference| {
                (
                    &reference.module,
                    reference.imported_names.as_slice(),
                    reference.level,
                    reference.in_function,
                    reference.in_type_checking,
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (
                    &Some("typing".to_string()),
                    &["TYPE_CHECKING".to_string()][..],
                    0,
                    false,
                    false
                ),
                (&Some("pkg.core".to_string()), &[][..], 0, false, false),
                (
                    &Some("models".to_string()),
                    &["User".to_string()][..],
                    2,
                    false,
                    true
                ),
                (
                    &Some("fallback".to_string()),
                    &["Guest".to_string()][..],
                    1,
                    false,
                    false
                ),
                (
                    &Some("lazy".to_string()),
                    &["Loader".to_string()][..],
                    1,
                    true,
                    false
                ),
            ]
        );
    }
}
