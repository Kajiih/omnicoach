//! Header and decorator extents of Python statements.

use crate::code_lint::ast::tree::{ParsedFile, span_from_ruff_range};
use crate::diagnostic::SourceSpan;
use ruff_python_ast::Stmt;
use ruff_python_ast::token::TokenKind;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_stmt};
use ruff_text_size::Ranged as _;

/// Resolves the byte span of the header of the innermost Python statement enclosing `span`.
pub(in crate::code_lint::ast) fn enclosing_statement_header_span(
    file: &ParsedFile,
    span: SourceSpan,
) -> Option<SourceSpan> {
    struct StatementFinder<'a> {
        target_span: SourceSpan,
        found: Option<&'a Stmt>,
    }

    impl<'a> SourceOrderVisitor<'a> for StatementFinder<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if !span_from_ruff_range(statement.range()).contains(self.target_span) {
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
    let start_offset = statement_range.start().to_usize();
    let end_offset = python_statement_body_start(statement).map_or_else(
        || statement_range.end().to_usize(),
        |body_start| {
            parsed
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
                .map_or(start_offset, |token| token.range().end().to_usize())
        },
    );
    Some(SourceSpan::new(start_offset, end_offset))
}

/// Returns the byte offset where the body of a Python compound `statement` begins, or `None`
/// for simple statements without a suite.
fn python_statement_body_start(statement: &Stmt) -> Option<ruff_text_size::TextSize> {
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

/// Records the byte span of every Python decorator in `file`.
pub(in crate::code_lint::ast) fn record_decorator_and_attribute_spans(
    file: &ParsedFile,
    record: &mut impl FnMut(SourceSpan),
) {
    struct DecoratorVisitor<Record> {
        record: Record,
    }

    impl<'a, Record: FnMut(SourceSpan)> SourceOrderVisitor<'a> for DecoratorVisitor<Record> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            let decorators = match statement {
                Stmt::FunctionDef(function) => function.decorator_list.as_slice(),
                Stmt::ClassDef(class) => class.decorator_list.as_slice(),
                _ => &[],
            };
            for decorator in decorators {
                (self.record)(span_from_ruff_range(decorator.range()));
            }
            walk_stmt(self, statement);
        }
    }

    if let Some(parsed) = file.py_module() {
        DecoratorVisitor { record }.visit_body(&parsed.syntax().body);
    }
}
