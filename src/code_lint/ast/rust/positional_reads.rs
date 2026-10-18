//! Positional tuple-field read extraction for Rust (`ra_ap_syntax`).

use crate::code_lint::ast::{
    AstNode, ParsedFile, PositionalRead, ScopePositionalReads, span_from_rowan_range,
};
use ra_ap_syntax::ast;
use ra_ap_syntax::{AstNode as _, SyntaxKind, SyntaxNode};

/// Collects Rust tuple-field reads grouped by function (see [`super::collect_positional_reads`]).
#[must_use]
pub(in crate::code_lint::ast) fn collect_positional_reads(
    file: &ParsedFile,
) -> Vec<ScopePositionalReads<'_>> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_positional_reads_rec(parsed.tree().syntax(), file, None, &mut out);
    out
}

/// Walks `node`, recording reads and mutations into the enclosing function's `scope` (none
/// outside functions; closures belong to it) and pushing each finished function scope to `out`.
fn collect_positional_reads_rec<'a>(
    node: &SyntaxNode,
    file: &'a ParsedFile,
    mut scope: Option<&mut ScopePositionalReads<'a>>,
    out: &mut Vec<ScopePositionalReads<'a>>,
) {
    if ast::Fn::can_cast(node.kind()) {
        let mut function_scope = ScopePositionalReads::default();
        for child in node.children() {
            collect_positional_reads_rec(&child, file, Some(&mut function_scope), out);
        }
        out.push(function_scope);
        return;
    }
    if let Some(field_expr) = ast::FieldExpr::cast(node.clone())
        && let Some(active_scope) = scope.as_deref_mut()
    {
        record_field_expression(&field_expr, file, active_scope);
    }
    for child in node.children() {
        collect_positional_reads_rec(&child, file, scope.as_deref_mut(), out);
    }
}

/// Records a tuple-field access (`span.0`) as a positional read, or its receiver as an exempt
/// receiver when the field is mutated.
fn record_field_expression<'a>(
    field_expr: &ast::FieldExpr,
    file: &'a ParsedFile,
    scope: &mut ScopePositionalReads<'a>,
) {
    let (Some(receiver), Some(field_name)) = (field_expr.expr(), field_expr.name_ref()) else {
        return;
    };
    if field_name
        .syntax()
        .first_token()
        .is_none_or(|token| token.kind() != SyntaxKind::INT_NUMBER)
    {
        return;
    }
    let receiver_span = span_from_rowan_range(receiver.syntax().text_range());
    let receiver_text = file.source[receiver_span.start..receiver_span.end].to_string();
    if is_mutated(field_expr.syntax()) {
        scope.exempt_receivers.insert(receiver_text);
    } else if is_stable_receiver(&receiver)
        && let Ok(position) = field_name.text().parse()
    {
        scope.reads.push(PositionalRead {
            node: AstNode::from_span(
                file,
                span_from_rowan_range(field_expr.syntax().text_range()),
            ),
            receiver: receiver_text,
            position,
        });
    }
}

/// Returns true if `node` is assigned to (`t.0 = x`, `t.0 += x`, `(t.0, t.1) = (b, a)`) or
/// mutably borrowed (`&mut t.0`).
fn is_mutated(node: &SyntaxNode) -> bool {
    let mut place = node.clone();
    while let Some(parent) = place.parent() {
        match parent.kind() {
            SyntaxKind::TUPLE_EXPR | SyntaxKind::PAREN_EXPR => place = parent,
            SyntaxKind::BIN_EXPR => {
                let Some(bin_expr) = ast::BinExpr::cast(parent) else {
                    return false;
                };
                if !matches!(bin_expr.op_kind(), Some(ast::BinaryOp::Assignment { .. })) {
                    return false;
                }
                return bin_expr
                    .lhs()
                    .is_some_and(|left| left.syntax().text_range() == place.text_range());
            }
            SyntaxKind::REF_EXPR => {
                return ast::RefExpr::cast(parent)
                    .is_some_and(|ref_expr| ref_expr.mut_token().is_some());
            }
            _ => return false,
        }
    }
    false
}

/// Returns true if `receiver` names a stable value: a single-segment name/`self` or field chain
/// with no call inside, so that identical text means the same value.
fn is_stable_receiver(receiver: &ast::Expr) -> bool {
    let is_name_or_field = match receiver {
        ast::Expr::PathExpr(path_expr) => path_expr
            .path()
            .is_some_and(|path| path.qualifier().is_none()),
        ast::Expr::FieldExpr(_) => true,
        _ => false,
    };
    is_name_or_field && !super::expr_contains_call(receiver)
}
