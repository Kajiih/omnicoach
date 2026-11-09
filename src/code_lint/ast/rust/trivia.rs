//! Rust comments (including doc comments) and whitespace.

use crate::code_lint::ast::tree::{ParsedFile, span_from_rowan_range};
use crate::diagnostic::SourceSpan;
use ra_ap_syntax::{AstNode as _, SyntaxKind, SyntaxToken};

/// Collects the spans of all comment tokens in `file` in source order.
pub fn collect_comment_spans(file: &ParsedFile) -> Vec<SourceSpan> {
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

/// Returns the next non-trivia token after `token`.
pub(super) fn next_non_trivia_token(token: &SyntaxToken) -> Option<SyntaxToken> {
    std::iter::successors(token.next_token(), SyntaxToken::next_token)
        .find(|candidate| !candidate.kind().is_trivia())
}

/// Returns the non-trivia token immediately preceding a `::` separator (`COLON2` or two
/// byte-adjacent `COLON` tokens) before `token`, if any.
pub(super) fn token_before_double_colon(token: &SyntaxToken) -> Option<SyntaxToken> {
    let sep = previous_non_trivia_token(token)?;
    if sep.kind() == SyntaxKind::COLON2 {
        return previous_non_trivia_token(&sep);
    }
    if sep.kind() == SyntaxKind::COLON {
        let first_colon = previous_non_trivia_token(&sep)?;
        if first_colon.kind() == SyntaxKind::COLON
            && first_colon.text_range().end() == sep.text_range().start()
        {
            return previous_non_trivia_token(&first_colon);
        }
    }
    None
}

/// Returns the non-trivia token immediately following a `::` separator (`COLON2` or two
/// byte-adjacent `COLON` tokens) after `token`, if any.
pub(super) fn token_after_double_colon(token: &SyntaxToken) -> Option<SyntaxToken> {
    let sep = next_non_trivia_token(token)?;
    if sep.kind() == SyntaxKind::COLON2 {
        return next_non_trivia_token(&sep);
    }
    if sep.kind() == SyntaxKind::COLON {
        let second_colon = next_non_trivia_token(&sep)?;
        if second_colon.kind() == SyntaxKind::COLON
            && sep.text_range().end() == second_colon.text_range().start()
        {
            return next_non_trivia_token(&second_colon);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn ident_tokens_in_macro(source: &str) -> Vec<SyntaxToken> {
        let file = ParsedFile::rust(source);
        let parsed = file.rs_parsed().expect("valid Rust snippet");
        parsed
            .tree()
            .syntax()
            .descendants()
            .find(|node| node.kind() == SyntaxKind::TOKEN_TREE)
            .expect("macro token tree exists")
            .descendants_with_tokens()
            .filter_map(ra_ap_syntax::NodeOrToken::into_token)
            .filter(|token| token.kind() == SyntaxKind::IDENT)
            .collect()
    }

    #[rstest]
    #[case::adjacent_double_colon("mac!(left::right);", 0, 1, Some("right"), Some("left"))]
    #[case::spaced_colons("mac!(left : : right);", 0, 1, None, None)]
    #[case::single_colon_label("mac!(prefix label: value);", 1, 2, None, None)]
    #[case::single_colon_prefix("mac!(key :value suffix);", 0, 1, None, None)]
    fn test_double_colon_token_helpers(
        #[case] source: &str,
        #[case] left_idx: usize,
        #[case] right_idx: usize,
        #[case] expected_after: Option<&str>,
        #[case] expected_before: Option<&str>,
    ) {
        let idents = ident_tokens_in_macro(source);

        assert_eq!(
            token_after_double_colon(&idents[left_idx])
                .as_ref()
                .map(SyntaxToken::text),
            expected_after
        );
        assert_eq!(
            token_before_double_colon(&idents[right_idx])
                .as_ref()
                .map(SyntaxToken::text),
            expected_before
        );
    }
}
