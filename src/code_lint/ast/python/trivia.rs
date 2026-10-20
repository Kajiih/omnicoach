//! Python comments and whitespace.

use crate::code_lint::ast::tree::{ParsedFile, span_from_ruff_range};
use crate::diagnostic::SourceSpan;
use ruff_python_ast::token::TokenKind;
use ruff_text_size::Ranged as _;

/// Collects the spans of all comment tokens in `file` in source order, or none if `file` failed
/// to parse.
pub(in crate::code_lint::ast) fn collect_comment_spans(file: &ParsedFile) -> Vec<SourceSpan> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    parsed
        .tokens()
        .iter()
        .filter(|token| token.kind() == TokenKind::Comment)
        .map(|token| span_from_ruff_range(token.range()))
        .collect()
}
