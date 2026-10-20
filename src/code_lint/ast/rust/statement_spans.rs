//! Header and attribute extents of Rust statements and items.

use super::tree::syntax_node_at_span;
use super::trivia::{is_rust_non_doc_trivia, is_rust_trivia};
use crate::code_lint::ast::tree::{ParsedFile, span_from_rowan_range};
use crate::diagnostic::SourceSpan;
use ra_ap_syntax::ast;
use ra_ap_syntax::{AstNode as _, SyntaxElement, SyntaxKind, SyntaxNode};

/// Resolves the byte span of the header of the innermost Rust statement enclosing `span`.
pub(in crate::code_lint::ast) fn enclosing_statement_header_span(
    file: &ParsedFile,
    span: SourceSpan,
) -> Option<SourceSpan> {
    let start_node = syntax_node_at_span(file, span)?;
    let statement = start_node.ancestors().find(|candidate| {
        candidate
            .parent()
            .is_some_and(|parent| is_rust_statement_container(parent.kind()))
    })?;
    Some(rust_header_span(&statement))
}

/// Returns true for Rust `SyntaxKind`s that hold statements or items as direct children.
const fn is_rust_statement_container(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::SOURCE_FILE
            | SyntaxKind::STMT_LIST
            | SyntaxKind::ASSOC_ITEM_LIST
            | SyntaxKind::ITEM_LIST
            | SyntaxKind::EXTERN_ITEM_LIST
    )
}

/// Computes the byte span of a Rust `statement` node's header.
///
/// Leading whitespace and non-doc comments attached inside `statement` are skipped when
/// determining the header start so that outer doc comments (`///`, `/** */`) and `#[...]`
/// outer attributes start the header and preceding `//` explanation comments are recognized
/// above it.
fn rust_header_span(statement: &SyntaxNode) -> SourceSpan {
    let fallback_start = usize::from(statement.text_range().start());
    let start_offset = statement
        .descendants_with_tokens()
        .filter_map(SyntaxElement::into_token)
        .find(|token| !is_rust_non_doc_trivia(token))
        .map_or(fallback_start, |token| {
            usize::from(token.text_range().start())
        });
    let mut header_end_offset = start_offset;

    for child in statement.children_with_tokens() {
        if is_rust_body_container(child.kind()) {
            break;
        }
        if !is_rust_trivia(child.kind()) {
            header_end_offset = usize::from(child.text_range().end());
        }
    }

    SourceSpan::new(start_offset, header_end_offset)
}

/// Returns true for Rust `SyntaxKind`s that form the braced body of a compound statement or item.
const fn is_rust_body_container(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::BLOCK_EXPR
            | SyntaxKind::STMT_LIST
            | SyntaxKind::ASSOC_ITEM_LIST
            | SyntaxKind::ITEM_LIST
            | SyntaxKind::EXTERN_ITEM_LIST
    )
}

/// Records the byte span of every Rust attribute in `file`.
pub(in crate::code_lint::ast) fn record_decorator_and_attribute_spans(
    file: &ParsedFile,
    record: &mut impl FnMut(SourceSpan),
) {
    let Some(parsed) = file.rs_parsed() else {
        return;
    };
    for attribute in parsed
        .tree()
        .syntax()
        .descendants()
        .filter_map(ast::Attr::cast)
    {
        record(span_from_rowan_range(attribute.syntax().text_range()));
    }
}
