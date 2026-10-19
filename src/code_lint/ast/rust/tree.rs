//! Navigating the Rust syntax tree (`ra_ap_syntax`): finding and casting nodes by span, and
//! inspecting `TokenTree` elements.

use crate::code_lint::ast::{AstNode, ParsedFile, span_from_rowan_range};
use crate::diagnostic::SourceSpan;
use ra_ap_syntax::ast;
use ra_ap_syntax::{AstNode as _, SyntaxElement, SyntaxKind, SyntaxNode};

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

/// Extracts the argument nodes inside a Rust `macro_invocation`'s `token_tree`.
#[must_use]
pub(super) fn extract_macro_arguments<'a>(macro_node: &AstNode<'a>) -> Vec<AstNode<'a>> {
    let Some(macro_call) = cast_at_span::<ast::MacroCall>(macro_node) else {
        return Vec::new();
    };
    let Some(token_tree) = macro_call.token_tree() else {
        return Vec::new();
    };
    meaningful_token_tree_elements(&token_tree)
        .into_iter()
        .map(|element| {
            AstNode::from_span(macro_node.file, span_from_rowan_range(element.text_range()))
        })
        .collect()
}

/// Extracts the non-delimiter, non-trivia direct child elements of `token_tree`.
pub(super) fn meaningful_token_tree_elements(token_tree: &ast::TokenTree) -> Vec<SyntaxElement> {
    token_tree
        .syntax()
        .children_with_tokens()
        .filter(|element| !is_token_tree_delimiter_or_trivia(element))
        .collect()
}

/// Returns true if `element` is a delimiter or comma token inside a `TokenTree`.
fn is_token_tree_delimiter_or_trivia(element: &SyntaxElement) -> bool {
    match element {
        SyntaxElement::Node(_) => false,
        SyntaxElement::Token(token) => {
            token.kind().is_trivia()
                || matches!(
                    token.kind(),
                    SyntaxKind::L_PAREN
                        | SyntaxKind::R_PAREN
                        | SyntaxKind::L_BRACK
                        | SyntaxKind::R_BRACK
                        | SyntaxKind::L_CURLY
                        | SyntaxKind::R_CURLY
                        | SyntaxKind::COMMA
                )
        }
    }
}
