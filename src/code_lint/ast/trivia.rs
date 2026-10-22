//! Comment node extraction across languages.

use super::tree::{AstNode, ParsedFile};

/// Collects all comment nodes in `file` in source order.
///
/// Non-comment trivia (such as Python line continuations) is never included.
#[must_use]
pub fn collect_comment_nodes(file: &ParsedFile) -> Vec<AstNode<'_>> {
    let spans = file
        .comment_spans
        .get_or_init(|| dispatch_lang!(file.lang(), collect_comment_spans(file)));
    spans
        .iter()
        .map(|&span| AstNode::from_span(file, span))
        .collect()
}
