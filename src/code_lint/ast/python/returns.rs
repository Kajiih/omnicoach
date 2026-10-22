//! What a Python function returns: its annotated return type and, when unannotated, what its own
//! body returns or yields.
//!
//! Design: `docs/dev/return_contracts/03_design.md` (D1, D2).

use super::annotations::{PythonReturnTypeUnion, positional_tuple_return_arity, return_type_union};
use super::classes::is_interface_method;
use super::effects::in_place_mutated_receiver_expr;
use super::functions::{PythonFunctionSignature, extract_function_signatures};
use super::names::written_name;
use crate::code_lint::ast::returns::{FunctionReturn, ReturnShape};
use crate::code_lint::ast::tree::ParsedFile;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, Stmt};
use ruff_text_size::Ranged as _;
use std::collections::{HashMap, HashSet};

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
    pub(in crate::code_lint) fn return_type_union(&self) -> Option<PythonReturnTypeUnion<'a>> {
        self.return_type_node.as_ref().map(return_type_union)
    }

    /// What the function's own body returns or yields.
    #[must_use]
    pub(in crate::code_lint) fn body_results(&self) -> PythonBodyResults {
        collect_body_results(&self.definition.body)
    }

    /// What the function returns: read from its return annotation, or, when unannotated, from
    /// what its own body returns or yields.
    #[must_use]
    pub(in crate::code_lint) fn return_shape(&self) -> ReturnShape {
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
        if self.has_stub_body() {
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
/// ([`PythonFunctionSignature::has_imposed_signature`]) or that are methods of an interface class
/// ([`is_interface_method`]) are skipped.
pub fn collect_function_returns(file: &ParsedFile) -> Vec<FunctionReturn<'_>> {
    extract_function_signatures(file)
        .into_iter()
        .filter(|signature| !signature.has_imposed_signature() && !is_interface_method(signature))
        .map(|signature| FunctionReturn {
            shape: signature.return_shape(),
            name: signature.name,
            name_node: signature.name_node,
            declared_type: signature.return_type_node,
        })
        .collect()
}

/// Inspects `body` (excluding nested functions, classes, and lambdas) for returned or yielded values.
#[must_use]
pub(super) fn collect_body_results(body: &[Stmt]) -> PythonBodyResults {
    let mut collector = BodyResultCollector {
        results: PythonBodyResults::default(),
    };
    collector.visit_body(body);
    collector.results
}

/// Collects function or method names whose return values are mutated in place in `file`.
///
/// Matches direct call mutations (`fn().append(...)`, `fn()[0] = 1`) and local bindings
/// (`buf = fn(); buf.append(...)`, `(buf := fn())`). A binding only counts in the function
/// (or module) scope that assigns it. Callees are matched by name only, so a mutated
/// `obj.get()` result also exempts an unrelated function named `get`.
#[must_use]
pub fn collect_locally_mutated_return_functions(file: &ParsedFile) -> &HashSet<String> {
    file.locally_mutated_return_functions.get_or_init(|| {
        // A binding is keyed by its enclosing function (`None` at module level) and its name.
        type ScopedName = (Option<usize>, String);

        struct MutatedReturnVisitor<'a> {
            source: &'a str,
            enclosing_function: Option<usize>,
            mutated_functions: HashSet<String>,
            bindings_to_callee: HashMap<ScopedName, String>,
            mutated_identifiers: HashSet<ScopedName>,
        }

        impl MutatedReturnVisitor<'_> {
            fn record_binding(&mut self, target: &Expr, value: &Expr) {
                if let Expr::Name(name) = target
                    && let Some(callee_name) = called_terminal_name_expr(value, self.source)
                {
                    let scoped_name = (self.enclosing_function, name.id.to_string());
                    self.bindings_to_callee.insert(scoped_name, callee_name);
                }
            }

            fn record_mutated_receiver(&mut self, receiver: &Expr) {
                if let Some(callee_name) = called_terminal_name_expr(receiver, self.source) {
                    self.mutated_functions.insert(callee_name);
                } else if let Expr::Name(name) = receiver {
                    self.mutated_identifiers
                        .insert((self.enclosing_function, name.id.to_string()));
                }
            }
        }

        impl<'a> SourceOrderVisitor<'a> for MutatedReturnVisitor<'a> {
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
                        let prev_func = self.enclosing_function;
                        self.enclosing_function = Some(func.range().start().to_usize());
                        self.visit_body(&func.body);
                        self.enclosing_function = prev_func;
                    }
                    Stmt::Assign(assign) => {
                        if let [target] = assign.targets.as_slice() {
                            self.record_binding(target, &assign.value);
                        }
                        walk_stmt(self, statement);
                    }
                    Stmt::AnnAssign(ann) => {
                        if let Some(value) = &ann.value {
                            self.record_binding(&ann.target, value);
                        }
                        walk_stmt(self, statement);
                    }
                    Stmt::AugAssign(aug) => {
                        self.record_mutated_receiver(&aug.target);
                        walk_stmt(self, statement);
                    }
                    _ => walk_stmt(self, statement),
                }
            }

            fn visit_expr(&mut self, expr: &'a Expr) {
                if let Expr::Named(named) = expr {
                    self.record_binding(&named.target, &named.value);
                }
                if let Some(receiver) = in_place_mutated_receiver_expr(expr) {
                    self.record_mutated_receiver(receiver);
                }
                walk_expr(self, expr);
            }
        }

        let Some(parsed) = file.py_module() else {
            return HashSet::new();
        };
        let mut visitor = MutatedReturnVisitor {
            source: &file.source,
            enclosing_function: None,
            mutated_functions: HashSet::new(),
            bindings_to_callee: HashMap::new(),
            mutated_identifiers: HashSet::new(),
        };
        visitor.visit_body(&parsed.syntax().body);

        for scoped_name in &visitor.mutated_identifiers {
            if let Some(callee_name) = visitor.bindings_to_callee.get(scoped_name) {
                visitor.mutated_functions.insert(callee_name.clone());
            }
        }

        visitor.mutated_functions
    })
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

/// Extracts the terminal function/method name if `expr` is a `call` expression.
fn called_terminal_name_expr(expr: &Expr, source: &str) -> Option<String> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let terminal = written_name(&call.func, source).terminal;
    (!terminal.is_empty()).then_some(terminal)
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

    #[rstest]
    #[case::direct_call_mutation("make().append(1)", &["make"])]
    #[case::assigned_then_mutated("def use():\n    items = make()\n    items.append(1)", &["make"])]
    #[case::walrus_then_mutated("def use():\n    if (items := make()):\n        items.append(1)", &["make"])]
    #[case::subscript_write("def use():\n    items = make()\n    items[0] = 1", &["make"])]
    #[case::read_only_use("def use():\n    items = make()\n    return len(items)", &[])]
    #[case::binding_in_other_function("def a():\n    items = make()\ndef b():\n    items.append(1)", &[])]
    fn test_collect_locally_mutated_return_functions(
        #[case] usage: &str,
        #[case] expected: &[&str],
    ) {
        let source = format!("def make():\n    return []\n{usage}");
        let file = ParsedFile::new(&source, Language::Python);
        let expected: HashSet<String> = expected.iter().map(|name| (*name).to_string()).collect();
        assert_eq!(*collect_locally_mutated_return_functions(&file), expected);
    }
}
