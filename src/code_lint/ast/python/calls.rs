//! Python call-expression candidate extraction (`ruff_python_ast`).

use super::EnclosingFunctionTracker;
use crate::code_lint::ast::{
    CachedCallCandidate, CallSiteContext, ParsedFile, span_from_ruff_range,
};
use crate::diagnostic::SourceSpan;
use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, walk_except_handler, walk_expr, walk_stmt, walk_with_item,
};
use ruff_python_ast::{ExceptHandler, Expr, Stmt, WithItem};
use ruff_text_size::Ranged as _;
use std::sync::Arc;

/// Collects all direct call expressions in a Python file using `ruff_python_ast`.
#[must_use]
pub(in crate::code_lint::ast) fn collect_call_candidates(
    file: &ParsedFile,
) -> Vec<CachedCallCandidate> {
    struct CallVisitor<'a> {
        file: &'a ParsedFile,
        /// Span of the context-manager expression of the `with` item being visited.
        with_item_context_span: Option<SourceSpan>,
        /// True while inside an `except` handler of the current function scope.
        in_except_clause: bool,
        /// True while inside the condition or body of a `for` or `while` loop of the current scope.
        in_loop: bool,
        enclosing_functions: EnclosingFunctionTracker,
        out: Vec<CachedCallCandidate>,
    }

    impl<'a> SourceOrderVisitor<'a> for CallVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            let enclosing_except = self.in_except_clause;
            let enclosing_loop = self.in_loop;
            if matches!(statement, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
                self.in_except_clause = false;
            }
            if matches!(statement, Stmt::FunctionDef(_)) {
                self.in_loop = false;
            }
            let outer_functions = self.enclosing_functions.enter(statement);
            match statement {
                Stmt::For(for_stmt) => {
                    self.visit_expr(&for_stmt.target);
                    self.visit_expr(&for_stmt.iter);
                    self.in_loop = true;
                    self.visit_body(&for_stmt.body);
                    self.in_loop = enclosing_loop;
                    self.visit_body(&for_stmt.orelse);
                }
                Stmt::While(while_stmt) => {
                    self.in_loop = true;
                    self.visit_expr(&while_stmt.test);
                    self.visit_body(&while_stmt.body);
                    self.in_loop = enclosing_loop;
                    self.visit_body(&while_stmt.orelse);
                }
                _ => walk_stmt(self, statement),
            }
            self.enclosing_functions.exit(outer_functions);
            self.in_except_clause = enclosing_except;
            self.in_loop = enclosing_loop;
        }

        fn visit_expr(&mut self, expr: &'a Expr) {
            let enclosing_except = self.in_except_clause;
            let enclosing_loop = self.in_loop;
            match expr {
                Expr::Call(call) => {
                    let span = span_from_ruff_range(call.range());
                    let is_with_context_manager = self.with_item_context_span == Some(span);
                    self.out.push(build_python_call_candidate(
                        &self.file.source,
                        call,
                        span,
                        CallSiteContext {
                            is_with_context_manager,
                            is_in_except_clause: self.in_except_clause,
                            is_in_loop: self.in_loop,
                            enclosing_functions: Arc::clone(&self.enclosing_functions.functions),
                        },
                    ));
                }
                Expr::Lambda(_) => {
                    self.in_except_clause = false;
                    self.in_loop = false;
                }
                _ => {}
            }
            walk_expr(self, expr);
            self.in_except_clause = enclosing_except;
            self.in_loop = enclosing_loop;
        }

        fn visit_with_item(&mut self, with_item: &'a WithItem) {
            let enclosing = self.with_item_context_span;
            self.with_item_context_span =
                Some(span_from_ruff_range(with_item.context_expr.range()));
            walk_with_item(self, with_item);
            self.with_item_context_span = enclosing;
        }

        fn visit_except_handler(&mut self, except_handler: &'a ExceptHandler) {
            let enclosing = self.in_except_clause;
            self.in_except_clause = true;
            walk_except_handler(self, except_handler);
            self.in_except_clause = enclosing;
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = CallVisitor {
        file,
        with_item_context_span: None,
        in_except_clause: false,
        in_loop: false,
        enclosing_functions: EnclosingFunctionTracker::default(),
        out: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.out
}

/// Builds a [`CachedCallCandidate`] for a Python call expression.
fn build_python_call_candidate(
    source: &str,
    call: &ruff_python_ast::ExprCall,
    span: SourceSpan,
    context: CallSiteContext,
) -> CachedCallCandidate {
    let func_span = span_from_ruff_range(call.func.range());
    let callee = source[func_span.start..func_span.end].to_string();
    let (method_name, receiver_call_callee) = if let Expr::Attribute(attr) = call.func.as_ref() {
        let receiver_callee = if let Expr::Call(inner_call) = attr.value.as_ref() {
            let inner_span = span_from_ruff_range(inner_call.func.range());
            Some(source[inner_span.start..inner_span.end].to_string())
        } else {
            None
        };
        (Some(attr.attr.to_string()), receiver_callee)
    } else {
        (None, None)
    };
    let mut argument_spans: Vec<SourceSpan> = call
        .arguments
        .args
        .iter()
        .map(|arg| span_from_ruff_range(arg.range()))
        .chain(
            call.arguments
                .keywords
                .iter()
                .map(|kw| span_from_ruff_range(kw.range())),
        )
        .collect();
    argument_spans.sort_by_key(|span| (span.start, span.end));
    CachedCallCandidate {
        span,
        callee,
        method_name,
        receiver_call_callee,
        argument_spans,
        context,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;

    #[test]
    fn test_call_candidate_is_with_context_manager() {
        let source = indoc::indoc! {r"
            with suppress(FileNotFoundError):
                pass
            x = suppress(KeyError)
        "};
        let file = ParsedFile::new(source, Language::Python);
        let calls = collect_call_candidates(&file);
        assert_eq!(calls.len(), 2);
        assert!(calls[0].is_with_context_manager);
        assert!(!calls[1].is_with_context_manager);
    }
}
