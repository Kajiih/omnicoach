//! Rust call and method-call candidate extraction (`ra_ap_syntax`).

use super::named_function;
use crate::code_lint::ast::{
    CachedCallCandidate, EnclosingFunction, ParsedFile, span_from_rowan_range,
    with_innermost_function,
};
use ra_ap_syntax::AstNode as _;
use std::sync::Arc;

/// Collects all direct call and method-call expressions in a Rust file using `ra_ap_syntax`.
#[must_use]
pub(in crate::code_lint::ast) fn collect_call_candidates(
    file: &ParsedFile,
) -> Vec<CachedCallCandidate> {
    use ra_ap_syntax::ast::{self, HasArgList as _};

    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut enclosing_functions: Arc<[EnclosingFunction]> = Arc::default();
    let mut outer_functions = Vec::new();
    for event in parsed.tree().syntax().preorder() {
        let syntax_node = match event {
            ra_ap_syntax::WalkEvent::Enter(syntax_node) => syntax_node,
            ra_ap_syntax::WalkEvent::Leave(syntax_node) => {
                if named_function(&syntax_node).is_some()
                    && let Some(outer) = outer_functions.pop()
                {
                    enclosing_functions = outer;
                }
                continue;
            }
        };
        if let Some(function) = named_function(&syntax_node) {
            let inner = with_innermost_function(function, &enclosing_functions);
            outer_functions.push(std::mem::replace(&mut enclosing_functions, inner));
        }
        if let Some(call) = ast::CallExpr::cast(syntax_node.clone()) {
            let Some(func) = call.expr() else {
                continue;
            };
            let func_span = span_from_rowan_range(func.syntax().text_range());
            let callee = file.source[func_span.start..func_span.end].to_string();
            let argument_spans = call.arg_list().map_or_else(Vec::new, |arguments| {
                arguments
                    .args()
                    .map(|arg| span_from_rowan_range(arg.syntax().text_range()))
                    .collect()
            });
            out.push(CachedCallCandidate {
                span: span_from_rowan_range(call.syntax().text_range()),
                callee,
                method_name: None,
                receiver_call_callee: None,
                argument_spans,
                is_with_context_manager: false,
                is_in_except_clause: false,
                is_in_loop: is_in_rust_loop(call.syntax()),
                enclosing_functions: Arc::clone(&enclosing_functions),
            });
        } else if let Some(method_call) = ast::MethodCallExpr::cast(syntax_node) {
            let (Some(receiver), Some(name_ref)) = (method_call.receiver(), method_call.name_ref())
            else {
                continue;
            };
            let start: usize = receiver.syntax().text_range().start().into();
            let end: usize = name_ref.syntax().text_range().end().into();
            let callee = file.source[start..end].to_string();
            let method_name = Some(name_ref.text().to_string());
            let receiver_call_callee = rust_call_callee_text(&receiver, &file.source);
            let argument_spans = method_call.arg_list().map_or_else(Vec::new, |arguments| {
                arguments
                    .args()
                    .map(|arg| span_from_rowan_range(arg.syntax().text_range()))
                    .collect()
            });
            out.push(CachedCallCandidate {
                span: span_from_rowan_range(method_call.syntax().text_range()),
                callee,
                method_name,
                receiver_call_callee,
                argument_spans,
                is_with_context_manager: false,
                is_in_except_clause: false,
                is_in_loop: is_in_rust_loop(method_call.syntax()),
                enclosing_functions: Arc::clone(&enclosing_functions),
            });
        }
    }
    out
}

/// Returns true if `node` lies inside a Rust `loop`, `while`, or `for` body without an intervening
/// `fn` or closure boundary.
fn is_in_rust_loop(node: &ra_ap_syntax::SyntaxNode) -> bool {
    use ra_ap_syntax::ast::{self, HasLoopBody as _};

    let range = node.text_range();
    for ancestor in node.ancestors().skip(1) {
        if ast::Fn::can_cast(ancestor.kind()) || ast::ClosureExpr::can_cast(ancestor.kind()) {
            return false;
        }
        if ast::LoopExpr::can_cast(ancestor.kind()) || ast::WhileExpr::can_cast(ancestor.kind()) {
            return true;
        }
        if let Some(for_expr) = ast::ForExpr::cast(ancestor)
            && for_expr
                .loop_body()
                .is_some_and(|body| body.syntax().text_range().contains_range(range))
        {
            return true;
        }
    }
    false
}

/// Extracts the callee string of a Rust call or method-call expression.
fn rust_call_callee_text(expr: &ra_ap_syntax::ast::Expr, source: &str) -> Option<String> {
    match expr {
        ra_ap_syntax::ast::Expr::CallExpr(call) => {
            let func = call.expr()?;
            let span = span_from_rowan_range(func.syntax().text_range());
            Some(source[span.start..span.end].to_string())
        }
        ra_ap_syntax::ast::Expr::MethodCallExpr(method_call) => {
            let receiver = method_call.receiver()?;
            let name_ref = method_call.name_ref()?;
            let start: usize = receiver.syntax().text_range().start().into();
            let end: usize = name_ref.syntax().text_range().end().into();
            Some(source[start..end].to_string())
        }
        _ => None,
    }
}
