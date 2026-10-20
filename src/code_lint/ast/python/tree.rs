//! Navigating the Python syntax tree: finding nodes by span and direct child definitions.

use crate::code_lint::ast::tree::{AstNode, span_from_ruff_range};
use crate::diagnostic::SourceSpan;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, Stmt, StmtFunctionDef};
use ruff_text_size::Ranged as _;

/// Finds the `Expr` node in `node.file` whose byte span equals `node.span()`.
pub(super) fn find_expr<'a>(node: &AstNode<'a>) -> Option<&'a Expr> {
    struct ExprFinder<'a> {
        target_span: SourceSpan,
        found: Option<&'a Expr>,
    }

    impl<'a> SourceOrderVisitor<'a> for ExprFinder<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if self.found.is_none()
                && span_from_ruff_range(statement.range()).contains(self.target_span)
            {
                walk_stmt(self, statement);
            }
        }

        fn visit_expr(&mut self, expr: &'a Expr) {
            if self.found.is_some() {
                return;
            }
            let span = span_from_ruff_range(expr.range());
            if !span.contains(self.target_span) {
                return;
            }
            if span == self.target_span {
                self.found = Some(expr);
                return;
            }
            walk_expr(self, expr);
        }
    }

    let parsed = node.file.py_module()?;
    let mut finder = ExprFinder {
        target_span: node.span(),
        found: None,
    };
    finder.visit_body(&parsed.syntax().body);
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
