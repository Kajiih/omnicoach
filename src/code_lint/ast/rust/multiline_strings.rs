//! Multiline string literal analysis for Rust (`ra_ap_syntax`).

use super::attributes::attribute_terminal_name;
use super::names::{compact_path_text, resolve_preceding_macro_path};
use super::trivia::previous_non_trivia_token;
use crate::code_lint::ast::{AstNode, ParsedFile, span_from_rowan_range};
use ra_ap_syntax::ast;
use ra_ap_syntax::{AstNode as _, SyntaxElement, SyntaxKind, SyntaxToken};

/// Collects all Rust multiline string literal nodes in `file` that are not doc attributes,
/// `insta` inline snapshots, or enclosed in a macro matching `is_wrapper`.
#[must_use]
pub(in crate::code_lint::ast) fn find_unwrapped_multiline_strings(
    file: &ParsedFile,
    is_wrapper: impl Fn(&str) -> bool,
) -> Vec<AstNode<'_>> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    parsed
        .tree()
        .syntax()
        .descendants_with_tokens()
        .filter_map(SyntaxElement::into_token)
        .filter(|token| {
            is_multiline_string_token(token)
                && !is_insta_inline_snapshot(token)
                && !is_enclosed_in_doc_attribute(token)
                && !is_enclosed_in_macro(token, &is_wrapper)
        })
        .map(|token| AstNode::from_span(file, span_from_rowan_range(token.text_range())))
        .collect()
}

/// Returns true if `token` is a Rust string literal token that spans multiple lines and contains
/// runtime newlines (raw strings spanning lines, or standard strings with at least one intermediate
/// line not ending in a `\` line continuation).
#[must_use]
fn is_multiline_string_token(token: &SyntaxToken) -> bool {
    if !matches!(
        token.kind(),
        SyntaxKind::STRING | SyntaxKind::BYTE_STRING | SyntaxKind::C_STRING
    ) {
        return false;
    }
    let text = token.text();
    if !text.contains('\n') {
        return false;
    }
    if text.starts_with('r') || text.starts_with("br") || text.starts_with("cr") {
        return true;
    }
    let mut lines = text.lines();
    lines.next_back();
    lines.any(|line| !line.trim_end().ends_with('\\'))
}

/// Returns true if `token` is preceded by `@` (an `insta` inline snapshot literal `@"..."`).
#[must_use]
fn is_insta_inline_snapshot(token: &SyntaxToken) -> bool {
    previous_non_trivia_token(token).is_some_and(|previous| previous.kind() == SyntaxKind::AT)
}

/// Returns true if `token` is enclosed inside a `#[doc = "..."]` attribute.
#[must_use]
fn is_enclosed_in_doc_attribute(token: &SyntaxToken) -> bool {
    token
        .parent_ancestors()
        .filter_map(ast::Attr::cast)
        .any(|attribute| is_doc_attribute(&attribute))
}

/// Returns true if `attribute` represents a `#[doc = "..."]` attribute.
#[must_use]
fn is_doc_attribute(attribute: &ast::Attr) -> bool {
    attribute_terminal_name(attribute).is_some_and(|terminal| terminal.text() == "doc")
}

/// Returns true if `token` is enclosed in a Rust `macro_invocation` (or nested macro `token_tree`)
/// within the current scope whose path satisfies `predicate`.
#[must_use]
fn is_enclosed_in_macro(token: &SyntaxToken, predicate: &impl Fn(&str) -> bool) -> bool {
    for ancestor in token.parent_ancestors() {
        if matches!(ancestor.kind(), SyntaxKind::FN | SyntaxKind::CLOSURE_EXPR) {
            break;
        }
        if let Some(macro_call) = ast::MacroCall::cast(ancestor.clone()) {
            let full_path = macro_call.path().map_or_else(String::new, |path| {
                compact_path_text(&path.syntax().text().to_string())
            });
            let normalized = full_path.trim_start_matches("::");
            if predicate(normalized) {
                return true;
            }
        } else if ancestor.kind() == SyntaxKind::TOKEN_TREE
            && let Some(macro_path) = resolve_preceding_macro_path(&ancestor)
            && predicate(macro_path.path.trim_start_matches("::"))
        {
            return true;
        }
    }
    false
}
