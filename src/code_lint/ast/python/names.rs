//! What a Python expression names: dotted path extraction and import-resolved names.

use crate::code_lint::ast::{ParsedFile, QualifiedName, span_from_ruff_range};
use ruff_python_ast::Expr;
use ruff_text_size::Ranged as _;

/// The name that `expr` names, resolved through the imports of `file`
/// (`t.List` after `import typing as t` is `typing.List` with terminal `List`), or `None`
/// if it names a definition of `file` (see [`QualifiedName::resolve`]).
pub(super) fn resolved_path_and_terminal_expr(
    expr: &Expr,
    file: &ParsedFile,
) -> Option<QualifiedName> {
    resolve_path_and_terminal_expr(expr, &file.source).resolve(file)
}

/// Helper to resolve the dotted expression path and terminal identifier from an `Expr`.
pub(super) fn resolve_path_and_terminal_expr(expr: &Expr, source: &str) -> QualifiedName {
    let span = span_from_ruff_range(expr.range());
    let path = source[span.start..span.end].to_string();
    let terminal = if let Expr::Attribute(attr) = expr {
        attr.attr.to_string()
    } else {
        path.rsplit('.').next().unwrap_or("").to_string()
    };
    QualifiedName { path, terminal }
}
