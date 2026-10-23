//! Multiline string literal analysis for Rust (`ra_ap_syntax`).

use super::attributes::attribute_terminal_name;
use super::names::resolve_preceding_macro_path;
use super::trivia::previous_non_trivia_token;
use crate::code_lint::ast::multiline_strings::{
    MultilineKind, MultilineString, escaped_multiline_newlines,
};
use crate::code_lint::ast::tree::{AstNode, ParsedFile, span_from_rowan_range};
use ra_ap_syntax::ast;
use ra_ap_syntax::{AstNode as _, AstToken as _, SyntaxElement, SyntaxKind, SyntaxToken};

/// Collects all multiline string literals in a Rust `file` (see
/// [`crate::code_lint::ast::collect_multiline_strings`]).
#[must_use]
pub fn collect_multiline_strings(file: &ParsedFile) -> Vec<MultilineString<'_>> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    parsed
        .tree()
        .syntax()
        .descendants_with_tokens()
        .filter_map(SyntaxElement::into_token)
        .filter_map(|token| {
            let kind = token_multiline_kind(file, &token)?;
            Some(MultilineString {
                node: AstNode::from_span(file, span_from_rowan_range(token.text_range())),
                kind,
            })
        })
        .collect()
}

/// Classifies `token` as a source or escaped multiline string literal, or `None` when it is not a
/// string literal, is a doc attribute or `insta` inline snapshot, or is a single-line literal.
fn token_multiline_kind(file: &ParsedFile, token: &SyntaxToken) -> Option<MultilineKind> {
    if !matches!(
        token.kind(),
        SyntaxKind::STRING | SyntaxKind::BYTE_STRING | SyntaxKind::C_STRING
    ) || is_insta_inline_snapshot(token)
        || is_enclosed_in_doc_attribute(token)
    {
        return None;
    }
    if is_source_multiline_token(token) {
        return Some(MultilineKind::Source {
            enclosing_calls: enclosing_macro_paths(file, token),
        });
    }
    token_escaped_newlines(token).map(|newlines| MultilineKind::Escaped { newlines })
}

/// Returns true if `token` spans multiple source lines and contains runtime newlines (raw strings
/// spanning lines, or standard strings with at least one non-final line not ending in `\`).
#[must_use]
fn is_source_multiline_token(token: &SyntaxToken) -> bool {
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

/// Decodes a single-line `STRING`, `BYTE_STRING`, or `C_STRING` token and returns its escaped
/// `\n` count when it spans at least two non-blank lines.
fn token_escaped_newlines(token: &SyntaxToken) -> Option<usize> {
    ast::String::cast(token.clone())
        .and_then(|string| {
            string
                .value()
                .ok()
                .and_then(|val| escaped_multiline_newlines(val.as_bytes()))
        })
        .or_else(|| {
            ast::ByteString::cast(token.clone()).and_then(|byte_string| {
                byte_string
                    .value()
                    .ok()
                    .and_then(|val| escaped_multiline_newlines(&val))
            })
        })
        .or_else(|| {
            ast::CString::cast(token.clone()).and_then(|c_string| {
                c_string
                    .value()
                    .ok()
                    .and_then(|val| escaped_multiline_newlines(&val))
            })
        })
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

/// Collects the canonical import-resolved paths of non-local macro invocations enclosing `token`
/// within its innermost function or closure scope.
fn enclosing_macro_paths(file: &ParsedFile, token: &SyntaxToken) -> Vec<String> {
    token
        .parent_ancestors()
        .take_while(|ancestor| {
            !matches!(ancestor.kind(), SyntaxKind::FN | SyntaxKind::CLOSURE_EXPR)
        })
        .filter(|ancestor| ancestor.kind() == SyntaxKind::TOKEN_TREE)
        .filter_map(|ancestor| {
            resolve_preceding_macro_path(&ancestor)
                .and_then(|name| name.resolve(file))
                .map(|name| name.path)
        })
        .collect()
}
