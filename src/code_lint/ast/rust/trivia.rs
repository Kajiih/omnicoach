//! Rust comments (including doc comments) and whitespace.

use crate::code_lint::ast::{ParsedFile, span_from_rowan_range};
use crate::diagnostic::SourceSpan;
use ra_ap_syntax::{AstNode as _, SyntaxKind, SyntaxToken};

/// Collects the spans of all comment tokens in `file` in source order.
pub(in crate::code_lint::ast) fn collect_comment_spans(file: &ParsedFile) -> Vec<SourceSpan> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    parsed
        .tree()
        .syntax()
        .descendants_with_tokens()
        .filter_map(|element| {
            let token = element.into_token()?;
            is_rust_comment_kind(token.kind()).then(|| span_from_rowan_range(token.text_range()))
        })
        .collect()
}

/// Returns true if `token` is whitespace or a non-doc Rust comment (`//`, `/* */`).
pub(super) fn is_rust_non_doc_trivia(token: &SyntaxToken) -> bool {
    matches!(token.kind(), SyntaxKind::WHITESPACE)
        || (is_rust_comment_kind(token.kind()) && !is_rust_doc_comment(token.text()))
}

/// Returns true if `text` is a Rust outer or inner doc comment (`///`, `//!`, `/** */`, `/*! */`).
///
/// Per the Rust Reference, `////...`, `/***...`, and `/**/` are ordinary non-doc comments.
#[must_use]
pub fn is_rust_doc_comment(text: &str) -> bool {
    (text.starts_with("///") && !text.starts_with("////"))
        || text.starts_with("//!")
        || (text.starts_with("/**") && !text.starts_with("/***") && text != "/**/")
        || text.starts_with("/*!")
}

/// Returns true if `kind` is a Rust comment (`//`, `/* */`, `///`, `//!`, `/** */`, `/*! */`)
/// or whitespace token.
pub(super) const fn is_rust_trivia(kind: SyntaxKind) -> bool {
    matches!(kind, SyntaxKind::WHITESPACE) || is_rust_comment_kind(kind)
}

/// Returns true if `kind` is a Rust comment token (`//`, `/* */`, `///`, `//!`, `/** */`, `/*! */`).
pub(super) const fn is_rust_comment_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::COMMENT | SyntaxKind::OUTER_DOC_COMMENT | SyntaxKind::INNER_DOC_COMMENT
    )
}

/// Returns the previous non-trivia token before `token`.
pub(super) fn previous_non_trivia_token(token: &SyntaxToken) -> Option<SyntaxToken> {
    std::iter::successors(token.prev_token(), SyntaxToken::prev_token)
        .find(|candidate| !candidate.kind().is_trivia())
}
