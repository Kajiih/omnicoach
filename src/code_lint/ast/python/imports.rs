//! Python file-level import and definition map extraction.

use crate::code_lint::ast::imports::ImportMap;
use crate::code_lint::ast::tree::ParsedFile;
use crate::diagnostic::LineIndex;
use ruff_python_ast::{ExceptHandler, Stmt};

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
            _ => {}
        }
    }
}
