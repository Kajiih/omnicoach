//! What evaluating a Rust expression does: function, method, and macro calls.

use ra_ap_syntax::ast;
use ra_ap_syntax::{AstNode as _, SyntaxKind};

/// Returns true if `expr` contains a function call, method call, or macro invocation.
pub(super) fn expr_contains_call(expr: &ast::Expr) -> bool {
    expr.syntax().descendants().any(|node| {
        matches!(
            node.kind(),
            SyntaxKind::CALL_EXPR | SyntaxKind::METHOD_CALL_EXPR | SyntaxKind::MACRO_EXPR
        )
    })
}
