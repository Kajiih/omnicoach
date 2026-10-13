//! What a Python function returns: its annotated return type and, when unannotated, what its own
//! body returns or yields.
//!
//! Design: `docs/dev/return_contracts/03_design.md` (D1, D2).

use super::annotations::{PythonReturnTypeUnion, positional_tuple_return_arity, return_type_union};
use super::functions::{PythonFunctionSignature, extract_function_signatures};
use crate::code_lint::ast::{FunctionReturn, ParsedFile, ReturnShape};
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, Stmt};

/// What a function's own body returns or yields, nested functions, classes and lambdas excluded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PythonBodyResults {
    /// True if the body has a `return <value>` whose value is not the `None` literal.
    pub returns_value: bool,
    /// True if the body has a `yield` or `yield from`.
    pub yields: bool,
    /// The largest element count among the tuple displays of at least two elements that the body
    /// returns or yields (`return low, high`); displays with a starred element are not counted.
    pub tuple_display_arity: Option<usize>,
}

impl<'a> PythonFunctionSignature<'a> {
    /// The return annotation flattened across unions, if the function has one (see
    /// [`return_type_union`]).
    #[must_use]
    pub fn return_type_union(&self) -> Option<PythonReturnTypeUnion<'a>> {
        self.return_type_node.as_ref().map(return_type_union)
    }
}

impl PythonFunctionSignature<'_> {
    /// What the function's own body returns or yields.
    #[must_use]
    pub fn body_results(&self) -> PythonBodyResults {
        let mut collector = BodyResultCollector {
            results: PythonBodyResults::default(),
        };
        collector.visit_body(&self.definition.body);
        collector.results
    }

    /// What the function returns: read from its return annotation, or, when unannotated, from
    /// what its own body returns or yields.
    #[must_use]
    pub fn return_shape(&self) -> ReturnShape {
        let Some(return_type_node) = &self.return_type_node else {
            return self.unannotated_return_shape();
        };
        if let Some(arity) = positional_tuple_return_arity(return_type_node) {
            return ReturnShape::PositionalTuple { arity };
        }
        let union = return_type_union(return_type_node);
        if union.is_boolean() {
            ReturnShape::Boolean
        } else if union.has_none && union.branches.is_empty() {
            ReturnShape::Unit
        } else {
            ReturnShape::Other
        }
    }

    /// See [`Self::return_shape`]: a stub body says nothing about the result.
    fn unannotated_return_shape(&self) -> ReturnShape {
        if self.has_stub_body {
            return ReturnShape::Unknown;
        }
        let results = self.body_results();
        results.tuple_display_arity.map_or(
            if results.returns_value || results.yields {
                ReturnShape::Unknown
            } else {
                ReturnShape::Unit
            },
            |arity| ReturnShape::PositionalTuple { arity },
        )
    }
}

/// See [`crate::code_lint::ast::collect_function_returns`]: functions whose signature is imposed
/// ([`PythonFunctionSignature::is_exempt_from_signature_rules`]) are skipped.
pub(in crate::code_lint::ast) fn collect_function_returns(
    file: &ParsedFile,
) -> Vec<FunctionReturn<'_>> {
    extract_function_signatures(file)
        .into_iter()
        .filter(|signature| !signature.is_exempt_from_signature_rules())
        .map(|signature| FunctionReturn {
            shape: signature.return_shape(),
            name: signature.name,
            name_node: signature.name_node,
            declared_type: signature.return_type_node,
        })
        .collect()
}

/// Walks a function's own body for [`PythonBodyResults`].
struct BodyResultCollector {
    results: PythonBodyResults,
}

impl BodyResultCollector {
    /// Records `value` as returned or yielded if it is a tuple display of at least two elements.
    fn record_tuple_display(&mut self, value: Option<&Expr>) {
        if let Some(Expr::Tuple(tuple)) = value
            && tuple.elts.len() >= 2
            && !tuple.elts.iter().any(Expr::is_starred_expr)
        {
            let arity = self.results.tuple_display_arity.unwrap_or_default();
            self.results.tuple_display_arity = Some(arity.max(tuple.elts.len()));
        }
    }
}

impl<'a> SourceOrderVisitor<'a> for BodyResultCollector {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        match statement {
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => return,
            Stmt::Return(return_statement) => {
                let value = return_statement.value.as_deref();
                if value.is_some_and(|value| !value.is_none_literal_expr()) {
                    self.results.returns_value = true;
                }
                self.record_tuple_display(value);
            }
            _ => {}
        }
        walk_stmt(self, statement);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Lambda(_) => return,
            Expr::Yield(yield_expr) => {
                self.results.yields = true;
                self.record_tuple_display(yield_expr.value.as_deref());
            }
            Expr::YieldFrom(_) => self.results.yields = true,
            _ => {}
        }
        walk_expr(self, expr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;
    use rstest::rstest;

    fn first_shape(source: &str) -> ReturnShape {
        let file = ParsedFile::new(source, Language::Python);
        extract_function_signatures(&file)[0].return_shape()
    }

    #[rstest]
    #[case::bool("def f() -> bool: return True", ReturnShape::Boolean)]
    #[case::type_guard(
        "from typing import TypeGuard\ndef f(x) -> TypeGuard[int]: return True",
        ReturnShape::Boolean
    )]
    #[case::async_envelope(
        "from typing import Awaitable\ndef f() -> Awaitable[bool]: return g()",
        ReturnShape::Boolean
    )]
    #[case::optional_bool("def f() -> bool | None: return None", ReturnShape::Other)]
    #[case::none_annotation("def f() -> None: pass", ReturnShape::Unit)]
    #[case::tuple("def f() -> tuple[int, str]: return 1, ''", ReturnShape::PositionalTuple { arity: 2 })]
    #[case::optional_tuple(
        "def f() -> tuple[int, str, int] | None: return None",
        ReturnShape::PositionalTuple { arity: 3 }
    )]
    #[case::yielded_tuple(
        "from collections.abc import Iterator\ndef f() -> Iterator[tuple[int, str]]: yield 1, ''",
        ReturnShape::PositionalTuple { arity: 2 }
    )]
    #[case::variadic_tuple("def f() -> tuple[int, ...]: return ()", ReturnShape::Other)]
    #[case::local_bool_class(
        "class bool: pass\ndef f() -> bool: return bool()",
        ReturnShape::Other
    )]
    #[case::unannotated_tuple_display("def f(x):\n    return x, x", ReturnShape::PositionalTuple { arity: 2 })]
    #[case::unannotated_starred_display("def f(x):\n    return *x, x", ReturnShape::Unknown)]
    #[case::unannotated_value("def f(x):\n    return x", ReturnShape::Unknown)]
    #[case::unannotated_generator("def f(x):\n    yield x", ReturnShape::Unknown)]
    #[case::unannotated_none_return(
        "def f(x):\n    if x:\n        return None\n    print(x)",
        ReturnShape::Unit
    )]
    #[case::unannotated_stub("def f(x):\n    ...", ReturnShape::Unknown)]
    #[case::nested_scopes_not_entered(
        "def f(x):\n    def g():\n        return x, x\n    h = lambda: (x, x)\n    print(g, h)",
        ReturnShape::Unit
    )]
    fn return_shape_reads_annotation_then_body(
        #[case] source: &str,
        #[case] expected: ReturnShape,
    ) {
        assert_eq!(first_shape(source), expected);
    }
}
