//! Navigating the Python syntax tree: finding nodes by span and direct or nested definitions.

use crate::code_lint::ast::tree::{AstNode, ParsedFile, span_from_ruff_range};
use crate::diagnostic::SourceSpan;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, Stmt, StmtFunctionDef};
use ruff_text_size::Ranged as _;

/// Finds all nested Python function definitions (`def ...` inside another `def ...`) and returns
/// `(function_node, function_name)` pairs.
#[must_use]
pub(in crate::code_lint) fn find_nested_functions(file: &ParsedFile) -> Vec<(AstNode<'_>, String)> {
    find_nested_definitions(file, NestedDefinitionKind::Function)
}

/// Finds all Python class definitions inside a function or method body (`class ...` inside `def ...`)
/// and returns `(class_node, class_name)` pairs.
#[must_use]
pub(in crate::code_lint) fn find_nested_classes(file: &ParsedFile) -> Vec<(AstNode<'_>, String)> {
    find_nested_definitions(file, NestedDefinitionKind::Class)
}

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum NestedDefinitionKind {
    Function,
    Class,
}

fn find_nested_definitions(
    file: &ParsedFile,
    target_kind: NestedDefinitionKind,
) -> Vec<(AstNode<'_>, String)> {
    struct NestedDefinitionVisitor<'a> {
        file: &'a ParsedFile,
        target_kind: NestedDefinitionKind,
        in_function: bool,
        out: Vec<(AstNode<'a>, String)>,
    }

    impl<'a> SourceOrderVisitor<'a> for NestedDefinitionVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            match statement {
                Stmt::FunctionDef(function_def) => {
                    if self.in_function && self.target_kind == NestedDefinitionKind::Function {
                        self.out.push((
                            AstNode::from_span(self.file, span_from_ruff_range(function_def.range)),
                            function_def.name.id.to_string(),
                        ));
                    }
                    let previous = self.in_function;
                    self.in_function = true;
                    walk_stmt(self, statement);
                    self.in_function = previous;
                }
                Stmt::ClassDef(class_def) => {
                    if self.in_function && self.target_kind == NestedDefinitionKind::Class {
                        self.out.push((
                            AstNode::from_span(self.file, span_from_ruff_range(class_def.range)),
                            class_def.name.id.to_string(),
                        ));
                    }
                    let previous = self.in_function;
                    self.in_function = false;
                    walk_stmt(self, statement);
                    self.in_function = previous;
                }
                _ => walk_stmt(self, statement),
            }
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = NestedDefinitionVisitor {
        file,
        target_kind,
        in_function: false,
        out: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.out
}
