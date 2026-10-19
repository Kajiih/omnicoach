//! Multiline string literal analysis for Python (`ruff_python_ast`).

use super::names::written_name;
use crate::code_lint::ast::{AstNode, ParsedFile, span_from_ruff_range};
use crate::diagnostic::SourceSpan;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, Stmt, StringFlags as _};
use ruff_text_size::Ranged as _;

/// Finds all multiline string literals in a Python file that are not docstrings
/// and not wrapped in an allowed call.
#[must_use]
pub(in crate::code_lint::ast) fn find_unwrapped_multiline_strings(
    file: &ParsedFile,
    is_allowed_wrapper: impl Fn(&str) -> bool,
) -> Vec<AstNode<'_>> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut finder = UnwrappedMultilineFinder {
        file,
        is_allowed_wrapper,
        docstring_expr_span: None,
        allowed_call_depth: 0,
        out: Vec::new(),
    };
    finder.visit_body(&parsed.syntax().body);
    finder.out
}

struct UnwrappedMultilineFinder<'a, F> {
    file: &'a ParsedFile,
    is_allowed_wrapper: F,
    docstring_expr_span: Option<SourceSpan>,
    allowed_call_depth: usize,
    out: Vec<AstNode<'a>>,
}

impl<F> UnwrappedMultilineFinder<'_, F> {
    fn record_multiline_part(&mut self, span: SourceSpan, is_triple_quoted: bool) {
        let is_docstring = self.docstring_expr_span == Some(span);
        if is_triple_quoted
            && self.file.source[span.start..span.end].contains('\n')
            && !is_docstring
            && self.allowed_call_depth == 0
        {
            self.out.push(AstNode::from_span(self.file, span));
        }
    }
}

impl<'a, F: Fn(&str) -> bool> SourceOrderVisitor<'a> for UnwrappedMultilineFinder<'a, F> {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        match statement {
            Stmt::FunctionDef(func) => {
                for dec in &func.decorator_list {
                    self.visit_decorator(dec);
                }
                self.visit_parameters(&func.parameters);
                if let Some(returns) = &func.returns {
                    self.visit_annotation(returns);
                }
                let prev_depth = self.allowed_call_depth;
                self.allowed_call_depth = 0;
                self.visit_body(&func.body);
                self.allowed_call_depth = prev_depth;
            }
            Stmt::ClassDef(cls) => {
                for dec in &cls.decorator_list {
                    self.visit_decorator(dec);
                }
                if let Some(args) = &cls.arguments {
                    self.visit_arguments(args);
                }
                let prev_depth = self.allowed_call_depth;
                self.allowed_call_depth = 0;
                self.visit_body(&cls.body);
                self.allowed_call_depth = prev_depth;
            }
            Stmt::Expr(expr_statement) => {
                let prev_doc = self.docstring_expr_span;
                self.docstring_expr_span = Some(span_from_ruff_range(expr_statement.range()));
                walk_stmt(self, statement);
                self.docstring_expr_span = prev_doc;
            }
            _ => walk_stmt(self, statement),
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Lambda(lambda) => {
                if let Some(params) = &lambda.parameters {
                    self.visit_parameters(params);
                }
                let prev_depth = self.allowed_call_depth;
                self.allowed_call_depth = 0;
                self.visit_expr(&lambda.body);
                self.allowed_call_depth = prev_depth;
            }
            Expr::Call(call) => {
                let path = written_name(&call.func, &self.file.source).path;
                let is_allowed = (self.is_allowed_wrapper)(&path);
                if is_allowed {
                    self.allowed_call_depth += 1;
                }
                walk_expr(self, expr);
                if is_allowed {
                    self.allowed_call_depth -= 1;
                }
            }
            Expr::StringLiteral(str_lit) => {
                for part in str_lit.value.as_slice() {
                    self.record_multiline_part(
                        span_from_ruff_range(part.range()),
                        part.flags.is_triple_quoted(),
                    );
                }
            }
            Expr::FString(fstr) => {
                for part in &fstr.value {
                    match part {
                        ruff_python_ast::FStringPartRef::Literal(lit) => {
                            self.record_multiline_part(
                                span_from_ruff_range(lit.range()),
                                lit.flags.is_triple_quoted(),
                            );
                        }
                        ruff_python_ast::FStringPartRef::FString(fpart) => {
                            self.record_multiline_part(
                                span_from_ruff_range(fpart.range()),
                                fpart.flags.is_triple_quoted(),
                            );
                        }
                    }
                }
                walk_expr(self, expr);
            }
            Expr::BytesLiteral(bytes_lit) => {
                for part in bytes_lit.value.as_slice() {
                    self.record_multiline_part(
                        span_from_ruff_range(part.range()),
                        part.flags.is_triple_quoted(),
                    );
                }
            }
            _ => walk_expr(self, expr),
        }
    }
}
