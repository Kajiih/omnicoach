//! Python parameter-usage analysis: one walk of a function body classifies every use of its
//! parameters (mutation or escape, and the read-only collection capability they need).

use super::PythonFunctionSignature;
use super::scopes::parameters_shadow_name;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, Parameters, Stmt};
use std::collections::HashMap;

/// Read-only methods available on `Sequence`, `Mapping`, or `Set` (`collections.abc`).
const READONLY_COLLECTION_METHODS: &[&str] = &[
    "count",
    "index",
    "get",
    "keys",
    "values",
    "items",
    "isdisjoint",
    "issubset",
    "issuperset",
    "copy",
    "__contains__",
    "__len__",
    "__iter__",
    "__getitem__",
    "__reversed__",
];

const BOOL_CONSTRUCTOR: &str = "bool";
const BUILTIN_LEN: &str = "len";

/// Builtins that iterate their collection argument(s) in a single pass and are also treated as
/// collection-sizing/iterating calls in positional-read analysis.
const ITERATING_COLLECTION_BUILTINS: &[&str] = &["enumerate", "zip", "sorted"];

/// Additional builtins that consume an `Iterable` in a single pass.
const OTHER_SINGLE_PASS_ITERABLE_BUILTINS: &[&str] = &[
    "sum",
    "min",
    "max",
    "any",
    "all",
    "list",
    "tuple",
    "set",
    "frozenset",
    "dict",
    "iter",
    "map",
    "filter",
];

/// Additional builtins that read a collection without mutating it in place or retaining a
/// mutable alias to the outer container.
const OTHER_SAFE_READONLY_BUILTINS: &[&str] = &[
    BOOL_CONSTRUCTOR,
    "repr",
    "str",
    "hash",
    "id",
    "type",
    "isinstance",
    "issubclass",
    "print",
    "range",
    "next",
];

/// Classifies how `signature`'s body uses each of its parameters, keyed by parameter name.
#[must_use]
pub fn summarize_parameter_usages<'a>(
    signature: &PythonFunctionSignature<'a>,
) -> HashMap<&'a str, ParameterUsage> {
    let func = signature.definition;
    let mut visitor = ParameterUseVisitor {
        usages: func
            .parameters
            .iter()
            .map(|parameter| (parameter.name().as_str(), ParameterUsage::default()))
            .collect(),
        shadowed: Vec::new(),
        loop_or_closure_depth: 0,
    };
    visitor.visit_body(&func.body);
    visitor.usages
}

/// Returns true if `name` is a builtin that reads or iterates a collection without mutating it
/// in place or retaining a mutable alias to the outer container.
fn is_safe_readonly_builtin(name: &str) -> bool {
    is_single_pass_iterable_builtin(name)
        || is_collection_builtin(name)
        || OTHER_SAFE_READONLY_BUILTINS.contains(&name)
}

/// Returns true if `name` is a builtin that consumes an `Iterable` in a single pass.
fn is_single_pass_iterable_builtin(name: &str) -> bool {
    ITERATING_COLLECTION_BUILTINS.contains(&name)
        || OTHER_SINGLE_PASS_ITERABLE_BUILTINS.contains(&name)
}

/// Returns true if `name` is a builtin that iterates or sizes its positional arguments
/// (`len(xs)`, `enumerate(xs)`, `zip(xs, ys)`, `reversed(xs)`, `sorted(xs)`).
pub(super) fn is_collection_builtin(name: &str) -> bool {
    matches!(name, BUILTIN_LEN | "reversed") || ITERATING_COLLECTION_BUILTINS.contains(&name)
}

/// How a function body reads a parameter at one use site.
#[derive(Clone, Copy, PartialEq, Eq)]
enum UseRole {
    /// Mutated, rebound, aliased, returned, yielded, or passed to an unknown function or method.
    Escape,
    /// Read without mutation by an operation that needs more than `Collection`: indexing,
    /// `reversed`, `.index()`, equality, arithmetic, `repr`, `print`, ...
    Read,
    /// Truth-tested: `if x`, `not x`, `bool(x)`; propagates to `and`/`or` operands.
    Truthiness,
    /// Sized or membership-tested: `len(x)`, `v in x`.
    Sized,
    /// Iterated once: `for _ in x`, a comprehension, a single-pass builtin, or `*x` in a display.
    Iterated,
    /// Compared by identity: `x is None`.
    Identity,
}

/// How a function body uses one of its parameters; see [`summarize_parameter_usages`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ParameterUsage {
    mutated_or_escaping: bool,
    needs_sequence: bool,
    needs_collection: bool,
    iteration_count: usize,
}

impl ParameterUsage {
    /// Returns true if the parameter is mutated in place or escapes (aliased, returned, yielded,
    /// or passed to an unknown function or method).
    #[must_use]
    pub const fn is_mutated_or_escaping(self) -> bool {
        self.mutated_or_escaping
    }

    /// Returns the minimum read-only collection capability the parameter's uses require.
    #[must_use]
    pub const fn collection_capability(self) -> ParameterCollectionCapability {
        if self.needs_sequence {
            ParameterCollectionCapability::Sequence
        } else if self.needs_collection || self.iteration_count > 1 {
            ParameterCollectionCapability::Collection
        } else if self.iteration_count == 1 {
            ParameterCollectionCapability::Iterable
        } else {
            ParameterCollectionCapability::Unused
        }
    }
}

/// Minimum read-only `collections.abc` capability required by a parameter's usages inside
/// its function body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParameterCollectionCapability {
    /// Parameter is never referenced in the function body.
    Unused,
    /// Parameter is only iterated once at top-level depth (`Iterable` suffices).
    Iterable,
    /// Parameter uses `len(x)`, `v in x`, truthiness (`if x:`), or multi-pass iteration (`Collection` suffices).
    Collection,
    /// Parameter uses indexing/slicing, `reversed(x)`, `.index()`/`.count()`, pattern matching, or escapes (`Sequence` required).
    Sequence,
}

/// Classifies every use of a function's parameters in one walk of its body.
struct ParameterUseVisitor<'a> {
    usages: HashMap<&'a str, ParameterUsage>,
    /// Parameters shadowed by the parameters of an enclosing nested `def` or `lambda`.
    shadowed: Vec<&'a str>,
    loop_or_closure_depth: usize,
}

impl<'a> ParameterUseVisitor<'a> {
    /// Visits the body of a nested `def` or `lambda`, whose `parameters` shadow ours.
    fn visit_nested_scope(
        &mut self,
        parameters: Option<&'a Parameters>,
        visit_body: impl FnOnce(&mut Self),
    ) {
        let outer_shadowed = self.shadowed.len();
        if let Some(parameters) = parameters {
            self.shadowed.extend(
                self.usages
                    .keys()
                    .filter(|name| parameters_shadow_name(parameters, name)),
            );
        }
        self.loop_or_closure_depth += 1;
        visit_body(self);
        self.loop_or_closure_depth -= 1;
        self.shadowed.truncate(outer_shadowed);
    }

    /// Visits a comprehension: later iterables and the elements run once per outer item.
    fn visit_comprehension_scope(
        &mut self,
        generators: &'a [ruff_python_ast::Comprehension],
        elements: impl IntoIterator<Item = &'a Expr>,
    ) {
        let outer_depth = self.loop_or_closure_depth;
        for generator in generators {
            self.visit_expr_as(&generator.iter, UseRole::Iterated);
            self.loop_or_closure_depth += 1;
            self.visit_expr(&generator.target);
            for condition in &generator.ifs {
                self.visit_expr(condition);
            }
        }
        for element in elements {
            self.visit_expr(element);
        }
        self.loop_or_closure_depth = outer_depth;
    }

    fn visit_display_elements(&mut self, elements: &'a [Expr]) {
        for element in elements {
            match element {
                Expr::Starred(starred) => self.visit_expr_as(&starred.value, UseRole::Iterated),
                _ => self.visit_expr(element),
            }
        }
    }

    fn visit_call_expr(&mut self, call: &'a ruff_python_ast::ExprCall) {
        let positional_role = match call.func.as_ref() {
            Expr::Name(func_name) => match func_name.id.as_str() {
                BUILTIN_LEN => Some(UseRole::Sized),
                BOOL_CONSTRUCTOR => Some(UseRole::Truthiness),
                name if is_single_pass_iterable_builtin(name) => Some(UseRole::Iterated),
                name if is_safe_readonly_builtin(name) => Some(UseRole::Read),
                _ => None,
            },
            _ => None,
        };
        match call.func.as_ref() {
            Expr::Attribute(attr) if READONLY_COLLECTION_METHODS.contains(&attr.attr.as_str()) => {
                self.visit_expr_as(&attr.value, UseRole::Read);
            }
            func => self.visit_expr(func),
        }
        let Some(positional_role) = positional_role else {
            self.visit_arguments(&call.arguments);
            return;
        };
        for arg in &call.arguments.args {
            match arg {
                Expr::Starred(starred) => self.visit_expr_as(&starred.value, UseRole::Read),
                _ => self.visit_expr_as(arg, positional_role),
            }
        }
        for keyword in &call.arguments.keywords {
            self.visit_expr_as(&keyword.value, UseRole::Read);
        }
    }

    fn visit_compare_expr(&mut self, comp: &'a ruff_python_ast::ExprCompare) {
        let has_in_operator = comp.ops.iter().any(|op| {
            matches!(
                op,
                ruff_python_ast::CmpOp::In | ruff_python_ast::CmpOp::NotIn
            )
        });
        let is_identity_check = comp.ops.iter().any(|op| {
            matches!(
                op,
                ruff_python_ast::CmpOp::Is | ruff_python_ast::CmpOp::IsNot
            )
        });
        for (idx, operand) in comp.operands.iter().enumerate() {
            let role = if has_in_operator && idx + 1 == comp.operands.len() {
                UseRole::Sized
            } else if is_identity_check {
                UseRole::Identity
            } else {
                UseRole::Read
            };
            self.visit_expr_as(operand, role);
        }
    }

    /// Visits `expr`, recording `role` if it is a direct read of a parameter.
    fn visit_expr_as(&mut self, expr: &'a Expr, role: UseRole) {
        match expr {
            Expr::Name(name) if matches!(name.ctx, ruff_python_ast::ExprContext::Load) => {
                self.record(name.id.as_str(), role);
            }
            Expr::BoolOp(bool_op) if role == UseRole::Truthiness => {
                for value in &bool_op.values {
                    self.visit_expr_as(value, role);
                }
            }
            _ => self.visit_expr(expr),
        }
    }

    fn record(&mut self, name: &str, role: UseRole) {
        if self.shadowed.contains(&name) {
            return;
        }
        let Some(usage) = self.usages.get_mut(name) else {
            return;
        };
        match role {
            UseRole::Escape => {
                usage.mutated_or_escaping = true;
                usage.needs_sequence = true;
            }
            UseRole::Read => usage.needs_sequence = true,
            UseRole::Truthiness | UseRole::Sized => usage.needs_collection = true,
            UseRole::Iterated => {
                usage.iteration_count += 1;
                if self.loop_or_closure_depth > 0 {
                    usage.needs_collection = true;
                }
            }
            UseRole::Identity => {}
        }
    }
}

impl<'a> SourceOrderVisitor<'a> for ParameterUseVisitor<'a> {
    fn visit_annotation(&mut self, _expr: &'a Expr) {}

    fn visit_stmt(&mut self, statement: &'a Stmt) {
        match statement {
            Stmt::TypeAlias(_) => {}
            Stmt::FunctionDef(func) => {
                for decorator in &func.decorator_list {
                    self.visit_decorator(decorator);
                }
                // Defaults are evaluated in the enclosing scope; only the body is shadowed.
                self.visit_parameters(&func.parameters);
                self.visit_nested_scope(Some(&func.parameters), |visitor| {
                    visitor.visit_body(&func.body);
                });
            }
            Stmt::For(for_statement) => {
                self.visit_expr_as(&for_statement.iter, UseRole::Iterated);
                self.loop_or_closure_depth += 1;
                self.visit_expr(&for_statement.target);
                self.visit_body(&for_statement.body);
                self.visit_body(&for_statement.orelse);
                self.loop_or_closure_depth -= 1;
            }
            Stmt::While(while_statement) => {
                self.loop_or_closure_depth += 1;
                self.visit_expr_as(&while_statement.test, UseRole::Truthiness);
                self.visit_body(&while_statement.body);
                self.visit_body(&while_statement.orelse);
                self.loop_or_closure_depth -= 1;
            }
            Stmt::If(if_statement) => {
                self.visit_expr_as(&if_statement.test, UseRole::Truthiness);
                self.visit_body(&if_statement.body);
                for clause in &if_statement.elif_else_clauses {
                    if let Some(test) = &clause.test {
                        self.visit_expr_as(test, UseRole::Truthiness);
                    }
                    self.visit_body(&clause.body);
                }
            }
            Stmt::Assert(assert_statement) => {
                self.visit_expr_as(&assert_statement.test, UseRole::Truthiness);
                if let Some(message) = &assert_statement.msg {
                    self.visit_expr_as(message, UseRole::Read);
                }
            }
            _ => walk_stmt(self, statement),
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Name(name) => self.record(name.id.as_str(), UseRole::Escape),
            Expr::Lambda(lambda) => {
                if let Some(parameters) = &lambda.parameters {
                    self.visit_parameters(parameters);
                }
                self.visit_nested_scope(lambda.parameters.as_deref(), |visitor| {
                    visitor.visit_expr(&lambda.body);
                });
            }
            Expr::UnaryOp(unary) if unary.op == ruff_python_ast::UnaryOp::Not => {
                self.visit_expr_as(&unary.operand, UseRole::Truthiness);
            }
            Expr::If(if_expr) => {
                self.visit_expr_as(&if_expr.test, UseRole::Truthiness);
                self.visit_expr(&if_expr.body);
                self.visit_expr(&if_expr.orelse);
            }
            Expr::Call(call) => self.visit_call_expr(call),
            Expr::Compare(comp) => self.visit_compare_expr(comp),
            Expr::Subscript(sub) if matches!(sub.ctx, ruff_python_ast::ExprContext::Load) => {
                self.visit_expr_as(&sub.value, UseRole::Read);
                self.visit_expr_as(&sub.slice, UseRole::Read);
            }
            Expr::BinOp(bin) => {
                self.visit_expr_as(&bin.left, UseRole::Read);
                self.visit_expr_as(&bin.right, UseRole::Read);
            }
            Expr::List(list) if matches!(list.ctx, ruff_python_ast::ExprContext::Load) => {
                self.visit_display_elements(&list.elts);
            }
            Expr::Tuple(tuple) if matches!(tuple.ctx, ruff_python_ast::ExprContext::Load) => {
                self.visit_display_elements(&tuple.elts);
            }
            Expr::Set(set) => self.visit_display_elements(&set.elts),
            Expr::Dict(dict) => {
                for item in &dict.items {
                    match &item.key {
                        Some(key) => {
                            self.visit_expr(key);
                            self.visit_expr(&item.value);
                        }
                        None => self.visit_expr_as(&item.value, UseRole::Read),
                    }
                }
            }
            Expr::ListComp(comp) => self.visit_comprehension_scope(&comp.generators, [&*comp.elt]),
            Expr::SetComp(comp) => self.visit_comprehension_scope(&comp.generators, [&*comp.elt]),
            Expr::Generator(comp) => {
                self.visit_comprehension_scope(&comp.generators, [&*comp.elt]);
            }
            Expr::DictComp(comp) => self.visit_comprehension_scope(
                &comp.generators,
                comp.key.as_deref().into_iter().chain([&*comp.value]),
            ),
            _ => walk_expr(self, expr),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::ast::ParsedFile;
    use crate::code_lint::ast::python::extract_function_signatures;
    use crate::diagnostic::Language;

    #[rstest::rstest]
    #[case::unused("def f(x):\n    return 1", ParameterCollectionCapability::Unused)]
    #[case::single_for(
        "def f(x):\n    for i in x:\n        print(i)",
        ParameterCollectionCapability::Iterable
    )]
    #[case::single_comprehension(
        "def f(x):\n    return [i for i in x]",
        ParameterCollectionCapability::Iterable
    )]
    #[case::single_consuming_builtin(
        "def f(x):\n    return sum(x)",
        ParameterCollectionCapability::Iterable
    )]
    #[case::len(
        "def f(x):\n    return len(x)",
        ParameterCollectionCapability::Collection
    )]
    #[case::membership(
        "def f(x):\n    return 1 in x",
        ParameterCollectionCapability::Collection
    )]
    #[case::truthiness(
        "def f(x):\n    if x:\n        return 1",
        ParameterCollectionCapability::Collection
    )]
    #[case::negation(
        "def f(x):\n    return not x",
        ParameterCollectionCapability::Collection
    )]
    #[case::assert_truthiness(
        "def f(x):\n    assert x\n    return 1",
        ParameterCollectionCapability::Collection
    )]
    #[case::two_passes(
        "def f(x):\n    return sum(x) + max(x)",
        ParameterCollectionCapability::Collection
    )]
    #[case::iteration_inside_loop(
        "def f(x):\n    for _ in range(3):\n        for i in x:\n            print(i)",
        ParameterCollectionCapability::Collection
    )]
    #[case::index("def f(x):\n    return x[0]", ParameterCollectionCapability::Sequence)]
    #[case::reversed(
        "def f(x):\n    return list(reversed(x))",
        ParameterCollectionCapability::Sequence
    )]
    #[case::index_method(
        "def f(x):\n    return x.index(1)",
        ParameterCollectionCapability::Sequence
    )]
    #[case::pattern_matching(
        "def f(x):\n    match x:\n        case [a]:\n            return a",
        ParameterCollectionCapability::Sequence
    )]
    #[case::passed_to_unknown_function(
        "def f(x):\n    return helper(x)",
        ParameterCollectionCapability::Sequence
    )]
    #[case::shadowing_nested_def_default(
        "def f(x):\n    def g(x=x[0]):\n        return x\n    return g",
        ParameterCollectionCapability::Sequence
    )]
    fn test_parameter_collection_capability(
        #[case] source: &str,
        #[case] expected: ParameterCollectionCapability,
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(
            summarize_parameter_usages(&sigs[0])["x"].collection_capability(),
            expected
        );
    }

    #[rstest::rstest]
    #[case::method_mutation("def f(x):\n    x.append(1)", true)]
    #[case::subscript_write("def f(x):\n    x[0] = 1", true)]
    #[case::subscript_delete("def f(x):\n    del x[0]", true)]
    #[case::augmented_assignment("def f(x):\n    x += [1]", true)]
    #[case::alias("def f(x):\n    y = x\n    return len(y)", true)]
    #[case::stored_in_container("def f(x):\n    y = [x]\n    return len(y)", true)]
    #[case::returned("def f(x):\n    return x", true)]
    #[case::yielded("def f(x):\n    yield x", true)]
    #[case::passed_to_unknown_function("def f(x):\n    helper(x)", true)]
    #[case::passed_to_method("def f(x):\n    registry.register(x)", true)]
    #[case::mutated_in_closure("def f(x):\n    def g():\n        x.append(1)\n    g()", true)]
    #[case::shadowing_nested_def_default(
        "def f(x):\n    def g(x=x):\n        x.append(1)\n    g()",
        true
    )]
    #[case::read_only_iteration("def f(x):\n    for i in x:\n        print(i)", false)]
    #[case::read_only_len("def f(x):\n    return len(x)", false)]
    #[case::shadowed_by_nested_def(
        "def f(x):\n    def g(x):\n        x.append(1)\n    g([])",
        false
    )]
    #[case::shadowed_by_lambda(
        "def f(x):\n    g = lambda x: x.append(1)\n    return len(x)",
        false
    )]
    #[case::shadowed_while_sibling_mutated(
        "def f(x, y):\n    def g(x):\n        y.append(x)\n    return len(x)",
        false
    )]
    fn test_parameter_mutated_or_escaping(#[case] source: &str, #[case] expected: bool) {
        let file = ParsedFile::new(source, Language::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(
            summarize_parameter_usages(&sigs[0])["x"].is_mutated_or_escaping(),
            expected
        );
    }
}
