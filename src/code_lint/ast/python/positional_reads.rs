//! Python positional reads (`xs[0]`, `argv[-1]`) grouped by scope, with the receivers that the
//! scope also iterates, sizes or mutates.

use super::MUTATING_METHODS;
use super::parameter_usage::is_collection_builtin;
use crate::code_lint::ast::{
    AstNode, ParsedFile, PositionalRead, ScopePositionalReads, span_from_ruff_range,
};
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, Stmt};
use ruff_text_size::Ranged as _;

/// Returns the value of a Python decimal `integer` literal (not `0x1`, `1_000`, ...).
fn decimal_literal_expr(expr: &Expr, source: &str) -> Option<i64> {
    let Expr::NumberLiteral(number) = expr else {
        return None;
    };
    if !matches!(number.value, ruff_python_ast::Number::Int(_)) {
        return None;
    }
    let text = &source[number.range().start().to_usize()..number.range().end().to_usize()];
    if !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Returns the position read by a Python `subscript` index: a decimal literal, or its negation
/// for end-relative reads (`xs[-1]`).
fn literal_position_expr(index: &Expr, source: &str) -> Option<i64> {
    if let Expr::UnaryOp(unary) = index
        && unary.op == ruff_python_ast::UnaryOp::USub
    {
        return decimal_literal_expr(&unary.operand, source).map(std::ops::Neg::neg);
    }
    decimal_literal_expr(index, source)
}

/// Returns true if `expr` contains any call expression (`Expr::Call`).
fn expr_contains_call(expr: &Expr) -> bool {
    struct CallDetector {
        found: bool,
    }

    impl<'a> SourceOrderVisitor<'a> for CallDetector {
        fn visit_expr(&mut self, expr: &'a Expr) {
            if self.found {
                return;
            }
            if matches!(expr, Expr::Call(_)) {
                self.found = true;
                return;
            }
            walk_expr(self, expr);
        }
    }

    let mut detector = CallDetector { found: false };
    detector.visit_expr(expr);
    detector.found
}

/// Returns true if `receiver` names a stable value: a name, attribute or subscript chain with no
/// call inside, so that identical text means the same value.
fn is_stable_receiver_expr(receiver: &Expr) -> bool {
    matches!(
        receiver,
        Expr::Name(_) | Expr::Attribute(_) | Expr::Subscript(_)
    ) && !expr_contains_call(receiver)
}

struct PositionalReadsCollector<'a> {
    file: &'a ParsedFile,
    current_scope: Option<ScopePositionalReads<'a>>,
    out: Vec<ScopePositionalReads<'a>>,
}

impl PositionalReadsCollector<'_> {
    fn expr_text(&self, expr: &Expr) -> String {
        let range = expr.range();
        self.file.source[range.start().to_usize()..range.end().to_usize()].to_string()
    }

    fn record_exempt_receiver(&mut self, expr: &Expr) {
        let text = self.expr_text(expr);
        if let Some(scope) = &mut self.current_scope {
            scope.exempt_receivers.insert(text);
        }
    }

    fn record_subscript(&mut self, subscript: &ruff_python_ast::ExprSubscript) {
        let receiver_text = self.expr_text(&subscript.value);
        let is_load = matches!(subscript.ctx, ruff_python_ast::ExprContext::Load);
        let position = literal_position_expr(&subscript.slice, &self.file.source);
        let is_stable = is_stable_receiver_expr(&subscript.value);
        let Some(scope) = &mut self.current_scope else {
            return;
        };
        if let Some(position) = position
            && is_load
        {
            if is_stable {
                scope.reads.push(PositionalRead {
                    node: AstNode::from_span(self.file, span_from_ruff_range(subscript.range())),
                    receiver: receiver_text,
                    position,
                });
            }
        } else {
            scope.exempt_receivers.insert(receiver_text);
        }
    }

    fn record_call_collections(&mut self, call: &ruff_python_ast::ExprCall) {
        match call.func.as_ref() {
            Expr::Name(func_name) if is_collection_builtin(func_name.id.as_str()) => {
                for arg in &call.arguments.args {
                    self.record_exempt_receiver(arg);
                }
            }
            Expr::Attribute(attr) if MUTATING_METHODS.contains(&attr.attr.as_str()) => {
                self.record_exempt_receiver(&attr.value);
            }
            _ => {}
        }
    }
}

impl<'a> SourceOrderVisitor<'a> for PositionalReadsCollector<'a> {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        match statement {
            Stmt::FunctionDef(func) => {
                // Default values and annotations are evaluated in the enclosing scope.
                for dec in &func.decorator_list {
                    self.visit_decorator(dec);
                }
                self.visit_parameters(&func.parameters);
                if let Some(returns) = &func.returns {
                    self.visit_annotation(returns);
                }
                let prev_scope = self.current_scope.replace(ScopePositionalReads::default());
                self.visit_body(&func.body);
                if let Some(finished_scope) = std::mem::replace(&mut self.current_scope, prev_scope)
                {
                    self.out.push(finished_scope);
                }
            }
            Stmt::ClassDef(_) => {
                let prev_scope = self.current_scope.take();
                walk_stmt(self, statement);
                self.current_scope = prev_scope;
            }
            Stmt::For(for_statement) => {
                self.record_exempt_receiver(&for_statement.iter);
                walk_stmt(self, statement);
            }
            _ => walk_stmt(self, statement),
        }
    }

    fn visit_comprehension(&mut self, comp: &'a ruff_python_ast::Comprehension) {
        self.record_exempt_receiver(&comp.iter);
        ruff_python_ast::visitor::source_order::walk_comprehension(self, comp);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Lambda(_) => return,
            Expr::Subscript(sub) => {
                self.record_subscript(sub);
            }
            Expr::Call(call) => {
                self.record_call_collections(call);
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

/// Collects Python positional reads grouped by scope (see [`crate::code_lint::ast::collect_positional_reads`]).
#[must_use]
pub(in crate::code_lint::ast) fn collect_positional_reads(
    file: &ParsedFile,
) -> Vec<ScopePositionalReads<'_>> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut collector = PositionalReadsCollector {
        file,
        current_scope: Some(ScopePositionalReads::default()),
        out: Vec::new(),
    };
    collector.visit_body(&parsed.syntax().body);
    if let Some(module_scope) = collector.current_scope {
        collector.out.push(module_scope);
    }
    collector.out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;

    /// Module-level reads form their own scope. `rule_test!` cannot cover this: repeating a
    /// fail case in one file merges both copies into the same module scope.
    #[test]
    fn test_collect_positional_reads_module_scope() {
        let source = indoc::indoc! {r"
            src, dst = sys.argv[1], sys.argv[2]

            def main(argv):
                return argv[1]
        "};
        let file = ParsedFile::new(source, Language::Python);
        let reads_by_scope: Vec<Vec<(String, i64)>> = collect_positional_reads(&file)
            .into_iter()
            .map(|scope| {
                scope
                    .reads
                    .into_iter()
                    .map(|read| (read.receiver, read.position))
                    .collect()
            })
            .collect();

        let module_reads = vec![("sys.argv".to_string(), 1), ("sys.argv".to_string(), 2)];
        assert!(
            reads_by_scope.contains(&module_reads),
            "no module scope holding exactly the top-level reads: {reads_by_scope:?}"
        );
    }
}
