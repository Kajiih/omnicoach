//! Multiline string literal analysis for Python (`ruff_python_ast`).

use super::names::written_name;
use crate::code_lint::ast::multiline_strings::{
    MultilineKind, MultilineString, escaped_multiline_newlines,
};
use crate::code_lint::ast::tree::{AstNode, ParsedFile, span_from_ruff_range};
use crate::diagnostic::SourceSpan;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, Stmt, StringFlags as _};
use ruff_text_size::Ranged as _;

/// Collects all multiline string literals in a Python `file` (see
/// [`crate::code_lint::ast::collect_multiline_strings`]).
#[must_use]
pub fn collect_multiline_strings(file: &ParsedFile) -> Vec<MultilineString<'_>> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut collector = MultilineStringCollector {
        file,
        docstring_expr_span: None,
        enclosing_calls: Vec::new(),
        out: Vec::new(),
    };
    collector.visit_body(&parsed.syntax().body);
    collector.out
}

struct MultilineStringCollector<'a> {
    file: &'a ParsedFile,
    docstring_expr_span: Option<SourceSpan>,
    enclosing_calls: Vec<String>,
    out: Vec<MultilineString<'a>>,
}

impl MultilineStringCollector<'_> {
    fn record_part(&mut self, span: SourceSpan, is_triple_quoted: bool, decoded: &[u8]) {
        if self.docstring_expr_span == Some(span) {
            return;
        }
        if is_triple_quoted && self.file.source[span.start..span.end].contains('\n') {
            self.out.push(MultilineString {
                node: AstNode::from_span(self.file, span),
                kind: MultilineKind::Source {
                    enclosing_calls: self.enclosing_calls.clone(),
                },
            });
        } else if let Some(newlines) = escaped_multiline_newlines(decoded) {
            self.out.push(MultilineString {
                node: AstNode::from_span(self.file, span),
                kind: MultilineKind::Escaped { newlines },
            });
        }
    }
}

impl<'a> SourceOrderVisitor<'a> for MultilineStringCollector<'a> {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        if let Stmt::Expr(expr_statement) = statement {
            let prev_doc = self.docstring_expr_span;
            self.docstring_expr_span = Some(span_from_ruff_range(expr_statement.range()));
            walk_stmt(self, statement);
            self.docstring_expr_span = prev_doc;
        } else {
            walk_stmt(self, statement);
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Lambda(_) => {
                let saved_calls = std::mem::take(&mut self.enclosing_calls);
                walk_expr(self, expr);
                self.enclosing_calls = saved_calls;
            }
            Expr::Call(call) => {
                let resolved = written_name(&call.func, &self.file.source).resolve(self.file);
                if let Some(name) = &resolved {
                    self.enclosing_calls.push(name.path.clone());
                }
                walk_expr(self, expr);
                if resolved.is_some() {
                    self.enclosing_calls.pop();
                }
            }
            Expr::StringLiteral(str_lit) => {
                for part in str_lit.value.as_slice() {
                    self.record_part(
                        span_from_ruff_range(part.range()),
                        part.flags.is_triple_quoted(),
                        part.as_str().as_bytes(),
                    );
                }
            }
            Expr::FString(fstr) => {
                for part in &fstr.value {
                    match part {
                        ruff_python_ast::FStringPartRef::Literal(lit) => {
                            self.record_part(
                                span_from_ruff_range(lit.range()),
                                lit.flags.is_triple_quoted(),
                                lit.as_str().as_bytes(),
                            );
                        }
                        ruff_python_ast::FStringPartRef::FString(fpart) => {
                            let decoded: String = fpart
                                .elements
                                .iter()
                                .map(|elt| elt.as_literal().map_or("_", |lit| lit.value.as_ref()))
                                .collect();
                            self.record_part(
                                span_from_ruff_range(fpart.range()),
                                fpart.flags.is_triple_quoted(),
                                decoded.as_bytes(),
                            );
                        }
                    }
                }
                walk_expr(self, expr);
            }
            Expr::BytesLiteral(bytes_lit) => {
                for part in bytes_lit.value.as_slice() {
                    self.record_part(
                        span_from_ruff_range(part.range()),
                        part.flags.is_triple_quoted(),
                        part.as_slice(),
                    );
                }
            }
            _ => walk_expr(self, expr),
        }
    }
}
