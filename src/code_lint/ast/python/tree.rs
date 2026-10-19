//! Navigating the Python syntax tree: finding nodes by span and direct child definitions.

use crate::code_lint::ast::span_from_ruff_range;
use crate::diagnostic::SourceSpan;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, ModModule, Stmt, StmtFunctionDef};
use ruff_text_size::Ranged as _;

/// Finds the `Expr` node in `module` whose byte span equals `target_span`.
pub(super) fn find_expr_at_span(module: &ModModule, target_span: SourceSpan) -> Option<&Expr> {
    struct ExprFinder<'a> {
        target_span: SourceSpan,
        found: Option<&'a Expr>,
    }

    impl<'a> SourceOrderVisitor<'a> for ExprFinder<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            let span = span_from_ruff_range(statement.range());
            if self.found.is_none()
                && span.start <= self.target_span.start
                && self.target_span.end <= span.end
            {
                walk_stmt(self, statement);
            }
        }

        fn visit_expr(&mut self, expr: &'a Expr) {
            if self.found.is_some() {
                return;
            }
            let span = span_from_ruff_range(expr.range());
            if !(span.start <= self.target_span.start && self.target_span.end <= span.end) {
                return;
            }
            if span == self.target_span {
                self.found = Some(expr);
                return;
            }
            walk_expr(self, expr);
        }
    }

    let mut finder = ExprFinder {
        target_span,
        found: None,
    };
    finder.visit_body(&module.body);
    finder.found
}

/// Returns each direct `StmtFunctionDef` in `scope_body` (`module` or class `body`).
pub(super) fn direct_function_definitions(scope_body: &[Stmt]) -> Vec<&StmtFunctionDef> {
    scope_body
        .iter()
        .filter_map(|statement| match statement {
            Stmt::FunctionDef(func_def) => Some(func_def),
            _ => None,
        })
        .collect()
}
