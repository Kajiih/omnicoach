//! Statement-level AST structure shared across supported languages.
//!
//! Provides the two structural facts that comment-attachment analysis depends on:
//! which statement encloses a given node, and which lines of that statement form its
//! header (the part preceding its body).
//!
//! The traversal is language-agnostic; the grammar vocabulary it relies on is not, and
//! lives in [`crate::code_lint::ast::python`] and [`crate::code_lint::ast::rust`].

architecture_component!(CodeSyntaxAdapters);

use crate::code_lint::ast::{ParsedFile, RawNode, dispatch_lang};
use crate::diagnostic::SourceSpan;
use ast_grep_language::SupportLang;
use std::ops::RangeInclusive;

/// Returns true for node kinds that hold statements as direct children in `lang`.
///
/// Unsupported languages report no containers, which makes callers behave as though no
/// enclosing statement exists rather than guessing with another grammar's vocabulary.
fn is_statement_container(kind: &str, lang: SupportLang) -> bool {
    dispatch_lang!(lang, is_statement_container(kind), false)
}

/// Returns the innermost statement enclosing `node`.
///
/// A statement is any node that sits directly inside a statement container, so this is
/// `node` itself or its closest such ancestor. Deriving it from the container relation
/// rather than from a list of statement kinds keeps every grammar construct covered.
#[must_use]
pub(super) fn find_enclosing_statement<'a>(node: &RawNode<'a>) -> Option<RawNode<'a>> {
    let lang = *node.lang();
    std::iter::once(node.clone())
        .chain(node.ancestors())
        .find(|candidate| {
            candidate
                .parent()
                .is_some_and(|parent| is_statement_container(parent.kind().as_ref(), lang))
        })
}

/// Returns the 1-indexed inclusive line range of `statement`'s header.
///
/// The header is everything up to and including the last syntax that precedes the
/// statement's body, where the body is the first direct child that is itself a statement
/// container: Python's `:`-introduced `block`, or Rust's braced `block` /
/// `declaration_list`. A statement with no such child (a simple statement) is entirely
/// header.
///
/// Extra nodes are skipped. Tree-sitter admits trivia such as comments as `extra` nodes
/// anywhere in the tree, so a comment sitting between the header and the body is a direct
/// child of the statement; counting it would stretch the header over the body's first line.
///
/// Anything after the header belongs to the body and must not be read as documentation of
/// the statement itself.
#[must_use]
pub(super) fn header_line_range(statement: &RawNode<'_>) -> RangeInclusive<usize> {
    let lang = *statement.lang();
    let start_line = statement.start_pos().line() + 1;
    let mut header_end_line = start_line;
    for child in statement.children() {
        if is_statement_container(child.kind().as_ref(), lang) {
            return start_line..=header_end_line;
        }
        if !child.is_extra() {
            header_end_line = child.end_pos().line() + 1;
        }
    }
    start_line..=statement.end_pos().line() + 1
}

/// Resolves the 1-indexed inclusive header line range of the innermost statement enclosing `span`.
#[must_use]
pub fn enclosing_statement_header_range(
    file: &ParsedFile,
    span: SourceSpan,
) -> Option<RangeInclusive<usize>> {
    let node = file
        .grep
        .root()
        .dfs()
        .filter(|candidate| {
            candidate.range().start <= span.start && candidate.range().end >= span.end
        })
        .min_by_key(|candidate| candidate.range().end - candidate.range().start)?;

    let statement = find_enclosing_statement(&node)?;
    Some(header_line_range(&statement))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast_grep_language::SupportLang;
    use indoc::indoc;
    use rstest::rstest;

    /// Resolves the statement enclosing the first node of `target_kind` whose text starts
    /// with `target_prefix`, and returns its kind and header line range.
    fn enclosing_statement_of(
        source: &str,
        lang: SupportLang,
        target_kind: &str,
        target_prefix: &str,
    ) -> (String, RangeInclusive<usize>) {
        let grep = ast_grep_core::AstGrep::new(source, lang);
        let target = grep
            .root()
            .dfs()
            .find(|node| node.kind() == target_kind && node.text().starts_with(target_prefix))
            .expect("target node should exist in source snippet");
        let statement =
            find_enclosing_statement(&target).expect("target should be inside a statement");
        (statement.kind().to_string(), header_line_range(&statement))
    }

    #[rstest]
    // Compound statements: the header stops at the line introducing the body, so body
    // comments can never be mistaken for header explanations.
    #[case::python_multiline_with(
        indoc! {r#"
            with (
                open("file.txt"),
                suppress(FileNotFoundError),
            ):
                pass
        "#},
        SupportLang::Python, "call", "suppress", "with_statement", 1..=4
    )]
    #[case::python_if(
        indoc! {r"
            if (
                ready()
            ):
                pass
        "},
        SupportLang::Python, "call", "ready", "if_statement", 1..=3
    )]
    #[case::python_for(
        indoc! {r"
            for item in fetch():
                pass
        "},
        SupportLang::Python, "call", "fetch", "for_statement", 1..=1
    )]
    #[case::python_try(
        indoc! {r"
            try:
                run()
            except ValueError:
                pass
        "},
        SupportLang::Python, "call", "run", "expression_statement", 2..=2
    )]
    // Simple statements have no body, so the whole statement is header.
    #[case::python_multiline_assignment(
        indoc! {r#"
            CONFIG = (
                get_config("FALLBACK")
            )
        "#},
        SupportLang::Python, "call", "get_config", "expression_statement", 1..=3
    )]
    #[case::python_annotated_assignment(
        indoc! {r"
            CONFIG: dict[str, str] = build()
        "},
        SupportLang::Python, "call", "build", "expression_statement", 1..=1
    )]
    // Rust: a braced block is a body just like a Python suite, so the header stops before it.
    #[case::rust_let_with_block_value(
        indoc! {r"
            fn f() {
                let x = {
                    compute()
                };
            }
        "},
        SupportLang::Rust, "block", "{\n        compute", "let_declaration", 2..=2
    )]
    #[case::rust_simple_let(
        indoc! {r"
            fn f() {
                let x = compute();
            }
        "},
        SupportLang::Rust, "call_expression", "compute", "let_declaration", 2..=2
    )]
    #[case::rust_const_item(
        indoc! {r"
            const LIMIT: usize = compute();
        "},
        SupportLang::Rust, "call_expression", "compute", "const_item", 1..=1
    )]
    fn test_statement_and_header_resolution(
        #[case] source: &str,
        #[case] lang: SupportLang,
        #[case] target_kind: &str,
        #[case] target_prefix: &str,
        #[case] expected_kind: &str,
        #[case] expected_header: RangeInclusive<usize>,
    ) {
        let (kind, header) = enclosing_statement_of(source, lang, target_kind, target_prefix);
        assert_eq!(kind, expected_kind);
        assert_eq!(header, expected_header);
    }

    #[rstest]
    #[case::python(
        indoc! {r"
            with suppress(Exception):
                cleanup()
        "},
        SupportLang::Python, "call", "cleanup", "expression_statement"
    )]
    #[case::rust(
        indoc! {r"
            fn f() {
                let x = {
                    compute()
                };
            }
        "},
        SupportLang::Rust, "call_expression", "compute", "call_expression"
    )]
    fn test_node_in_body_resolves_to_inner_statement_not_outer(
        #[case] source: &str,
        #[case] lang: SupportLang,
        #[case] target_kind: &str,
        #[case] target_prefix: &str,
        #[case] expected_kind: &str,
    ) {
        let (kind, _) = enclosing_statement_of(source, lang, target_kind, target_prefix);
        assert_eq!(kind, expected_kind);
    }

    #[test]
    fn test_comment_between_header_and_body_does_not_extend_header() {
        let source = indoc! {r"
            with suppress(Exception):
                # Explanation that belongs to the body, not the header
                pass
        "};
        let (_, header) = enclosing_statement_of(source, SupportLang::Python, "call", "suppress");
        assert_eq!(header, 1..=1);
    }
}
