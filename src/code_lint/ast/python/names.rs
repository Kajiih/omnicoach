//! What a Python expression names: dotted path extraction.

use crate::code_lint::ast::{QualifiedName, span_from_ruff_range};
use ruff_python_ast::Expr;
use ruff_text_size::Ranged as _;

/// The dotted path and terminal identifier of `expr` as written in `source`.
pub(super) fn written_name(expr: &Expr, source: &str) -> QualifiedName {
    let span = span_from_ruff_range(expr.range());
    let path = source[span.start..span.end].to_string();
    let terminal = if let Expr::Attribute(attr) = expr {
        attr.attr.to_string()
    } else {
        path.rsplit('.').next().unwrap_or("").to_string()
    };
    QualifiedName { path, terminal }
}
