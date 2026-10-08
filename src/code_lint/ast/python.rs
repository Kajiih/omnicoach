//! AST helper predicates and structural extractors for Python.

mod annotations;
mod classes;
mod format_strings;
mod functions;
mod literals;
mod logging;
mod parameter_usage;
mod positional_reads;
mod scopes;
mod strings;

pub use self::annotations::{
    AnnotationTraversalDepth, CollectionKind, CollectionShape, PythonCollectionType,
    PythonReturnTypeBranch, PythonReturnTypeUnion, collect_collection_types, collection_display,
    collection_type, return_type_union,
};
pub use self::classes::{
    PythonAnnotatedAttribute, PythonBaseClass, PythonClassInfo, PythonFieldAfterMethod,
    PythonInstanceAttributeAnnotation, collect_class_attributes, collect_fields_after_methods,
    collect_instance_attribute_annotations, extract_classes,
};
pub(super) use self::classes::{collect_callable_scopes, collect_type_method_scopes};
pub use self::format_strings::{
    PythonFormatPlaceholder, PythonFormatString, PythonFormatStyle, collect_format_strings,
    extract_valid_field_root, named_format_field_roots,
};
pub use self::functions::{
    PythonFunctionSignature, PythonParameterInfo, PythonParameterKind, extract_function_signatures,
    find_nested_functions,
};
pub(super) use self::literals::collect_literal_occurrences;
pub use self::logging::{PythonLoggerCall, collect_logger_calls};
pub use self::parameter_usage::{
    ParameterCollectionCapability, ParameterUsage, summarize_parameter_usages,
};
pub(super) use self::positional_reads::collect_positional_reads;
pub(super) use self::scopes::collect_bindings;
pub(super) use self::strings::find_unwrapped_multiline_strings;

use self::annotations::{has_final_annotation_expr, is_bare_final_annotation_expr};
use self::classes::is_protocol_or_abc_class;
#[cfg(test)]
use self::functions::extract_parameters;
use self::functions::{
    direct_function_definitions, has_override_decorator, method_receiver_name_ast,
};
use self::literals::is_constant_name;
use self::logging::extract_logger_call;
use self::strings::{fstring_segments_and_interpolations, static_string_text};
use crate::code_lint::ast::{
    AstNode, EnclosingFunction, ParsedFile, span_from_ruff_range, with_innermost_function,
};
use crate::diagnostic::SourceSpan;
#[cfg(test)]
use ruff_python_ast::Parameters;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Decorator, ExceptHandler, Expr, ModModule, Stmt, StmtFunctionDef};
use ruff_text_size::Ranged as _;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const PYTEST_RAISES: &str = "raises";
const ENVIRON_NAME: &str = "environ";

/// The callee of `call` if it is a `call` node, without the type arguments of a generic
/// instantiation (`list[str]()` has callee `list`).
#[must_use]
pub fn call_callee<'a>(call: &AstNode<'a>) -> Option<AstNode<'a>> {
    let Expr::Call(call_expr) = find_expr_at_span(call.file.py_module()?.syntax(), call.span())?
    else {
        return None;
    };
    let callee = if let Expr::Subscript(subscript) = call_expr.func.as_ref() {
        subscript.value.as_ref()
    } else {
        call_expr.func.as_ref()
    };
    Some(AstNode::from_span(
        call.file,
        span_from_ruff_range(callee.range()),
    ))
}

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

/// Finds the `Parameters` node in `module` matching `target_span` (either the `Parameters` span,
/// the enclosing `StmtFunctionDef` span, or the first function's parameters when `target_span`
/// spans the module).
#[cfg(test)]
pub(super) fn find_parameters_at_span(
    module: &ModModule,
    target_span: SourceSpan,
) -> Option<&Parameters> {
    struct ParamsFinder<'a> {
        target_span: SourceSpan,
        module_span: SourceSpan,
        found: Option<&'a Parameters>,
    }

    impl<'a> SourceOrderVisitor<'a> for ParamsFinder<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if self.found.is_some() {
                return;
            }
            if let Stmt::FunctionDef(func_def) = statement
                && (span_from_ruff_range(func_def.range) == self.target_span
                    || span_from_ruff_range(func_def.parameters.range) == self.target_span
                    || self.target_span == self.module_span)
            {
                self.found = Some(&func_def.parameters);
                return;
            }
            walk_stmt(self, statement);
        }

        fn visit_parameters(&mut self, parameters: &'a Parameters) {
            if self.found.is_none() && span_from_ruff_range(parameters.range) == self.target_span {
                self.found = Some(parameters);
            }
        }
    }

    let mut finder = ParamsFinder {
        target_span,
        module_span: span_from_ruff_range(module.range),
        found: None,
    };
    finder.visit_body(&module.body);
    finder.found
}

/// Represents a keyword argument (`key=value`) in a Python call or argument list.
#[derive(Clone)]
pub struct KeywordArg<'a> {
    /// The keyword parameter name.
    pub name: String,
    /// AST node for the argument identifier name.
    pub name_node: AstNode<'a>,
    /// AST node for the argument value expression.
    pub value_node: AstNode<'a>,
}

impl KeywordArg<'_> {
    /// Evaluates literal boolean arguments (`True` / `False`).
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self.value_node.text().as_ref() {
            "True" => Some(true),
            "False" => Some(false),
            _ => None,
        }
    }
}

/// Structured metadata for a Python decorator.
#[derive(Clone)]
pub struct DecoratorInfo<'a> {
    /// Full decorator AST node (including `@`).
    pub node: AstNode<'a>,
    /// Full path string (e.g. `"dataclasses.dataclass"`, `"pytest.mark.parametrize"`).
    pub path: String,
    /// Terminal identifier (e.g. `"dataclass"`, `"parametrize"`).
    pub terminal_name: String,
    /// Call node if the decorator was invoked with parentheses `@dec(...)`.
    pub call_node: Option<AstNode<'a>>,
    /// Parsed keyword arguments if invoked as a call.
    pub keyword_args: Vec<KeywordArg<'a>>,
}

impl<'a> DecoratorInfo<'a> {
    /// Looks up a keyword argument by name.
    #[must_use]
    pub fn get_arg(&self, key: &str) -> Option<&KeywordArg<'a>> {
        self.keyword_args.iter().find(|kw| kw.name == key)
    }

    /// Returns true if a keyword argument with `key` was explicitly passed.
    #[must_use]
    pub fn has_arg(&self, key: &str) -> bool {
        self.get_arg(key).is_some()
    }
}

/// Converts a slice of `ruff_python_ast::Decorator` nodes into `DecoratorInfo` values.
pub(super) fn extract_decorators_from_slice<'a>(
    decorators: &[Decorator],
    file: &'a ParsedFile,
) -> Vec<DecoratorInfo<'a>> {
    let mut out = Vec::with_capacity(decorators.len());
    for decorator in decorators {
        let (call_node, target_expr, keyword_args) = if let Expr::Call(call) = &decorator.expression
        {
            let kwargs = call
                .arguments
                .keywords
                .iter()
                .filter_map(|kw| {
                    let arg_ident = kw.arg.as_ref()?;
                    Some(KeywordArg {
                        name: arg_ident.id.to_string(),
                        name_node: AstNode::from_span(file, span_from_ruff_range(arg_ident.range)),
                        value_node: AstNode::from_span(
                            file,
                            span_from_ruff_range(kw.value.range()),
                        ),
                    })
                })
                .collect();
            (
                Some(AstNode::from_span(file, span_from_ruff_range(call.range()))),
                call.func.as_ref(),
                kwargs,
            )
        } else {
            (None, &decorator.expression, Vec::new())
        };

        let (path, terminal_name) = resolve_path_and_terminal_expr(target_expr, &file.source);
        out.push(DecoratorInfo {
            node: AstNode::from_span(file, span_from_ruff_range(decorator.range)),
            path,
            terminal_name,
            call_node,
            keyword_args,
        });
    }
    out
}

/// A single-name assignment at module level, including inside the bodies of top-level `if`,
/// `try` and `with` statements (`ALLOWED = [...]`, `PORTS: Final = {...}`, `NAME: str`).
pub struct PythonModuleAssignment<'a> {
    /// The assigned name (e.g. `"ALLOWED"` or `"_cache"`).
    pub name: String,
    /// The type annotation, if any.
    pub annotation: Option<AstNode<'a>>,
    /// The assigned value without enclosing parentheses, absent for a bare annotation.
    pub value: Option<AstNode<'a>>,
    /// True if the annotation is `Final` or `Final[T]` (qualified or not, optionally wrapped in
    /// `Annotated`).
    has_final_annotation: bool,
}

impl PythonModuleAssignment<'_> {
    /// Whether the assignment declares a constant: an `UPPER_SNAKE_CASE` name or a `Final`
    /// annotation.
    #[must_use]
    pub fn is_constant(&self) -> bool {
        is_constant_name(&self.name) || self.has_final_annotation
    }
}

/// Collects the single-name module-level assignments of `file`, in source order. Attribute and
/// unpacking targets are not collected.
#[must_use]
pub fn collect_module_assignments(file: &ParsedFile) -> Vec<PythonModuleAssignment<'_>> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_module_assignments_in_stmts(&parsed.syntax().body, file, &mut out);
    out
}

/// Collects single-name module-level assignments from `stmts`, recursing into top-level
/// `if`, `try`, and `with` blocks.
fn collect_module_assignments_in_stmts<'a>(
    stmts: &[Stmt],
    file: &'a ParsedFile,
    out: &mut Vec<PythonModuleAssignment<'a>>,
) {
    for statement in stmts {
        match statement {
            Stmt::Assign(assign) => {
                if let [Expr::Name(target)] = assign.targets.as_slice() {
                    out.push(PythonModuleAssignment {
                        name: target.id.to_string(),
                        annotation: None,
                        value: Some(AstNode::from_span(
                            file,
                            span_from_ruff_range(assign.value.range()),
                        )),
                        has_final_annotation: false,
                    });
                }
            }
            Stmt::AnnAssign(ann) => {
                if let Expr::Name(target) = ann.target.as_ref() {
                    out.push(PythonModuleAssignment {
                        name: target.id.to_string(),
                        annotation: Some(AstNode::from_span(
                            file,
                            span_from_ruff_range(ann.annotation.range()),
                        )),
                        value: ann
                            .value
                            .as_ref()
                            .map(|val| AstNode::from_span(file, span_from_ruff_range(val.range()))),
                        has_final_annotation: has_final_annotation_expr(&ann.annotation, file),
                    });
                }
            }
            Stmt::If(if_statement) => {
                collect_module_assignments_in_stmts(&if_statement.body, file, out);
                for clause in &if_statement.elif_else_clauses {
                    collect_module_assignments_in_stmts(&clause.body, file, out);
                }
            }
            Stmt::Try(try_statement) => {
                collect_module_assignments_in_stmts(&try_statement.body, file, out);
                for handler in &try_statement.handlers {
                    let ExceptHandler::ExceptHandler(handler_clause) = handler;
                    collect_module_assignments_in_stmts(&handler_clause.body, file, out);
                }
                collect_module_assignments_in_stmts(&try_statement.orelse, file, out);
                collect_module_assignments_in_stmts(&try_statement.finalbody, file, out);
            }
            Stmt::With(with_statement) => {
                collect_module_assignments_in_stmts(&with_statement.body, file, out);
            }
            _ => {}
        }
    }
}

/// Collects top-level module statements that appear after an `if __name__ == "__main__":`
/// guard block.
#[must_use]
pub fn collect_statements_after_main_guard(file: &ParsedFile) -> Vec<AstNode<'_>> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let body = &parsed.syntax().body;
    let Some(guard_index) = body.iter().position(is_main_guard_statement) else {
        return Vec::new();
    };
    body[guard_index + 1..]
        .iter()
        .filter(|statement| !is_main_guard_statement(statement))
        .map(|statement| AstNode::from_span(file, span_from_ruff_range(statement.range())))
        .collect()
}

/// Returns true if `statement` is `if __name__ == "__main__":` or `if "__main__" == __name__:`.
fn is_main_guard_statement(statement: &Stmt) -> bool {
    let Stmt::If(if_statement) = statement else {
        return false;
    };
    let Expr::Compare(compare) = if_statement.test.as_ref() else {
        return false;
    };
    let ([ruff_python_ast::CmpOp::Eq], [left, right]) = (&*compare.ops, &*compare.operands) else {
        return false;
    };
    (is_dunder_name_expr(left) && is_dunder_main_literal(right))
        || (is_dunder_main_literal(left) && is_dunder_name_expr(right))
}

/// Returns true if `expr` is the identifier `__name__`.
fn is_dunder_name_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::Name(name) if name.id.as_str() == "__name__")
}

/// Returns true if `expr` is a string literal with value `"__main__"`.
fn is_dunder_main_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::StringLiteral(literal) if literal.value.to_str() == "__main__")
}

/// A Python `assert` statement with the facts about its condition that packing checks need.
pub struct PythonAssert<'a> {
    /// The `assert` statement node.
    pub node: AstNode<'a>,
    /// True if the condition has a top-level `and` boolean operator.
    pub has_top_level_logical_and: bool,
    /// True if the condition is a comparison with an operand that is a tuple or list of two or
    /// more boolean literals.
    pub has_boolean_literal_comparison: bool,
}

/// Collects all Python `Stmt::Assert` nodes in `file`.
#[must_use]
pub fn collect_assert_statements(file: &ParsedFile) -> Vec<PythonAssert<'_>> {
    struct AssertCollector<'a> {
        file: &'a ParsedFile,
        out: Vec<PythonAssert<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for AssertCollector<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::Assert(assert_statement) = statement {
                let test = assert_statement.test.as_ref();
                self.out.push(PythonAssert {
                    node: AstNode::from_span(
                        self.file,
                        span_from_ruff_range(assert_statement.range()),
                    ),
                    has_top_level_logical_and: matches!(
                        test,
                        Expr::BoolOp(bool_op) if bool_op.op == ruff_python_ast::BoolOp::And
                    ),
                    has_boolean_literal_comparison: matches!(
                        test,
                        Expr::Compare(comparison)
                            if comparison.operands.iter().any(is_boolean_literal_collection_expr)
                    ),
                });
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut collector = AssertCollector {
        file,
        out: Vec::new(),
    };
    collector.visit_body(&parsed.syntax().body);
    collector.out
}

/// Returns true if `expr` is a Python `tuple` or `list` consisting solely of `>= 2` boolean literals (`True` / `False`).
fn is_boolean_literal_collection_expr(expr: &Expr) -> bool {
    let elements = match expr {
        Expr::Tuple(tuple) => &tuple.elts,
        Expr::List(list) => &list.elts,
        _ => return false,
    };
    elements.len() >= 2
        && elements
            .iter()
            .all(|element| matches!(element, Expr::BooleanLiteral(_)))
}

/// Collects all outermost Python test functions along with their identifier node, name, and assertion count.
#[must_use]
pub(super) fn collect_test_function_assertion_counts(
    file: &ParsedFile,
) -> Vec<(AstNode<'_>, String, usize)> {
    struct TestAssertionCollector<'a> {
        file: &'a ParsedFile,
        out: Vec<(AstNode<'a>, String, usize)>,
    }

    impl<'a> SourceOrderVisitor<'a> for TestAssertionCollector<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::FunctionDef(func_def) = statement {
                if is_test_function_def(func_def) {
                    let count = count_python_assertions_in_body(&func_def.body);
                    self.out.push((
                        AstNode::from_span(self.file, span_from_ruff_range(func_def.name.range)),
                        func_def.name.to_string(),
                        count,
                    ));
                }
                return;
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut collector = TestAssertionCollector {
        file,
        out: Vec::new(),
    };
    collector.visit_body(&parsed.syntax().body);
    collector.out
}

/// Returns true if a Python `StmtFunctionDef` is a test function (`test` or `test_*`).
fn is_test_function_def(func_def: &StmtFunctionDef) -> bool {
    let func_name = func_def.name.as_str();
    func_name == "test" || func_name.starts_with("test_")
}

/// Counts top-level assertion constructs in a Python test function body.
fn count_python_assertions_in_body(body: &[Stmt]) -> usize {
    struct AssertionCounter {
        count: usize,
    }

    impl<'a> SourceOrderVisitor<'a> for AssertionCounter {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            match statement {
                Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
                Stmt::Assert(_) => {
                    self.count += 1;
                }
                _ => walk_stmt(self, statement),
            }
        }

        fn visit_expr(&mut self, expr: &'a Expr) {
            if let Expr::Call(call) = expr
                && is_assertion_call_expr(call)
            {
                self.count += 1;
                return;
            }
            walk_expr(self, expr);
        }
    }

    let mut counter = AssertionCounter { count: 0 };
    counter.visit_body(body);
    counter.count
}

/// Returns true if a Python `ExprCall` is a test assertion call
/// (`self.assert*()`, `pytest.raises(...)`, `raises(...)`, `pytest.warns(...)`, `self.fail(...)`).
fn is_assertion_call_expr(call: &ruff_python_ast::ExprCall) -> bool {
    match call.func.as_ref() {
        Expr::Name(name) => name.id.as_str() == PYTEST_RAISES,
        Expr::Attribute(attr) => {
            let attr_name = attr.attr.as_str();
            if attr_name.starts_with("assert") {
                return true;
            }
            let Expr::Name(obj) = attr.value.as_ref() else {
                return false;
            };
            let obj_text = obj.id.as_str();
            (obj_text == "pytest" && (attr_name == PYTEST_RAISES || attr_name == "warns"))
                || (obj_text == "self" && attr_name == "fail")
        }
        _ => false,
    }
}

/// Tracks the functions enclosing the statement visited by a source-order walk.
#[derive(Default)]
pub(in crate::code_lint::ast) struct EnclosingFunctionTracker {
    /// Number of statements enclosing the visited statement.
    depth: usize,
    /// The enclosing functions, innermost first.
    pub(in crate::code_lint::ast) functions: Arc<[EnclosingFunction]>,
}

impl EnclosingFunctionTracker {
    /// Enters `statement`, which encloses everything visited until the matching [`Self::exit`].
    /// Returns the outer functions to pass to it.
    pub(in crate::code_lint::ast) fn enter(
        &mut self,
        statement: &Stmt,
    ) -> Arc<[EnclosingFunction]> {
        let outer = Arc::clone(&self.functions);
        if let Stmt::FunctionDef(func_def) = statement {
            let function = EnclosingFunction {
                name: func_def.name.to_string(),
                is_top_level: self.depth == 0,
            };
            self.functions = with_innermost_function(function, &outer);
        }
        self.depth += 1;
        outer
    }

    /// Leaves the statement entered by the [`Self::enter`] call that returned `outer`.
    pub(in crate::code_lint::ast) fn exit(&mut self, outer: Arc<[EnclosingFunction]>) {
        self.depth -= 1;
        self.functions = outer;
    }
}

/// A Python subscript reading the `os.environ` (or bare `environ`) mapping.
pub struct PythonEnvironSubscript<'a> {
    /// The whole subscript (`os.environ["HOST"]`).
    pub node: AstNode<'a>,
    /// The mapping expression being indexed (`os.environ` or `environ`).
    pub mapping: AstNode<'a>,
    /// The functions enclosing the subscript, innermost first (see
    /// [`AstCallCandidate::enclosing_functions`](crate::code_lint::ast::AstCallCandidate::enclosing_functions)).
    pub enclosing_functions: Arc<[EnclosingFunction]>,
}

/// Collects all Python subscript expressions indexing into `os.environ` or `environ`.
#[must_use]
pub fn collect_environ_subscripts(file: &ParsedFile) -> Vec<PythonEnvironSubscript<'_>> {
    struct EnvironSubscriptCollector<'a> {
        file: &'a ParsedFile,
        enclosing_functions: EnclosingFunctionTracker,
        out: Vec<PythonEnvironSubscript<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for EnvironSubscriptCollector<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            let outer_functions = self.enclosing_functions.enter(statement);
            walk_stmt(self, statement);
            self.enclosing_functions.exit(outer_functions);
        }

        fn visit_expr(&mut self, expr: &'a Expr) {
            if let Expr::Subscript(sub) = expr
                && is_environ_mapping_expr(&sub.value)
            {
                self.out.push(PythonEnvironSubscript {
                    node: AstNode::from_span(self.file, span_from_ruff_range(sub.range())),
                    mapping: AstNode::from_span(self.file, span_from_ruff_range(sub.value.range())),
                    enclosing_functions: Arc::clone(&self.enclosing_functions.functions),
                });
            }
            walk_expr(self, expr);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut collector = EnvironSubscriptCollector {
        file,
        enclosing_functions: EnclosingFunctionTracker::default(),
        out: Vec::new(),
    };
    collector.visit_body(&parsed.syntax().body);
    collector.out
}

/// Returns true if `value` is Python's `os.environ` or bare `environ`.
fn is_environ_mapping_expr(value: &Expr) -> bool {
    match value {
        Expr::Name(name) => name.id.as_str() == ENVIRON_NAME,
        Expr::Attribute(attr) => {
            attr.attr.as_str() == ENVIRON_NAME
                && matches!(attr.value.as_ref(), Expr::Name(obj) if obj.id.as_str() == "os")
        }
        _ => false,
    }
}

/// Methods that mutate a `list`, `dict` or `set` in place.
const MUTATING_METHODS: &[&str] = &[
    "append",
    "extend",
    "insert",
    "pop",
    "remove",
    "clear",
    "sort",
    "reverse",
    "update",
    "setdefault",
    "popitem",
    "add",
    "discard",
    "intersection_update",
    "difference_update",
    "symmetric_difference_update",
];

/// If `expr` mutates a collection receiver in place (`receiver.append(...)`, `receiver[k] = v`,
/// `del receiver[k]`), returns that `receiver` expression.
pub(super) fn in_place_mutated_receiver_expr(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::Call(call) => {
            let Expr::Attribute(attr) = call.func.as_ref() else {
                return None;
            };
            if MUTATING_METHODS.contains(&attr.attr.as_str()) {
                Some(&attr.value)
            } else {
                None
            }
        }
        Expr::Subscript(sub)
            if matches!(
                sub.ctx,
                ruff_python_ast::ExprContext::Store | ruff_python_ast::ExprContext::Del
            ) =>
        {
            Some(&sub.value)
        }
        _ => None,
    }
}

/// Helper to resolve the dotted expression path and terminal identifier from an `Expr`.
pub(super) fn resolve_path_and_terminal_expr(expr: &Expr, source: &str) -> (String, String) {
    let span = span_from_ruff_range(expr.range());
    let path = source[span.start..span.end].to_string();
    let terminal = if let Expr::Attribute(attr) = expr {
        attr.attr.to_string()
    } else {
        path.rsplit('.').next().unwrap_or("").to_string()
    };
    (path, terminal)
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

/// Extracts the terminal function/method name if `expr` is a `call` expression.
fn called_terminal_name_expr(expr: &Expr, source: &str) -> Option<String> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let (_, terminal) = resolve_path_and_terminal_expr(&call.func, source);
    (!terminal.is_empty()).then_some(terminal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::ast::collect_call_candidates;
    use crate::diagnostic::Language;

    #[test]
    fn test_collect_bindings_python() {
        let source = indoc::indoc! {r"
            import os
            import os.path
            import numpy as np
            from sys import stderr as err
            from os import path
            import sys, re
            from os import * # wildcard
            c = 2
            d, e = 3, 4
            for x in range(10): pass
            [y for y in range(10)]
            def foo(b: int = 1):
                pass
            try:
                pass
            except Exception as g:
                pass
            (v := 1)
            class MyClass:
                def my_method(self):
                    pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let bindings = collect_bindings(&file);
        let names: Vec<String> = bindings
            .iter()
            .map(|binding| binding.node.text().to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "os",
                "os",
                "np",
                "err",
                "path",
                "sys",
                "re",
                "c",
                "d",
                "e",
                "x",
                "y",
                "foo",
                "b",
                "g",
                "v",
                "MyClass",
                "my_method",
                "self",
            ]
        );
    }

    #[test]
    fn test_collect_bindings_python_match_case() {
        let source = indoc::indoc! {r"
            match val:
                case Point(x, y=z):
                    pass
                case [a, b]:
                    pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let bindings = collect_bindings(&file);
        let names: Vec<String> = bindings
            .iter()
            .map(|binding| binding.node.text().to_string())
            .collect();
        assert_eq!(names, vec!["x", "z", "a", "b"]);
    }

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

    /// Returns the first top-level function definition of `file`.
    fn first_function_def(file: &ParsedFile) -> &StmtFunctionDef {
        let parsed = file.py_module().expect("source should parse");
        let Some(Stmt::FunctionDef(func_def)) = parsed.syntax().body.first() else {
            unreachable!("source should start with a function definition");
        };
        func_def
    }

    /// Extracts the decorators of the first top-level function definition in `file`.
    fn first_function_decorators(file: &ParsedFile) -> Vec<DecoratorInfo<'_>> {
        extract_decorators_from_slice(&first_function_def(file).decorator_list, file)
    }

    #[test]
    fn test_extract_decorators_count() {
        let source = indoc::indoc! {r#"
            @dataclass(frozen=True, slots=False)
            @pytest.mark.parametrize("x", [1, 2])
            @custom
            def foo():
                pass
        "#};
        let file = ParsedFile::new(source, Language::Python);
        let decorators = first_function_decorators(&file);
        assert_eq!(decorators.len(), 3);
    }

    #[test]
    fn test_extract_decorator_keyword_args() {
        let source = indoc::indoc! {r"
            @dataclass(frozen=True, slots=False)
            def foo():
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let decorators = first_function_decorators(&file);

        let dec0 = &decorators[0];
        assert_eq!(dec0.terminal_name, "dataclass");
        assert_eq!(
            dec0.get_arg("frozen").and_then(KeywordArg::as_bool),
            Some(true)
        );
        assert_eq!(
            dec0.get_arg("slots").and_then(KeywordArg::as_bool),
            Some(false)
        );
        assert!(dec0.get_arg("unknown").is_none());
    }

    #[test]
    fn test_extract_decorators_metadata_and_path() {
        let source = indoc::indoc! {r#"
            @pytest.mark.parametrize("x", [1, 2])
            @custom
            def foo():
                pass
        "#};
        let file = ParsedFile::new(source, Language::Python);
        let decorators = first_function_decorators(&file);

        let dec0 = &decorators[0];
        assert_eq!(dec0.terminal_name, "parametrize");
        assert_eq!(dec0.path, "pytest.mark.parametrize");

        let dec1 = &decorators[1];
        assert_eq!(dec1.terminal_name, "custom");
        assert!(dec1.call_node.is_none());
    }

    #[rstest::rstest]
    #[case::other_decorator("@pytest.mark.parametrize(\"x\", [1, 2])", false)]
    #[case::bare_override("@override", true)]
    #[case::qualified_override("@typing_extensions.override", true)]
    fn test_has_override_decorator(#[case] decorator: &str, #[case] expected: bool) {
        let source = format!("{decorator}\ndef foo():\n    pass\n");
        let file = ParsedFile::new(&source, Language::Python);
        let decorators = &first_function_def(&file).decorator_list;
        assert_eq!(has_override_decorator(decorators, &file), expected);
    }

    #[test]
    fn test_extract_classes_and_inheritance() {
        let source = indoc::indoc! {r"
            class FakeService(abc.ABC, Protocol):
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let classes = extract_classes(&file);
        assert_eq!(classes.len(), 1);

        let cls = &classes[0];
        assert_eq!(cls.name, "FakeService");
        assert!(cls.inherits_from("Protocol"));
        assert!(cls.inherits_from("ABC"));
    }

    #[test]
    fn test_extract_classes_bases_metadata() {
        let source = indoc::indoc! {r"
            class FakeService(abc.ABC, Protocol):
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let classes = extract_classes(&file);
        let cls = &classes[0];
        assert_eq!(cls.bases.len(), 2);
        assert_eq!(cls.bases[0].name, "abc.ABC");
        assert_eq!(cls.bases[1].name, "Protocol");
    }

    #[test]
    fn test_extract_classes_dataclass() {
        let source = indoc::indoc! {r"
            @dataclass(frozen=True)
            class Config:
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let classes = extract_classes(&file);
        assert_eq!(classes.len(), 1);

        let cls = &classes[0];
        assert_eq!(cls.name, "Config");
        assert_eq!(cls.decorators[0].terminal_name, "dataclass");
        assert!(!cls.inherits_from("Protocol"));
    }

    #[test]
    fn test_extract_parameters_kinds() {
        let source = indoc::indoc! {r"
            def handler(self, a: int, b: str = 'hello', *, c: bool, **kwargs):
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let func = &extract_function_signatures(&file)[0].node;
        let params = extract_parameters(func);

        assert_eq!(params[0].kind, PythonParameterKind::Receiver);
        assert_eq!(params[1].kind, PythonParameterKind::Positional);
        assert_eq!(params[3].kind, PythonParameterKind::KeywordOnly);
        assert_eq!(params[4].kind, PythonParameterKind::VarKeyword);
    }

    #[test]
    fn test_extract_parameters_type_annotations() {
        let source = indoc::indoc! {r"
            def handler(self, a: int, b: str = 'hello', *, c: bool, **kwargs):
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let func = &extract_function_signatures(&file)[0].node;
        let params = extract_parameters(func);

        assert_eq!(params[1].name, "a");
        assert_eq!(params[1].type_text.as_deref(), Some("int"));
        assert_eq!(params[2].name, "b");
        assert_eq!(params[2].type_text.as_deref(), Some("str"));
    }

    #[rstest::rstest]
    #[case::a1_bare_concrete("def f(x: list, y: dict, z: typing.List) -> None: pass", &[&["list"][..], &["dict"][..], &["typing.List"][..]])]
    #[case::a2_pep585_generic("def f(x: list[int], y: dict[str, int], z: set[str]) -> None: pass", &[&["list"][..], &["dict"][..], &["set"][..]])]
    #[case::a3_pep484_qualified_and_unqualified("def f(x: typing.List[int], y: typing.Dict[str, int], z: typing.Set[str], w: Set[str]) -> None: pass", &[&["typing.List"][..], &["typing.Dict"][..], &["typing.Set"][..], &["Set"][..]])]
    #[case::a4_pep604_union("def f(x: list[int] | None, y: int | list[str] | set[int]) -> None: pass", &[&["list"][..], &["list", "set"][..]])]
    #[case::a5_qualified_pep604_union("def f(x: typing.List[int] | None) -> None: pass", &[&["typing.List"][..]])]
    #[case::a6_optional_and_union("def f(x: Optional[list[int]], y: Union[list[int], str, None]) -> None: pass", &[&["list"][..], &["list"][..]])]
    #[case::a7_qualified_optional_and_union("def f(x: typing.Optional[typing.Dict[str, int]], y: typing.Union[list[int], None]) -> None: pass", &[&["typing.Dict"][..], &["list"][..]])]
    #[case::a8_annotated_unwraps_first_arg("def f(x: Annotated[list[int], 'meta'], y: typing.Annotated[set[str], Doc('x')]) -> None: pass", &[&["list"][..], &["set"][..]])]
    #[case::a9_annotated_ignores_metadata("def f(x: Annotated[Sequence[int], list]) -> None: pass", &[&[][..]])]
    #[case::a10_qualifiers("def f(x: ClassVar[list[str]], y: Final[dict[str, int]], z: Required[set[str]]) -> None: pass", &[&["list"][..], &["dict"][..], &["set"][..]])]
    #[case::a11_covariant_single_and_tuple_containers("def f(a: Sequence[list[int]], b: Collection[set[str]], c: Awaitable[list[int]], d: tuple[str, dict[str, int]], e: tuple[list[int], ...]) -> None: pass", &[&["list"][..], &["set"][..], &["list"][..], &["dict"][..], &["list"][..]])]
    #[case::a12_mapping_covariant_value("def f(x: Mapping[str, list[int]]) -> None: pass", &[&["list"][..]])]
    #[case::a13_callable_covariant_return("def f(cb: Callable[[int], list[str]]) -> None: pass", &[&["list"][..]])]
    #[case::a14_callable_contravariant_param_ignored("def f(cb: Callable[[list[int]], None]) -> None: pass", &[&[][..]])]
    #[case::a15_invariant_mutable_outer_ignored("def f(x: MutableMapping[str, list[int]], y: MutableSequence[list[int]]) -> None: pass", &[&[][..], &[][..]])]
    #[case::a16_unknown_generic_ignored("def f(x: CustomBox[list[int]]) -> None: pass", &[&[][..]])]
    #[case::a17_abstract_collections_pass("def f(a: Sequence[int], b: Mapping[str, int], c: AbstractSet[str], d: collections.abc.Set[str]) -> None: pass", &[&[][..], &[][..], &[][..], &[][..]])]
    #[case::a17b_unaliased_collections_abc_set_import("from collections.abc import Set\ndef f(a: Set[str], b: typing.Set[str]) -> None: pass", &[&[][..], &["typing.Set"][..]])]
    #[case::a18_immutable_builtins_pass("def f(a: tuple[int, ...], b: frozenset[str], c: bytes, d: str) -> None: pass", &[&[][..], &[][..], &[][..], &[][..]])]
    #[case::a19_invariant_concrete_outer_not_recursed("def f(x: list[set[str]], y: dict[str, list[int]]) -> None: pass", &[&["list"][..], &["dict"][..]])]
    #[case::a20_typed_dict_qualifiers("def f(x: NotRequired[list[int]], y: ReadOnly[dict[str, int]]) -> None: pass", &[&["list"][..], &["dict"][..]])]
    #[case::a21_generator_yield_and_return_positions("def f(a: Generator[list[int], set[str], dict[str, int]], b: Coroutine[None, set[str], list[int]], c: AsyncGenerator[list[int], set[str]]) -> None: pass", &[&["list", "dict"][..], &["list"][..], &["list"][..]])]
    #[case::a22_iterator_and_abstract_set_covariant("def f(a: Iterable[list[int]], b: Iterator[dict[str, int]], c: AbstractSet[frozenset[int]]) -> None: pass", &[&["list"][..], &["dict"][..], &[][..]])]
    #[case::a23_qualified_covariant_outer("def f(x: collections.abc.Sequence[list[int]], y: typing.Mapping[str, set[str]]) -> None: pass", &[&["list"][..], &["set"][..]])]
    fn test_collect_collection_types_concrete_mutable_matrix_a(
        #[case] source: &str,
        #[case] expected_per_param: &[&[&str]],
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(sigs.len(), 1);
        let actual: Vec<Vec<String>> = sigs[0]
            .parameters
            .iter()
            .map(|parameter| {
                let type_node = parameter.type_node.as_ref().expect("param should be typed");
                collect_collection_types(type_node, AnnotationTraversalDepth::CovariantPositions)
                    .into_iter()
                    .filter(|collection_type| {
                        collection_type.kind == CollectionKind::ConcreteMutable
                    })
                    .map(|collection_type| collection_type.path)
                    .collect()
            })
            .collect();
        let expected: Vec<Vec<String>> = expected_per_param
            .iter()
            .map(|slice| {
                slice
                    .iter()
                    .map(|expected_name| (*expected_name).to_string())
                    .collect()
            })
            .collect();
        assert_eq!(actual, expected);
    }

    #[rstest::rstest]
    #[case::ellipsis_stub("def f(x: list[int]) -> None: ...", true)]
    #[case::pass_stub("def f(x: list[int]) -> None:\n    pass", true)]
    #[case::docstring_and_ellipsis(
        "def f(x: list[int]) -> None:\n    \"\"\"Doc.\"\"\"\n    ...",
        true
    )]
    #[case::raise_not_implemented_bare(
        "def f(x: list[int]) -> None:\n    raise NotImplementedError",
        true
    )]
    #[case::raise_not_implemented_call(
        "def f(x: list[int]) -> None:\n    raise NotImplementedError('todo')",
        true
    )]
    #[case::real_body("def f(x: list[int]) -> int:\n    return len(x)", false)]
    #[case::raise_from_not_implemented_cause(
        "def f(x: list[int]) -> None:\n    raise ValueError() from NotImplementedError",
        false
    )]
    fn test_has_stub_body(#[case] source: &str, #[case] expected: bool) {
        let file = ParsedFile::new(source, Language::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(sigs[0].has_stub_body, expected);
    }

    #[rstest::rstest]
    #[case::protocol_class(
        "class P(Protocol):\n    def f(self, x: list[int]) -> None: pass",
        true
    )]
    #[case::generic_protocol_class(
        "class P(typing.Protocol[T]):\n    def f(self, x: list[int]) -> None: pass",
        true
    )]
    #[case::abc_class("class A(abc.ABC):\n    def f(self, x: list[int]) -> None: pass", true)]
    #[case::abc_meta_class(
        "class A(metaclass=abc.ABCMeta):\n    def f(self, x: list[int]) -> None: pass",
        true
    )]
    #[case::regular_class("class C:\n    def f(self, x: list[int]) -> None: pass", false)]
    fn test_is_in_protocol_or_abc_class(#[case] source: &str, #[case] expected: bool) {
        let file = ParsedFile::new(source, Language::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(sigs[0].is_in_protocol_or_abc_class, expected);
    }

    /// Collects the top-level collection types of the annotation of the first parameter of the
    /// first function in `source`, as `(path, kind, shape)`.
    fn top_level_collection_types(source: &str) -> Vec<(String, CollectionKind, CollectionShape)> {
        let file = ParsedFile::new(source, Language::Python);
        let sigs = extract_function_signatures(&file);
        let type_node = sigs[0].parameters[0]
            .type_node
            .as_ref()
            .expect("param should be typed");
        collect_collection_types(type_node, AnnotationTraversalDepth::TransparentWrappersOnly)
            .into_iter()
            .map(|collection_type| {
                (
                    collection_type.path,
                    collection_type.kind,
                    collection_type.shape,
                )
            })
            .collect()
    }

    #[rstest::rstest]
    #[case::list(
        "def f(x: list[int]): pass",
        "list",
        CollectionKind::ConcreteMutable,
        CollectionShape::Sequence
    )]
    #[case::typing_dict(
        "def f(x: typing.Dict[str, int]): pass",
        "typing.Dict",
        CollectionKind::ConcreteMutable,
        CollectionShape::Mapping
    )]
    #[case::unqualified_set(
        "def f(x: Set[int]): pass",
        "Set",
        CollectionKind::ConcreteMutable,
        CollectionShape::Set
    )]
    #[case::deque(
        "def f(x: collections.deque[int]): pass",
        "collections.deque",
        CollectionKind::ConcreteMutable,
        CollectionShape::Sequence
    )]
    #[case::counter(
        "def f(x: collections.Counter[str]): pass",
        "collections.Counter",
        CollectionKind::ConcreteMutable,
        CollectionShape::Mapping
    )]
    #[case::mutable_sequence(
        "def f(x: MutableSequence[int]): pass",
        "MutableSequence",
        CollectionKind::AbstractMutable,
        CollectionShape::Sequence
    )]
    #[case::mutable_mapping(
        "def f(x: collections.abc.MutableMapping[str, int]): pass",
        "collections.abc.MutableMapping",
        CollectionKind::AbstractMutable,
        CollectionShape::Mapping
    )]
    #[case::mutable_set(
        "def f(x: typing.MutableSet[int] | None): pass",
        "typing.MutableSet",
        CollectionKind::AbstractMutable,
        CollectionShape::Set
    )]
    #[case::sequence(
        "def f(x: Optional[collections.abc.Sequence[int]]): pass",
        "collections.abc.Sequence",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Sequence
    )]
    #[case::mapping(
        "def f(x: Mapping[str, Sequence[int]]): pass",
        "Mapping",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Mapping
    )]
    #[case::abc_set(
        "def f(x: collections.abc.Set[int]): pass",
        "collections.abc.Set",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Set
    )]
    #[case::imported_abc_set(
        "from collections.abc import Set\ndef f(x: Set[int]): pass",
        "Set",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Set
    )]
    #[case::collection(
        "def f(x: typing.Collection[int]): pass",
        "typing.Collection",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Iterable
    )]
    #[case::iterable(
        "def f(x: Iterable[int]): pass",
        "Iterable",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Iterable
    )]
    #[case::tuple(
        "def f(x: tuple[int, ...]): pass",
        "tuple",
        CollectionKind::Immutable,
        CollectionShape::Sequence
    )]
    #[case::frozenset(
        "def f(x: frozenset[int]): pass",
        "frozenset",
        CollectionKind::Immutable,
        CollectionShape::Set
    )]
    fn test_collection_type_taxonomy(
        #[case] source: &str,
        #[case] path: &str,
        #[case] kind: CollectionKind,
        #[case] shape: CollectionShape,
    ) {
        assert_eq!(
            top_level_collection_types(source),
            [(path.to_string(), kind, shape)]
        );
    }

    #[rstest::rstest]
    #[case::nested_not_collected("def f(x: Sequence[MutableSequence[int]]): pass", &["Sequence"])]
    #[case::unknown_module_ignored("def f(x: mylib.MutableSequence[int]): pass", &[])]
    #[case::union_in_order("def f(x: dict[str, int] | list[int] | dict[str, str]): pass", &["dict", "list"])]
    fn test_collect_top_level_collection_types(#[case] source: &str, #[case] expected: &[&str]) {
        let paths: Vec<String> = top_level_collection_types(source)
            .into_iter()
            .map(|(path, _, _)| path)
            .collect();
        assert_eq!(paths, expected);
    }

    #[rstest::rstest]
    #[case::annotated_value(
        "ALLOWED: list[str] = (\"a\",)",
        &[("ALLOWED", Some("list[str]"), Some("(\"a\",)"), true)]
    )]
    #[case::parenthesized_value_with_comment(
        "ALLOWED = (\n    # Default roles\n    [\"admin\"]\n)",
        &[("ALLOWED", None, Some("[\"admin\"]"), true)]
    )]
    #[case::bare_annotation("NAME: str", &[("NAME", Some("str"), None, true)])]
    #[case::lowercase_final(
        "from typing import Final\nallowed: Final = 1",
        &[("allowed", Some("Final"), Some("1"), true)]
    )]
    #[case::lowercase_variable("cache = {}", &[("cache", None, Some("{}"), false)])]
    #[case::inside_top_level_if("if FLAG:\n    HOSTS = []", &[("HOSTS", None, Some("[]"), true)])]
    #[case::other_targets_and_scopes_not_collected(
        "config.X = 1\nA, B = 1, 2\ndef f():\n    LOCAL = 1\nclass C:\n    ATTR = 1",
        &[]
    )]
    fn test_collect_module_assignments(
        #[case] source: &str,
        #[case] expected: &[(&str, Option<&str>, Option<&str>, bool)],
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let assignments = collect_module_assignments(&file);
        let collected: Vec<_> = assignments
            .iter()
            .map(|assignment| {
                (
                    assignment.name.as_str(),
                    assignment.annotation.as_ref().map(AstNode::text),
                    assignment.value.as_ref().map(AstNode::text),
                    assignment.is_constant(),
                )
            })
            .collect();
        let expected: Vec<_> = expected
            .iter()
            .map(|&(name, annotation, value, is_constant)| {
                (
                    name,
                    annotation.map(Into::into),
                    value.map(Into::into),
                    is_constant,
                )
            })
            .collect();
        assert_eq!(collected, expected);
    }

    #[rstest::rstest]
    #[case::list_display("[1]", Some(("list", CollectionShape::Sequence)))]
    #[case::set_comprehension("{x for x in y}", Some(("set", CollectionShape::Set)))]
    #[case::empty_dict_display("{}", Some(("dict", CollectionShape::Mapping)))]
    #[case::generic_constructor_call("list[str]()", Some(("list", CollectionShape::Sequence)))]
    #[case::qualified_constructor_call(
        "collections.Counter()",
        Some(("collections.Counter", CollectionShape::Mapping))
    )]
    #[case::tuple_display("(1, 2)", None)]
    #[case::unknown_call("make()", None)]
    fn test_collection_display_and_call_callee_type(
        #[case] value: &str,
        #[case] expected: Option<(&str, CollectionShape)>,
    ) {
        let file = ParsedFile::new(&format!("VALUE = {value}"), Language::Python);
        let assignments = collect_module_assignments(&file);
        let value = assignments[0].value.as_ref().expect("value");
        let built = collection_display(value)
            .or_else(|| call_callee(value).and_then(|callee| collection_type(&callee)));
        assert_eq!(
            built
                .as_ref()
                .map(|collection| (collection.path.as_str(), collection.shape)),
            expected
        );
    }

    #[rstest::rstest]
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

    #[rstest::rstest]
    #[case::class_level("class C:\n    items: list[int]", &[("items", false, false)])]
    #[case::private_included("class C:\n    _items: list[int]", &[("_items", false, false)])]
    #[case::init_attribute("class C:\n    def __init__(self):\n        self.items: list[int] = []", &[("items", false, false)])]
    #[case::self_mutation("class C:\n    items: list[int]\n    def add(self):\n        self.items.append(1)", &[("items", true, false)])]
    #[case::cls_mutation("class C:\n    items: list[int]\n    @classmethod\n    def add(cls):\n        cls.items.append(1)", &[("items", true, false)])]
    #[case::class_name_mutation("class C:\n    items: list[int]\n    def add(self):\n        C.items.append(1)", &[("items", true, false)])]
    #[case::subscript_delete("class C:\n    items: list[int]\n    def pop(self):\n        del self.items[0]", &[("items", true, false)])]
    #[case::protocol_member("class P(Protocol):\n    items: list[int]", &[("items", false, true)])]
    fn test_collect_class_attributes(
        #[case] source: &str,
        #[case] expected: &[(&str, bool, bool)],
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let actual: Vec<(String, bool, bool)> = collect_class_attributes(&file)
            .into_iter()
            .map(|attribute| {
                (
                    attribute.name,
                    attribute.is_mutated_in_class,
                    attribute.is_in_protocol_or_abc,
                )
            })
            .collect();
        let expected: Vec<(String, bool, bool)> = expected
            .iter()
            .map(|(name, mutated, in_protocol)| ((*name).to_string(), *mutated, *in_protocol))
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_extract_classes_ignores_comments_and_keywords_in_superclasses() {
        let source = indoc::indoc! {r"
            class FakeClient(
                # Not a base class
                metaclass=ABCMeta,
            ):
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let classes = extract_classes(&file);
        assert_eq!(classes.len(), 1);
        assert!(classes[0].bases.is_empty());
    }

    #[test]
    fn test_python_base_class_is_structural_marker() {
        let source = indoc::indoc! {r"
            class C(
                HttpClient,
                object,
                Generic[T],
                Protocol,
                typing_extensions.Generic[T],
                typing_extensions.Protocol,
                ABC,
                abc.ABC,
                Repository[T],
                **kwargs,
            ):
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let classes = extract_classes(&file);
        let summary: Vec<(&str, bool)> = classes[0]
            .bases
            .iter()
            .map(|base| (base.name.as_str(), base.is_structural_marker()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("HttpClient", false),
                ("object", true),
                ("Generic[T]", true),
                ("Protocol", true),
                ("typing_extensions.Generic[T]", true),
                ("typing_extensions.Protocol", true),
                ("ABC", true),
                ("abc.ABC", true),
                ("Repository[T]", false),
            ]
        );
    }

    #[rstest::rstest]
    #[case::init_and_method(
        "class C:\n    def __init__(self):\n        self.x: int = 1\n    async def reset(self):\n        if True:\n            self.y: str",
        &[("C", "__init__", "x", "int"), ("C", "reset", "y", "str")]
    )]
    #[case::private_collected_unannotated_skipped(
        "class C:\n    def __init__(self):\n        self._p: int = 1\n        self.pub = 2",
        &[("C", "__init__", "_p", "int")]
    )]
    #[case::staticmethod_classmethod_and_nested_func_skipped(
        "class C:\n    @staticmethod\n    def sm(self):\n        self.a: int = 1\n    @classmethod\n    def cm(cls):\n        cls.b: int = 2\n    def run(self):\n        def inner(self):\n            self.c: int = 3",
        &[]
    )]
    fn test_collect_instance_attribute_annotations(
        #[case] source: &str,
        #[case] expected: &[(&str, &str, &str, &str)],
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let actual: Vec<(String, String, String, String)> =
            collect_instance_attribute_annotations(&file)
                .into_iter()
                .map(|attribute| {
                    (
                        attribute.class_name,
                        attribute.method_name,
                        attribute.name,
                        attribute.annotation.text().into_owned(),
                    )
                })
                .collect();
        let expected: Vec<(String, String, String, String)> = expected
            .iter()
            .map(|(class_name, method_name, name, annotation)| {
                (
                    (*class_name).to_string(),
                    (*method_name).to_string(),
                    (*name).to_string(),
                    (*annotation).to_string(),
                )
            })
            .collect();
        assert_eq!(actual, expected);
    }

    #[rstest::rstest]
    #[case::bare_final(
        "class C:\n    def __init__(self):\n        self.a: Final = 1",
        (true, false)
    )]
    #[case::qualified_bare_final(
        "class C:\n    def __init__(self):\n        self.a: typing.Final = 1",
        (true, false)
    )]
    #[case::annotated_bare_final(
        "class C:\n    def __init__(self):\n        self.a: Annotated[Final, 'm'] = 1",
        (true, false)
    )]
    #[case::parameterized_final(
        "class C:\n    def __init__(self):\n        self.a: Final[int] = 1",
        (false, false)
    )]
    #[case::dataclass(
        "@dataclass\nclass C:\n    def __post_init__(self):\n        self.a: int = 1",
        (false, true)
    )]
    #[case::pydantic_model(
        "class C(BaseModel):\n    def model_post_init(self, context):\n        self.a: int = 1",
        (false, true)
    )]
    fn test_instance_attribute_annotation_facts(
        #[case] source: &str,
        #[case] expected: (bool, bool),
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let actual: Vec<(bool, bool)> = collect_instance_attribute_annotations(&file)
            .iter()
            .map(|attribute| {
                (
                    attribute.is_bare_final(),
                    attribute.is_in_field_synthesizing_class,
                )
            })
            .collect();
        assert_eq!(actual, [expected]);
    }

    #[rstest::rstest]
    #[case::simple_union("def f() -> str | int: pass", false, &["str", "int"])]
    #[case::optional_shorthand("def f() -> Optional[list[int]]: pass", true, &["list[int]"])]
    #[case::pep604_none("def f() -> Sequence[str] | None: pass", true, &["Sequence[str]"])]
    #[case::none_first("def f() -> None | set[int]: pass", true, &["set[int]"])]
    #[case::multiple_branches_with_none("def f() -> list[int] | set[str] | None: pass", true, &["list[int]", "set[str]"])]
    #[case::awaitable_coroutine_envelopes(
        "def f() -> Awaitable[Coroutine[Any, Any, Mapping[str, int] | None]]: pass",
        true,
        &["Mapping[str, int]"]
    )]
    #[case::annotated_wrapper("def f() -> Annotated[tuple[int, ...] | None, 'meta']: pass", true, &["tuple[int, ...]"])]
    fn test_return_type_union(
        #[case] source: &str,
        #[case] expected_has_none: bool,
        #[case] expected_branches: &[&str],
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let signatures = extract_function_signatures(&file);
        let return_type_node = signatures[0]
            .return_type_node
            .as_ref()
            .expect("function should have return annotation");
        let union = return_type_union(return_type_node);
        assert_eq!(union.has_none, expected_has_none);
        let branches: Vec<_> = union
            .branches
            .iter()
            .map(|branch| branch.node.text())
            .collect();
        assert_eq!(branches, expected_branches);
    }

    #[test]
    fn test_return_type_branch_generic_type() {
        let file = ParsedFile::new("def f() -> tuple[int, ...]: pass", Language::Python);
        let signatures = extract_function_signatures(&file);
        let return_type_node = signatures[0].return_type_node.as_ref().unwrap();
        let union = return_type_union(return_type_node);
        let branch = &union.branches[0];
        let collection = branch.collection.as_ref().expect("collection type");
        assert_eq!(collection.name, "tuple");
        let args = branch.type_arguments.as_ref().expect("generic type");
        assert_eq!(args.len(), 2);
        assert_eq!(args[0].text(), "int");
        assert_eq!(args[1].text(), "...");
    }

    #[rstest::rstest]
    #[case::simple("Order {order_id} filled", Some(&["order_id"][..]))]
    #[case::compound_attribute("Order {order.id} filled", Some(&["order"][..]))]
    #[case::compound_subscript("Order {order[id]} filled", Some(&["order"][..]))]
    #[case::conversion_and_spec("Order {order_id!r} {amount:.2f}", Some(&["order_id", "amount"][..]))]
    #[case::positional_empty_and_numbered("Order {} and {0} and {0.id} and {1[key]}", Some(&[][..]))]
    #[case::escaped_double_braces("Literal {{order_id}} value {}", Some(&[][..]))]
    #[case::triple_braces_captures_inner("Literal {{{order_id}}}", Some(&["order_id"][..]))]
    #[case::nested_format_spec("Value {:>{width}}", Some(&["width"][..]))]
    #[case::non_identifier_braces("Payload {\"order_id\": 1} and {a, b}", Some(&[][..]))]
    #[case::unclosed_opening_brace("Malformed {order_id in input", None)]
    #[case::unmatched_closing_brace("Malformed {order_id} stray }", None)]
    fn test_named_format_field_roots(#[case] message: &str, #[case] expected: Option<&[&str]>) {
        let expected = expected.map(|roots| roots.iter().map(|root| (*root).to_owned()).collect());
        assert_eq!(named_format_field_roots(message), expected);
    }

    #[test]
    fn test_collect_logger_calls_reads_callee_message_and_arguments() {
        let source = indoc::indoc! {r#"
            logger.info("Order {order_id} filled", order_id)
            self.log.error("Peer " "{peer.id} failed", **extra)
            logging.log(20, f"User {user_id}", user_id=1)
            print("not a logger", value)
        "#};
        let file = ParsedFile::new(source, Language::Python);
        let calls: Vec<_> = collect_logger_calls(&file)
            .into_iter()
            .map(|call| {
                let mut keyword_names: Vec<String> = call.keyword_names.into_iter().collect();
                keyword_names.sort();
                (
                    call.callee,
                    call.message,
                    call.has_trailing_positional_args,
                    call.has_keyword_splat,
                    keyword_names,
                )
            })
            .collect();
        assert_eq!(
            calls,
            vec![
                (
                    "logger.info".to_owned(),
                    Some("Order {order_id} filled".to_owned()),
                    true,
                    false,
                    vec![],
                ),
                (
                    "self.log.error".to_owned(),
                    Some("Peer {peer.id} failed".to_owned()),
                    false,
                    true,
                    vec![],
                ),
                (
                    "logging.log".to_owned(),
                    None,
                    false,
                    false,
                    vec!["user_id".to_owned()]
                ),
            ]
        );
    }

    #[test]
    fn test_collect_type_method_scopes_python() {
        use crate::code_lint::ast::MethodVisibility::{Private, Public};

        let source = indoc::indoc! {r"
            class Service:
                @overload
                def __init__(self, port: int) -> None: ...
                @overload
                def __init__(self, port: str) -> None: ...
                def __init__(self, port: int | str) -> None:
                    pass

                @property
                def _token(self) -> str:
                    return 'x'

                @_token.setter
                def _token(self, value: str) -> None:
                    pass

                def execute(self) -> None:
                    pass

                def __repr__(self) -> str:
                    return 'Service'

                def __secret(self) -> None:
                    pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let scopes = collect_type_method_scopes(&file);
        let summary: Vec<_> = scopes
            .iter()
            .map(|scope| {
                let methods: Vec<_> = scope
                    .methods
                    .iter()
                    .map(|method| {
                        (
                            method.name.as_str(),
                            method.visibility,
                            method.is_constructor,
                        )
                    })
                    .collect();
                (scope.type_name.as_str(), methods)
            })
            .collect();
        assert_eq!(
            summary,
            vec![(
                "Service",
                vec![
                    ("__init__", Public, true),
                    ("_token", Private, false),
                    ("execute", Public, false),
                    ("__repr__", Public, false),
                    ("__secret", Private, false),
                ],
            )]
        );
    }

    #[test]
    fn test_collect_fields_after_methods_and_statements_after_main_guard() {
        let source = indoc::indoc! {r#"
            class Config:
                host: str

                def reset(self) -> None:
                    pass

                __repr__ = reset
                retries: int = 3

            if __name__ == "__main__":
                pass

            EXTRA = 1
        "#};
        let file = ParsedFile::new(source, Language::Python);
        let fields: Vec<(String, String, String)> = collect_fields_after_methods(&file)
            .into_iter()
            .map(|field| (field.class_name, field.name, field.node.text().into_owned()))
            .collect();
        let trailing: Vec<String> = collect_statements_after_main_guard(&file)
            .into_iter()
            .map(|node| node.text().into_owned())
            .collect();
        assert_eq!(
            (fields, trailing),
            (
                vec![(
                    "Config".to_owned(),
                    "retries".to_owned(),
                    "retries: int = 3".to_owned(),
                )],
                vec!["EXTRA = 1".to_owned()],
            )
        );
    }

    /// `(style, preceding_text, literals, [(field, conversion, format_spec)])`.
    type FormatStringSummary = (
        PythonFormatStyle,
        String,
        Vec<String>,
        Vec<(String, Option<String>, Option<String>)>,
    );

    fn summarize_format_strings(python_code: &str) -> Vec<FormatStringSummary> {
        let file = ParsedFile::new(python_code, Language::Python);
        collect_format_strings(&file)
            .into_iter()
            .map(|format_string| {
                let placeholders = format_string
                    .placeholders
                    .into_iter()
                    .map(|placeholder| {
                        (
                            placeholder.field,
                            placeholder.conversion,
                            placeholder.format_spec,
                        )
                    })
                    .collect();
                (
                    format_string.style,
                    format_string.preceding_text,
                    format_string.literals,
                    placeholders,
                )
            })
            .collect()
    }

    fn owned(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|text| (*text).to_owned()).collect()
    }

    fn field(
        name: &str,
        conversion: Option<&str>,
        format_spec: Option<&str>,
    ) -> (String, Option<String>, Option<String>) {
        (
            name.to_owned(),
            conversion.map(str::to_owned),
            format_spec.map(str::to_owned),
        )
    }

    #[test]
    fn test_collect_format_strings_splits_fstrings() {
        assert_eq!(
            summarize_format_strings(r#"message = f"Copied '{ source !r:>8}' {count=}""#),
            vec![(
                PythonFormatStyle::FString,
                String::new(),
                owned(&["Copied '", "' ", ""]),
                vec![
                    field("source", Some("r"), Some(">8")),
                    field("count", None, None)
                ],
            )]
        );
    }

    #[test]
    fn test_collect_format_strings_splits_str_format_templates() {
        assert_eq!(
            summarize_format_strings(
                r#"message = "Copied '{}' to '{target.name!s}' {{raw}}".format(a, target=b)"#
            ),
            vec![(
                PythonFormatStyle::StrFormat,
                String::new(),
                owned(&["Copied '", "' to '", "' {{raw}}"]),
                vec![field("", None, None), field("target.name", Some("s"), None)],
            )]
        );
    }

    #[rstest::rstest]
    #[case::percent_operator(r#"message = "Got %s, %(key)5.2f and 100%%" % values"#)]
    #[case::logger_with_arguments(r#"logger.info("Got %s, %(key)5.2f and 100%%", values)"#)]
    fn test_collect_format_strings_splits_printf_templates(#[case] python_code: &str) {
        assert_eq!(
            summarize_format_strings(python_code),
            vec![(
                PythonFormatStyle::Printf,
                String::new(),
                owned(&["Got ", ", ", " and 100%%"]),
                vec![
                    field("", Some("s"), None),
                    field("key", Some("f"), Some("5.2"))
                ],
            )]
        );
    }

    #[test]
    fn test_collect_format_strings_reads_concatenated_prefix() {
        let summaries = summarize_format_strings("message = (\"Load \" f\"'{name}'\")");
        let (style, preceding_text, ..) = &summaries[0];
        assert_eq!(
            (summaries.len(), *style, preceding_text.as_str()),
            (1, PythonFormatStyle::FString, "Load ")
        );
    }

    #[test]
    fn test_collect_format_strings_skips_unformatted_raw_and_byte_strings() {
        let source = indoc::indoc! {r#"
            plain = "Invalid '{name}' or '%s'"
            raw = r"Invalid '{name}'".format(name=value)
            byte = b"Invalid '%s'" % value
            no_arguments = logger.info("Literal '%s'")
        "#};
        assert_eq!(summarize_format_strings(source), vec![]);
    }
}
