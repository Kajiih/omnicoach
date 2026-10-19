//! Navigating the Rust syntax tree (`ra_ap_syntax`): finding and casting nodes by span.

use crate::code_lint::ast::{AstNode, ParsedFile};
use crate::diagnostic::SourceSpan;
use ra_ap_syntax::{AstNode as _, SyntaxElement, SyntaxNode};

/// Casts the syntax node at `node.span()` (or an exact-range wrapper ancestor) to `T`.
pub(super) fn cast_at_span<T: ra_ap_syntax::AstNode>(node: &AstNode<'_>) -> Option<T> {
    let inner = syntax_node(node)?;
    let target_range = inner.text_range();
    inner
        .ancestors()
        .take_while(|ancestor| ancestor.text_range() == target_range)
        .find_map(T::cast)
}

/// Resolves the deepest [`SyntaxNode`] in `node`'s file covering `node.span()`.
pub(super) fn syntax_node(node: &AstNode<'_>) -> Option<SyntaxNode> {
    syntax_node_at_span(node.file, node.span())
}

/// Resolves the deepest [`SyntaxNode`] in `file` covering `span`.
pub(super) fn syntax_node_at_span(file: &ParsedFile, span: SourceSpan) -> Option<SyntaxNode> {
    let parsed = file.rs_parsed()?;
    let start_offset = u32::try_from(span.start).ok()?;
    let end_offset = u32::try_from(span.end).ok()?;
    let range = ra_ap_syntax::TextRange::new(start_offset.into(), end_offset.into());
    match parsed.tree().syntax().covering_element(range) {
        SyntaxElement::Node(found) => Some(found),
        SyntaxElement::Token(token) => token.parent(),
    }
}
