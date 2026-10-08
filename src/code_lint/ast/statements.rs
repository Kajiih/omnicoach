//! Statement-level AST structure shared across supported languages.
//!
//! Provides the two structural facts that comment-attachment analysis depends on:
//! which statement encloses a given span, and which lines of that statement form its
//! header (the part preceding its body).

use crate::code_lint::ast::{ParsedFile, span_from_ruff_range};
use crate::diagnostic::{Language, SourceSpan};
use std::ops::RangeInclusive;

/// Resolves the 1-indexed inclusive header line range of the innermost statement enclosing `span`.
#[must_use]
pub fn enclosing_statement_header_range(
    file: &ParsedFile,
    span: SourceSpan,
) -> Option<RangeInclusive<usize>> {
    match file.lang() {
        Language::Python => python_enclosing_statement_header_range(file, span),
        Language::Rust => {
            let statement = find_rust_enclosing_statement(file, span)?;
            Some(rust_header_line_range(file, &statement))
        }
    }
}

/// Resolves the 1-indexed inclusive header line range of the innermost Python statement
/// enclosing `span`.
fn python_enclosing_statement_header_range(
    file: &ParsedFile,
    span: SourceSpan,
) -> Option<RangeInclusive<usize>> {
    use ruff_python_ast::Stmt;
    use ruff_python_ast::token::TokenKind;
    use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_stmt};
    use ruff_text_size::Ranged as _;

    struct StatementFinder<'a> {
        target_span: SourceSpan,
        found: Option<&'a Stmt>,
    }

    impl<'a> SourceOrderVisitor<'a> for StatementFinder<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            let statement_span = span_from_ruff_range(statement.range());
            if !(statement_span.start <= self.target_span.start
                && self.target_span.end <= statement_span.end)
            {
                return;
            }
            self.found = Some(statement);
            walk_stmt(self, statement);
        }
    }

    let parsed = file.py_module()?;
    let mut finder = StatementFinder {
        target_span: span,
        found: None,
    };
    finder.visit_body(&parsed.syntax().body);
    let statement = finder.found?;

    let statement_range = statement.range();
    let start_line = file.line_index.line(statement_range.start().to_usize());
    let end_line = python_statement_body_start(statement).map_or_else(
        || file.line_index.line(statement_range.end().to_usize()),
        |body_start| {
            let header_end_offset = parsed
                .tokens()
                .iter()
                .rfind(|token| {
                    token.range().start() >= statement_range.start()
                        && token.range().end() <= body_start
                        && !matches!(
                            token.kind(),
                            TokenKind::Comment
                                | TokenKind::Newline
                                | TokenKind::NonLogicalNewline
                                | TokenKind::Indent
                                | TokenKind::Dedent
                        )
                })
                .map_or_else(
                    || statement_range.start().to_usize(),
                    |token| token.range().end().to_usize(),
                );
            file.line_index.line(header_end_offset)
        },
    );
    Some(start_line..=end_line)
}

/// Returns the byte offset where the body of a Python compound `statement` begins, or `None`
/// for simple statements without a suite.
fn python_statement_body_start(
    statement: &ruff_python_ast::Stmt,
) -> Option<ruff_text_size::TextSize> {
    use ruff_python_ast::Stmt;
    use ruff_text_size::Ranged as _;

    match statement {
        Stmt::FunctionDef(node) => node.body.first().map(|first| first.range().start()),
        Stmt::ClassDef(node) => node.body.first().map(|first| first.range().start()),
        Stmt::For(node) => node.body.first().map(|first| first.range().start()),
        Stmt::While(node) => node.body.first().map(|first| first.range().start()),
        Stmt::If(node) => node.body.first().map(|first| first.range().start()),
        Stmt::With(node) => node.body.first().map(|first| first.range().start()),
        Stmt::Try(node) => node.body.first().map(|first| first.range().start()),
        Stmt::Match(node) => node.cases.first().map(|first| first.range().start()),
        _ => None,
    }
}

/// Resolves the innermost Rust statement node enclosing `span`.
fn find_rust_enclosing_statement(
    file: &ParsedFile,
    span: SourceSpan,
) -> Option<ra_ap_syntax::SyntaxNode> {
    use ra_ap_syntax::AstNode as _;

    let parsed = file.rs_parsed()?;
    let start_offset = u32::try_from(span.start).ok()?;
    let end_offset = u32::try_from(span.end).ok()?;
    let target_range = ra_ap_syntax::TextRange::new(start_offset.into(), end_offset.into());
    let covering = parsed.tree().syntax().covering_element(target_range);
    let start_node = match covering {
        ra_ap_syntax::SyntaxElement::Node(node) => node,
        ra_ap_syntax::SyntaxElement::Token(token) => token.parent()?,
    };

    start_node.ancestors().find(|candidate| {
        candidate
            .parent()
            .is_some_and(|parent| is_rust_statement_container(parent.kind()))
    })
}

/// Returns true for Rust `SyntaxKind`s that hold statements or items as direct children.
const fn is_rust_statement_container(kind: ra_ap_syntax::SyntaxKind) -> bool {
    use ra_ap_syntax::SyntaxKind;
    matches!(
        kind,
        SyntaxKind::SOURCE_FILE
            | SyntaxKind::STMT_LIST
            | SyntaxKind::ASSOC_ITEM_LIST
            | SyntaxKind::ITEM_LIST
            | SyntaxKind::EXTERN_ITEM_LIST
    )
}

/// Computes the 1-indexed inclusive header line range of a Rust `statement` node.
///
/// Leading comments and whitespace attached inside `statement` (such as `///` doc comments)
/// are skipped when determining the header start line so that `#[...]` outer attributes start
/// the header and preceding doc comments are recognized as adjacent explanations above it.
fn rust_header_line_range(
    file: &ParsedFile,
    statement: &ra_ap_syntax::SyntaxNode,
) -> RangeInclusive<usize> {
    let fallback_start = usize::from(statement.text_range().start());
    let start_offset = statement
        .descendants_with_tokens()
        .filter_map(ra_ap_syntax::SyntaxElement::into_token)
        .find(|token| !is_rust_trivia(token.kind()))
        .map_or(fallback_start, |token| {
            usize::from(token.text_range().start())
        });
    let start_line = file.line_index.line(start_offset);
    let mut header_end_line = start_line;

    for child in statement.children_with_tokens() {
        if is_rust_body_container(child.kind()) {
            return start_line..=header_end_line;
        }
        if !is_rust_trivia(child.kind()) {
            header_end_line = file.line_index.line(usize::from(child.text_range().end()));
        }
    }

    start_line..=header_end_line
}

/// Returns true for Rust `SyntaxKind`s that form the braced body of a compound statement or item.
const fn is_rust_body_container(kind: ra_ap_syntax::SyntaxKind) -> bool {
    use ra_ap_syntax::SyntaxKind;
    matches!(
        kind,
        SyntaxKind::BLOCK_EXPR
            | SyntaxKind::STMT_LIST
            | SyntaxKind::ASSOC_ITEM_LIST
            | SyntaxKind::ITEM_LIST
            | SyntaxKind::EXTERN_ITEM_LIST
    )
}

/// Returns true if `kind` is a Rust comment (`//`, `/* */`, `///`, `//!`, `/** */`, `/*! */`)
/// or whitespace token.
const fn is_rust_trivia(kind: ra_ap_syntax::SyntaxKind) -> bool {
    matches!(kind, ra_ap_syntax::SyntaxKind::WHITESPACE)
        || crate::code_lint::ast::is_rust_comment_kind(kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use rstest::rstest;

    /// Finds the byte span of the first occurrence of `target` in `source`.
    fn span_of(source: &str, target: &str) -> SourceSpan {
        let start = source
            .find(target)
            .expect("target snippet should exist in source");
        SourceSpan::new(start, start + target.len())
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
        Language::Python, "suppress(FileNotFoundError)", 1..=4
    )]
    #[case::python_if(
        indoc! {r"
            if (
                ready()
            ):
                pass
        "},
        Language::Python, "ready()", 1..=3
    )]
    #[case::python_for(
        indoc! {r"
            for item in fetch():
                pass
        "},
        Language::Python, "fetch()", 1..=1
    )]
    #[case::python_try(
        indoc! {r"
            try:
                run()
            except ValueError:
                pass
        "},
        Language::Python, "run()", 2..=2
    )]
    // Simple statements have no body, so the whole statement is header.
    #[case::python_multiline_assignment(
        indoc! {r#"
            CONFIG = (
                get_config("FALLBACK")
            )
        "#},
        Language::Python, "get_config(\"FALLBACK\")", 1..=3
    )]
    #[case::python_annotated_assignment(
        indoc! {r"
            CONFIG: dict[str, str] = build()
        "},
        Language::Python, "build()", 1..=1
    )]
    // A decorated definition's header runs from its first decorator to the end of the
    // definition's own header.
    #[case::python_decorated_function(
        indoc! {r"
            @staticmethod
            def build(
                items,
            ) -> list[str]:
                return []
        "},
        Language::Python, "list[str]", 1..=4
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
        Language::Rust, "{\n        compute()\n    }", 2..=2
    )]
    #[case::rust_simple_let(
        indoc! {r"
            fn f() {
                let x = compute();
            }
        "},
        Language::Rust, "compute()", 2..=2
    )]
    #[case::rust_const_item(
        indoc! {r"
            const LIMIT: usize = compute();
        "},
        Language::Rust, "compute()", 1..=1
    )]
    #[case::rust_attributed_function(
        indoc! {r"
            /// Doc comment line 1.
            ///
            /// Doc comment line 3.
            #[must_use]
            pub fn compute_items(input: &str) -> usize {
                input.len()
            }
        "},
        Language::Rust, "usize", 4..=5
    )]
    fn test_statement_and_header_resolution(
        #[case] source: &str,
        #[case] lang: Language,
        #[case] target: &str,
        #[case] expected_header: RangeInclusive<usize>,
    ) {
        let file = ParsedFile::new(source, lang);
        let span = span_of(source, target);
        assert_eq!(
            enclosing_statement_header_range(&file, span),
            Some(expected_header)
        );
    }

    #[rstest]
    #[case::python(
        indoc! {r"
            with suppress(Exception):
                cleanup()
        "},
        Language::Python, "cleanup()", 2..=2
    )]
    #[case::rust(
        indoc! {r"
            fn f() {
                let x = {
                    compute()
                };
            }
        "},
        Language::Rust, "compute()", 3..=3
    )]
    fn test_node_in_body_resolves_to_inner_statement_not_outer(
        #[case] source: &str,
        #[case] lang: Language,
        #[case] target: &str,
        #[case] expected_header: RangeInclusive<usize>,
    ) {
        let file = ParsedFile::new(source, lang);
        let span = span_of(source, target);
        assert_eq!(
            enclosing_statement_header_range(&file, span),
            Some(expected_header)
        );
    }

    #[test]
    fn test_comment_between_header_and_body_does_not_extend_header() {
        let source = indoc! {r"
            with suppress(Exception):
                # Explanation that belongs to the body, not the header
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let span = span_of(source, "suppress(Exception)");
        assert_eq!(enclosing_statement_header_range(&file, span), Some(1..=1));
    }
}
