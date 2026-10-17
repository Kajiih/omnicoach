//! AST helper predicates and structural extractors for Python.

mod annotations;
mod classes;
mod complexity;
mod docstrings;
mod format_strings;
mod functions;
mod literals;
mod logging;
mod parameter_usage;
mod positional_reads;
mod returns;
mod scopes;
mod strings;

pub use self::annotations::{
    AnnotationTraversalDepth, CollectionKind, CollectionShape, PythonCollectionType,
    PythonReturnTypeBranch, PythonReturnTypeUnion, collect_collection_types, collection_display,
    collection_type, generator_yield_type, return_type_union,
};
pub use self::classes::{
    PythonAnnotatedAttribute, PythonAttributeDefinition, PythonBaseClass, PythonClassInfo,
    PythonFieldAfterMethod, PythonInheritedDocstring, PythonInstanceAttributeAnnotation,
    collect_class_attributes, collect_fields_after_methods, collect_instance_attribute_annotations,
    extract_classes, inherited_method_docstring, is_fake_class_name, local_ancestors,
    local_base_classes,
};
pub(super) use self::classes::{collect_declaration_scopes, collect_type_method_scopes};
pub use self::complexity::PythonComplexityMetric;
pub use self::docstrings::{
    DocstringEntry, DocstringLine, DocstringSection, DocstringSectionKind, DocstringTypedLine,
    PythonDocstring, normalize_docstring_text, parse_google_sections,
};
pub use self::format_strings::{
    PythonFormatPlaceholder, PythonFormatString, PythonFormatStyle, collect_format_strings,
    extract_valid_field_root, named_format_field_roots,
};
pub use self::functions::{
    PythonFunctionSignature, PythonParameterInfo, PythonParameterKind, extract_function_signatures,
    find_nested_classes, find_nested_functions,
};
pub(super) use self::literals::collect_literal_occurrences;
pub use self::logging::{PythonLoggerCall, collect_logger_calls};
pub use self::parameter_usage::{
    ParameterCollectionCapability, ParameterUsage, summarize_parameter_usages,
};
pub(super) use self::positional_reads::collect_positional_reads;
pub use self::returns::PythonBodyResults;
pub(super) use self::returns::collect_function_returns;
pub(super) use self::scopes::collect_bindings;
pub(super) use self::strings::find_unwrapped_multiline_strings;

use self::annotations::{has_final_annotation_expr, is_bare_final_annotation_expr};
use self::classes::is_protocol_or_abc_class;
use self::functions::{
    direct_function_definitions, has_override_decorator, method_receiver_name_ast,
};
use self::literals::is_constant_name;
use self::logging::extract_logger_call;
use self::strings::{TemplateParts, fstring_segments_and_interpolations, static_string_text};
use crate::code_lint::ast::{
    AstNode, EnclosingFunction, ParsedFile, QualifiedName, ResolvedName, TestFunctionSetup,
    normalize_statement_text, resolve_name, span_from_ruff_range, with_innermost_function,
};
use crate::diagnostic::SourceSpan;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Decorator, ExceptHandler, Expr, ModModule, Stmt, StmtFunctionDef};
use ruff_text_size::Ranged as _;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const PYTEST_RAISES: &str = "raises";
const ENVIRON_NAME: &str = "environ";
const ISINSTANCE_NAME: &str = "isinstance";

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

/// Structured metadata for a Python decorator.
#[derive(Clone)]
pub(super) struct DecoratorInfo {
    /// Full path string as written (e.g. `"dataclasses.dataclass"`, `"pytest.mark.parametrize"`).
    pub(super) path: String,
    /// Terminal identifier as written (e.g. `"dataclass"`, `"parametrize"`).
    pub(super) terminal_name: String,
    /// Canonical path resolved through [`resolve_name`], or `None` when the first segment is a
    /// module-level definition in the file.
    resolved_path: Option<String>,
    /// Keyword argument names if invoked as a call.
    keyword_args: Vec<String>,
}

impl DecoratorInfo {
    /// Returns true if a keyword argument with `key` was explicitly passed.
    #[must_use]
    pub(super) fn has_arg(&self, key: &str) -> bool {
        self.keyword_args.iter().any(|arg| arg == key)
    }

    /// Returns true if the import-resolved decorator path equals one of `candidates`,
    /// excluding decorators whose first segment is defined in the file.
    #[must_use]
    pub(super) fn matches_any(&self, candidates: &[&str]) -> bool {
        self.resolved_path
            .as_deref()
            .is_some_and(|path| candidates.contains(&path))
    }

    /// Returns true if this decorator is an attribute access (`@<receiver>.<attr>`) whose
    /// attribute name is in `attrs` (e.g. `@prop.setter`, `@func.register`).
    #[must_use]
    pub(super) fn is_attribute_named(&self, attrs: &[&str]) -> bool {
        self.path.contains('.') && attrs.contains(&self.terminal_name.as_str())
    }
}

/// Converts a slice of `ruff_python_ast::Decorator` nodes into `DecoratorInfo` values.
pub(super) fn extract_decorators_from_slice(
    decorators: &[Decorator],
    file: &ParsedFile,
) -> Vec<DecoratorInfo> {
    let mut out = Vec::with_capacity(decorators.len());
    for decorator in decorators {
        let (target_expr, keyword_args) = if let Expr::Call(call) = &decorator.expression {
            let kwargs = call
                .arguments
                .keywords
                .iter()
                .filter_map(|kw| Some(kw.arg.as_ref()?.id.to_string()))
                .collect();
            (call.func.as_ref(), kwargs)
        } else {
            (&decorator.expression, Vec::new())
        };

        let QualifiedName { path, terminal } =
            resolve_path_and_terminal_expr(target_expr, &file.source);
        let resolved_path =
            resolved_path_and_terminal_expr(target_expr, file).map(|name| name.path);
        out.push(DecoratorInfo {
            path,
            terminal_name: terminal,
            resolved_path,
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

/// A top-level Python statement appearing after the `if __name__ == "__main__":` guard block.
pub struct PythonStatementAfterMainGuard<'a> {
    /// AST node of the trailing statement.
    pub node: AstNode<'a>,
    /// 1-based starting line of the `if __name__ == "__main__":` guard block.
    pub guard_line: usize,
}

/// Collects top-level module statements that appear after an `if __name__ == "__main__":`
/// guard block.
#[must_use]
pub fn collect_statements_after_main_guard(
    file: &ParsedFile,
) -> Vec<PythonStatementAfterMainGuard<'_>> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let body = &parsed.syntax().body;
    let Some(guard_index) = body.iter().position(is_main_guard_statement) else {
        return Vec::new();
    };
    let guard_line =
        AstNode::from_span(file, span_from_ruff_range(body[guard_index].range())).start_line();
    body[guard_index + 1..]
        .iter()
        .filter(|statement| !is_main_guard_statement(statement))
        .map(|statement| PythonStatementAfterMainGuard {
            node: AstNode::from_span(file, span_from_ruff_range(statement.range())),
            guard_line,
        })
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
    /// True if the condition compares two or more multi-element collection literals (`tuple`,
    /// `list`, `set`, or `dict`).
    pub has_composite_collection_comparison: bool,
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
                    has_composite_collection_comparison: matches!(
                        test,
                        Expr::Compare(comparison)
                            if comparison
                                .operands
                                .iter()
                                .filter(|operand| is_multi_element_collection_expr(operand))
                                .count()
                                >= 2
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

/// Returns true if `expr` is a Python `tuple`, `list`, `set`, or `dict` literal with `>= 2` elements.
const fn is_multi_element_collection_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Tuple(tuple) => tuple.elts.len() >= 2,
        Expr::List(list) => list.elts.len() >= 2,
        Expr::Set(set) => set.elts.len() >= 2,
        Expr::Dict(dict) => dict.items.len() >= 2,
        _ => false,
    }
}

/// Collects all outermost Python test functions along with their identifier node, name, and assertion count.
#[must_use]
pub(super) fn collect_test_function_assertion_counts(
    file: &ParsedFile,
) -> Vec<(AstNode<'_>, String, usize)> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let module_body = &parsed.syntax().body;
    let assertion_helpers = collect_python_assertion_helpers(module_body);
    collect_outer_python_test_functions(module_body)
        .into_iter()
        .map(|func_def| {
            let count = count_python_assertions_in_slice(
                &func_def.body,
                &func_def.body,
                &assertion_helpers,
            );
            (
                AstNode::from_span(file, span_from_ruff_range(func_def.name.range)),
                func_def.name.to_string(),
                count,
            )
        })
        .collect()
}

/// Collects control-flow and conditional constructs directly inside outermost Python test functions.
#[must_use]
pub(super) fn collect_test_logic_constructs(
    file: &ParsedFile,
) -> Vec<(AstNode<'_>, String, &'static str)> {
    struct TestLogicCollector<'a> {
        file: &'a ParsedFile,
        func_name: &'a str,
        out: Vec<(AstNode<'a>, String, &'static str)>,
    }

    impl<'a> SourceOrderVisitor<'a> for TestLogicCollector<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            let construct = match statement {
                Stmt::FunctionDef(_) | Stmt::ClassDef(_) => return,
                Stmt::If(_) => Some("an `if` statement"),
                Stmt::Match(_) => Some("a `match` statement"),
                Stmt::For(_) => Some("a `for` loop"),
                Stmt::While(_) => Some("a `while` loop"),
                Stmt::Try(_) => Some("a `try` statement"),
                _ => None,
            };
            if let Some(description) = construct {
                self.out.push((
                    AstNode::from_span(self.file, span_from_ruff_range(statement.range())),
                    self.func_name.to_string(),
                    description,
                ));
                return;
            }
            walk_stmt(self, statement);
        }

        fn visit_expr(&mut self, expr: &'a Expr) {
            const FILTERED_COMPREHENSION: &str = "a filtered comprehension";

            let construct = match expr {
                Expr::Lambda(_) => return,
                Expr::If(_) => Some("a conditional `if`-`else` expression"),
                Expr::ListComp(comp)
                    if comp
                        .generators
                        .iter()
                        .any(|generator| !generator.ifs.is_empty()) =>
                {
                    Some(FILTERED_COMPREHENSION)
                }
                Expr::SetComp(comp)
                    if comp
                        .generators
                        .iter()
                        .any(|generator| !generator.ifs.is_empty()) =>
                {
                    Some(FILTERED_COMPREHENSION)
                }
                Expr::DictComp(comp)
                    if comp
                        .generators
                        .iter()
                        .any(|generator| !generator.ifs.is_empty()) =>
                {
                    Some(FILTERED_COMPREHENSION)
                }
                Expr::Generator(comp)
                    if comp
                        .generators
                        .iter()
                        .any(|generator| !generator.ifs.is_empty()) =>
                {
                    Some(FILTERED_COMPREHENSION)
                }
                _ => None,
            };
            if let Some(description) = construct {
                self.out.push((
                    AstNode::from_span(self.file, span_from_ruff_range(expr.range())),
                    self.func_name.to_string(),
                    description,
                ));
                return;
            }
            walk_expr(self, expr);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for func_def in collect_outer_python_test_functions(&parsed.syntax().body) {
        let mut collector = TestLogicCollector {
            file,
            func_name: func_def.name.as_str(),
            out: Vec::new(),
        };
        collector.visit_body(&func_def.body);
        out.extend(collector.out);
    }
    out
}

/// Collects outermost Python test functions that interleave non-assertion statements between
/// behavioral assertions.
#[must_use]
pub(super) fn collect_multi_phase_test_functions(file: &ParsedFile) -> Vec<(AstNode<'_>, String)> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let module_body = &parsed.syntax().body;
    let assertion_helpers = collect_python_assertion_helpers(module_body);
    collect_outer_python_test_functions(module_body)
        .into_iter()
        .filter(|func_def| is_multi_phase_python_test_body(&func_def.body, &assertion_helpers))
        .map(|func_def| {
            (
                AstNode::from_span(file, span_from_ruff_range(func_def.name.range)),
                func_def.name.to_string(),
            )
        })
        .collect()
}

/// Returns true if `body` interleaves a non-assertion statement between two behavioral assertions.
fn is_multi_phase_python_test_body(body: &[Stmt], assertion_helpers: &HashSet<String>) -> bool {
    let mut seen_assertion = false;
    let mut seen_action_after_assertion = false;
    for statement in body {
        if let Stmt::Assert(assert_statement) = statement
            && is_narrowing_guard_assert(assert_statement, body)
        {
            continue;
        }
        if count_python_assertions_in_slice(
            std::slice::from_ref(statement),
            body,
            assertion_helpers,
        ) > 0
        {
            if seen_action_after_assertion {
                return true;
            }
            seen_assertion = true;
        } else if seen_assertion && !is_python_inspection_binding(statement) {
            seen_action_after_assertion = true;
        }
    }
    false
}

/// Returns true if `statement` binds local names to a call-free projection of existing values or
/// a `pytest` capture readout (`capsys.readouterr()` / `capfd.readouterr()`).
fn is_python_inspection_binding(statement: &Stmt) -> bool {
    match statement {
        Stmt::Assign(assign) => {
            !assign.targets.is_empty()
                && assign.targets.iter().all(is_local_binding_target)
                && is_python_inspection_expr(&assign.value)
        }
        Stmt::AnnAssign(ann_assign) => {
            is_local_binding_target(&ann_assign.target)
                && ann_assign
                    .value
                    .as_deref()
                    .is_some_and(is_python_inspection_expr)
        }
        _ => false,
    }
}

fn is_local_binding_target(target: &Expr) -> bool {
    match target {
        Expr::Name(_) => true,
        Expr::Starred(starred) => is_local_binding_target(&starred.value),
        Expr::Tuple(tuple) => {
            !tuple.elts.is_empty() && tuple.elts.iter().all(is_local_binding_target)
        }
        Expr::List(list) => !list.elts.is_empty() && list.elts.iter().all(is_local_binding_target),
        _ => false,
    }
}

fn is_python_inspection_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Name(_) => true,
        Expr::Attribute(attr) => is_python_inspection_expr(&attr.value),
        Expr::Subscript(subscript) => {
            is_python_inspection_expr(&subscript.value)
                && !python_expr_contains_call(&subscript.slice)
        }
        Expr::Tuple(tuple) => {
            !tuple.elts.is_empty() && tuple.elts.iter().all(is_python_inspection_expr)
        }
        Expr::List(list) => {
            !list.elts.is_empty() && list.elts.iter().all(is_python_inspection_expr)
        }
        Expr::Call(call) => is_pytest_readouterr_call(call),
        _ => false,
    }
}

fn python_expr_contains_call(expr: &Expr) -> bool {
    struct CallFinder {
        found: bool,
    }

    impl<'a> SourceOrderVisitor<'a> for CallFinder {
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

    let mut finder = CallFinder { found: false };
    finder.visit_expr(expr);
    finder.found
}

fn is_pytest_readouterr_call(call: &ruff_python_ast::ExprCall) -> bool {
    const PYTEST_CAPTURE_FIXTURES: &[&str] = &["capsys", "capfd", "capsysbinary", "capfdbinary"];

    call.arguments.is_empty()
        && matches!(
            call.func.as_ref(),
            Expr::Attribute(attr)
                if attr.attr.as_str() == "readouterr"
                    && matches!(
                        attr.value.as_ref(),
                        Expr::Name(receiver)
                            if PYTEST_CAPTURE_FIXTURES.contains(&receiver.id.as_str())
                    )
        )
}

/// Collects Python test functions grouped by enclosing module or class scope (segmented on
/// duplicate function names) along with their leading non-assertion setup statements.
#[must_use]
pub(super) fn collect_test_setup_scopes(file: &ParsedFile) -> Vec<Vec<TestFunctionSetup<'_>>> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let module_body = &parsed.syntax().body;
    let assertion_helpers = collect_python_assertion_helpers(module_body);
    let mut out = Vec::new();
    collect_python_test_setup_scope(module_body, file, &assertion_helpers, &mut out);
    out
}

fn collect_python_test_setup_scope<'a>(
    statements: &[Stmt],
    file: &'a ParsedFile,
    assertion_helpers: &HashSet<String>,
    out: &mut Vec<Vec<TestFunctionSetup<'a>>>,
) {
    let mut current_segment = Vec::new();
    let mut seen_names = HashSet::new();
    for statement in statements {
        let Stmt::FunctionDef(func_def) = statement else {
            continue;
        };
        if !is_test_function_def(func_def) {
            continue;
        }
        let name = func_def.name.to_string();
        if !seen_names.insert(name.clone()) {
            out.push(std::mem::take(&mut current_segment));
            seen_names.clear();
            seen_names.insert(name.clone());
        }
        current_segment.push(TestFunctionSetup {
            name_node: AstNode::from_span(file, span_from_ruff_range(func_def.name.range)),
            name,
            leading_setup_statements: extract_python_leading_setup_statements(
                func_def,
                file,
                assertion_helpers,
            ),
        });
    }
    if !current_segment.is_empty() {
        out.push(current_segment);
    }
    for statement in statements {
        if let Stmt::ClassDef(class_def) = statement {
            collect_python_test_setup_scope(&class_def.body, file, assertion_helpers, out);
        }
    }
}

fn extract_python_leading_setup_statements(
    func_def: &StmtFunctionDef,
    file: &ParsedFile,
    assertion_helpers: &HashSet<String>,
) -> Vec<String> {
    let statements = if docstrings::docstring_of(&func_def.body, file).is_some() {
        &func_def.body[1..]
    } else {
        &func_def.body[..]
    };
    statements
        .iter()
        .filter(|statement| !matches!(statement, Stmt::FunctionDef(_) | Stmt::ClassDef(_)))
        .take_while(|statement| {
            count_python_assertions_in_slice(
                std::slice::from_ref(*statement),
                &func_def.body,
                assertion_helpers,
            ) == 0
        })
        .map(|statement| {
            let span = span_from_ruff_range(statement.range());
            normalize_statement_text(&file.source[span.start..span.end])
        })
        .collect()
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
pub(super) fn resolve_path_and_terminal_expr(expr: &Expr, source: &str) -> QualifiedName {
    let span = span_from_ruff_range(expr.range());
    let path = source[span.start..span.end].to_string();
    let terminal = if let Expr::Attribute(attr) = expr {
        attr.attr.to_string()
    } else {
        path.rsplit('.').next().unwrap_or("").to_string()
    };
    QualifiedName { path, terminal }
}

/// The name that `expr` names, resolved through the imports of `file`
/// (`t.List` after `import typing as t` is `typing.List` with terminal `List`), or `None`
/// if it names a definition of `file` (see [`resolve_name`]).
pub(super) fn resolved_path_and_terminal_expr(
    expr: &Expr,
    file: &ParsedFile,
) -> Option<QualifiedName> {
    let name = resolve_path_and_terminal_expr(expr, &file.source);
    match resolve_name(file, &name.path) {
        ResolvedName::Imported(resolved) => {
            let terminal = resolved
                .rsplit_once('.')
                .map_or(resolved.as_str(), |(_, terminal)| terminal)
                .to_owned();
            Some(QualifiedName {
                path: resolved,
                terminal,
            })
        }
        ResolvedName::Local => None,
        ResolvedName::Unbound => Some(name),
    }
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
    let terminal = resolve_path_and_terminal_expr(&call.func, source).terminal;
    (!terminal.is_empty()).then_some(terminal)
}

/// Collects all outermost Python `StmtFunctionDef` nodes in `body` that are test functions.
fn collect_outer_python_test_functions(body: &[Stmt]) -> Vec<&StmtFunctionDef> {
    struct OuterTestFinder<'a> {
        out: Vec<&'a StmtFunctionDef>,
    }

    impl<'a> SourceOrderVisitor<'a> for OuterTestFinder<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::FunctionDef(func_def) = statement {
                if is_test_function_def(func_def) {
                    self.out.push(func_def);
                }
                return;
            }
            walk_stmt(self, statement);
        }
    }

    let mut finder = OuterTestFinder { out: Vec::new() };
    finder.visit_body(body);
    finder.out
}

/// Collects names of non-test helper functions and methods in `module_body` that perform
/// behavioral assertions (either directly containing `pytest.raises` / `raises` / `pytest.warns`,
/// or returning `None` while executing at least one behavioral assertion transitively).
fn collect_python_assertion_helpers(module_body: &[Stmt]) -> HashSet<String> {
    let candidates = collect_non_test_python_functions(module_body);
    let mut helpers = HashSet::new();
    loop {
        let mut added = false;
        for func_def in &candidates {
            let name = func_def.name.as_str();
            if helpers.contains(name) {
                continue;
            }
            let is_assertion_helper = python_body_has_exception_assertion(&func_def.body)
                || (is_unit_python_function(func_def)
                    && count_python_assertions_in_slice(&func_def.body, &func_def.body, &helpers)
                        > 0);
            if is_assertion_helper {
                helpers.insert(name.to_owned());
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    helpers
}

/// Collects all non-test `StmtFunctionDef` nodes in `body` (including methods and nested helpers).
fn collect_non_test_python_functions(body: &[Stmt]) -> Vec<&StmtFunctionDef> {
    struct NonTestFunctionCollector<'a> {
        out: Vec<&'a StmtFunctionDef>,
    }

    impl<'a> SourceOrderVisitor<'a> for NonTestFunctionCollector<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::FunctionDef(func_def) = statement
                && !is_test_function_def(func_def)
            {
                self.out.push(func_def);
            }
            walk_stmt(self, statement);
        }
    }

    let mut collector = NonTestFunctionCollector { out: Vec::new() };
    collector.visit_body(body);
    collector.out
}

/// Returns true if a Python `StmtFunctionDef` is a test function (`test` or `test_*`).
fn is_test_function_def(func_def: &StmtFunctionDef) -> bool {
    let func_name = func_def.name.as_str();
    func_name == "test" || func_name.starts_with("test_")
}

/// Returns true if `func_def` has no non-`None` return annotation and its body neither returns a
/// value nor yields.
fn is_unit_python_function(func_def: &StmtFunctionDef) -> bool {
    if !func_def
        .returns
        .as_deref()
        .is_none_or(|annotation| matches!(annotation, Expr::NoneLiteral(_)))
    {
        return false;
    }
    let results = returns::collect_body_results(&func_def.body);
    !results.returns_value && !results.yields
}

/// Returns true if `body` (excluding nested functions and classes) contains a call to
/// `pytest.raises(...)`, `raises(...)`, or `pytest.warns(...)`.
fn python_body_has_exception_assertion(body: &[Stmt]) -> bool {
    struct ExceptionAssertionFinder {
        found: bool,
    }

    impl<'a> SourceOrderVisitor<'a> for ExceptionAssertionFinder {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if self.found || matches!(statement, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
                return;
            }
            walk_stmt(self, statement);
        }

        fn visit_expr(&mut self, expr: &'a Expr) {
            if self.found {
                return;
            }
            if let Expr::Call(call) = expr
                && is_python_exception_assertion_call(call)
            {
                self.found = true;
                return;
            }
            walk_expr(self, expr);
        }
    }

    let mut finder = ExceptionAssertionFinder { found: false };
    finder.visit_body(body);
    finder.found
}

/// Counts behavioral assertion constructs in `statements`, using `full_body` to resolve narrowing
/// guards and `assertion_helpers` to recognize calls to same-file assertion helpers.
fn count_python_assertions_in_slice(
    statements: &[Stmt],
    full_body: &[Stmt],
    assertion_helpers: &HashSet<String>,
) -> usize {
    struct AssertionCounter<'a> {
        full_body: &'a [Stmt],
        assertion_helpers: &'a HashSet<String>,
        count: usize,
    }

    impl<'a> SourceOrderVisitor<'a> for AssertionCounter<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            match statement {
                Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
                Stmt::Assert(assert_statement) => {
                    if !is_narrowing_guard_assert(assert_statement, self.full_body) {
                        self.count += 1;
                        self.visit_expr(&assert_statement.test);
                    }
                }
                _ => walk_stmt(self, statement),
            }
        }

        fn visit_expr(&mut self, expr: &'a Expr) {
            if let Expr::Call(call) = expr
                && is_assertion_call_expr(call, self.assertion_helpers)
            {
                self.count += 1;
                return;
            }
            walk_expr(self, expr);
        }
    }

    let mut counter = AssertionCounter {
        full_body,
        assertion_helpers,
        count: 0,
    };
    counter.visit_body(statements);
    counter.count
}

/// Returns true if `assert_statement` narrows a local variable that is loaded later in `body`.
fn is_narrowing_guard_assert(
    assert_statement: &ruff_python_ast::StmtAssert,
    body: &[Stmt],
) -> bool {
    narrowed_variable_name(assert_statement)
        .is_some_and(|name| body_loads_name_after(body, name, assert_statement.range().end()))
}

/// If `assert_statement` is `assert <name> is not None`, `assert None is not <name>`, or
/// `assert isinstance(<name>, ...)`, returns `<name>`.
fn narrowed_variable_name(assert_statement: &ruff_python_ast::StmtAssert) -> Option<&str> {
    match assert_statement.test.as_ref() {
        Expr::Compare(compare) => {
            let ([ruff_python_ast::CmpOp::IsNot], [left, right]) =
                (&*compare.ops, &*compare.operands)
            else {
                return None;
            };
            match (left, right) {
                (Expr::Name(name), Expr::NoneLiteral(_))
                | (Expr::NoneLiteral(_), Expr::Name(name)) => Some(name.id.as_str()),
                _ => None,
            }
        }
        Expr::Call(call) => {
            let is_isinstance = match call.func.as_ref() {
                Expr::Name(name) => name.id.as_str() == ISINSTANCE_NAME,
                Expr::Attribute(attr) => {
                    attr.attr.as_str() == ISINSTANCE_NAME
                        && matches!(
                            attr.value.as_ref(),
                            Expr::Name(module) if module.id.as_str() == "builtins"
                        )
                }
                _ => false,
            };
            if !is_isinstance
                || call.arguments.args.len() != 2
                || !call.arguments.keywords.is_empty()
            {
                return None;
            }
            match &call.arguments.args[0] {
                Expr::Name(name) => Some(name.id.as_str()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Returns true if `body` (excluding nested functions and classes) loads `target_name` after `offset`.
fn body_loads_name_after(
    body: &[Stmt],
    target_name: &str,
    offset: ruff_text_size::TextSize,
) -> bool {
    struct LaterLoadFinder<'a> {
        target_name: &'a str,
        offset: ruff_text_size::TextSize,
        found: bool,
    }

    impl<'a> SourceOrderVisitor<'a> for LaterLoadFinder<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if self.found || statement.range().end() <= self.offset {
                return;
            }
            if !matches!(statement, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
                walk_stmt(self, statement);
            }
        }

        fn visit_expr(&mut self, expr: &'a Expr) {
            if self.found || expr.range().end() <= self.offset {
                return;
            }
            if let Expr::Name(name) = expr
                && name.ctx.is_load()
                && name.range().start() > self.offset
                && name.id.as_str() == self.target_name
            {
                self.found = true;
                return;
            }
            walk_expr(self, expr);
        }
    }

    let mut finder = LaterLoadFinder {
        target_name,
        offset,
        found: false,
    };
    finder.visit_body(body);
    finder.found
}

/// Returns true if a Python `ExprCall` is a test assertion call
/// (`self.assert*()`, `assert_*()`, `_assert_*()`, `pytest.raises(...)`, `raises(...)`,
/// `pytest.warns(...)`, `self.fail(...)`, or a same-file assertion helper in `assertion_helpers`).
fn is_assertion_call_expr(
    call: &ruff_python_ast::ExprCall,
    assertion_helpers: &HashSet<String>,
) -> bool {
    if is_python_exception_assertion_call(call) {
        return true;
    }
    match call.func.as_ref() {
        Expr::Name(name) => {
            let id = name.id.as_str();
            super::is_assertion_function_name(id) || assertion_helpers.contains(id)
        }
        Expr::Attribute(attr) => {
            let attr_name = attr.attr.as_str();
            if attr_name.starts_with("assert") || super::is_assertion_function_name(attr_name) {
                return true;
            }
            matches!(attr.value.as_ref(), Expr::Name(obj) if obj.id.as_str() == "self")
                && (attr_name == "fail" || assertion_helpers.contains(attr_name))
        }
        _ => false,
    }
}

/// Returns true if `call` is `pytest.raises(...)`, `raises(...)`, or `pytest.warns(...)`.
fn is_python_exception_assertion_call(call: &ruff_python_ast::ExprCall) -> bool {
    match call.func.as_ref() {
        Expr::Name(name) => name.id.as_str() == PYTEST_RAISES,
        Expr::Attribute(attr) => {
            matches!(attr.value.as_ref(), Expr::Name(obj) if obj.id.as_str() == "pytest")
                && matches!(attr.attr.as_str(), PYTEST_RAISES | "warns")
        }
        _ => false,
    }
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
    fn first_function_decorators(file: &ParsedFile) -> Vec<DecoratorInfo> {
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
        assert!(dec0.has_arg("frozen"));
        assert!(dec0.has_arg("slots"));
        assert!(!dec0.has_arg("unknown"));
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
        let dec1 = &decorators[1];

        assert_eq!(dec0.terminal_name, "parametrize");
        assert_eq!(dec0.path, "pytest.mark.parametrize");
        assert_eq!(dec1.terminal_name, "custom");
        assert_eq!(dec1.path, "custom");
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
    fn test_extract_classes_bases_metadata() {
        let source = indoc::indoc! {r"
            class FakeService(abc.ABC, Protocol):
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let classes = extract_classes(&file);
        let cls = &classes[0];
        let base_names: Vec<&str> = cls.bases.iter().map(|base| base.name.as_str()).collect();
        assert_eq!(classes.len(), 1);
        assert_eq!(cls.name, "FakeService");
        assert_eq!(base_names, vec!["abc.ABC", "Protocol"]);
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

        let cls = &classes[0];
        assert_eq!(cls.name, "Config");
        assert_eq!(cls.decorators[0].terminal_name, "dataclass");
        assert!(!cls.is_dataclass_missing_arg("frozen"));
        assert!(cls.is_dataclass_missing_arg("slots"));
    }

    #[test]
    fn test_extract_parameters_metadata() {
        let source = indoc::indoc! {r"
            def handler(self, a: int, b: str = 'hello', *, c: bool, **kwargs):
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let signatures = extract_function_signatures(&file);
        let actual: Vec<(&str, PythonParameterKind, Option<&str>)> = signatures[0]
            .parameters
            .iter()
            .map(|param| (param.name.as_str(), param.kind, param.type_text.as_deref()))
            .collect();

        assert_eq!(
            actual,
            vec![
                ("self", PythonParameterKind::Receiver, None),
                ("a", PythonParameterKind::Positional, Some("int")),
                ("b", PythonParameterKind::Positional, Some("str")),
                ("c", PythonParameterKind::KeywordOnly, Some("bool")),
                ("kwargs", PythonParameterKind::VarKeyword, None),
            ]
        );
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
        assert_eq!(sigs.len(), 1);
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
        assert_eq!(sigs[0].is_exempt_from_signature_rules(), expected);
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
        let branches: Vec<_> = union
            .branches
            .iter()
            .map(|branch| branch.node.text())
            .collect();
        assert_eq!(union.has_none, expected_has_none);
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
        let args = branch.type_arguments.as_ref().expect("generic type");
        assert_eq!(collection.name, "tuple");
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
        let fields: Vec<(
            String,
            String,
            String,
            crate::code_lint::ast::PlacementAnchor,
        )> = collect_fields_after_methods(&file)
            .into_iter()
            .map(|field| {
                (
                    field.class_name,
                    field.name,
                    field.node.text().into_owned(),
                    field.first_method,
                )
            })
            .collect();
        let trailing: Vec<(String, usize)> = collect_statements_after_main_guard(&file)
            .into_iter()
            .map(|item| (item.node.text().into_owned(), item.guard_line))
            .collect();
        assert_eq!(
            fields,
            vec![(
                "Config".to_owned(),
                "retries".to_owned(),
                "retries: int = 3".to_owned(),
                crate::code_lint::ast::PlacementAnchor {
                    name: "reset".to_owned(),
                    line: 4,
                },
            )]
        );
        assert_eq!(trailing, vec![("EXTRA = 1".to_owned(), 10)]);
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
        assert_eq!(summaries.len(), 1);
        assert_eq!(*style, PythonFormatStyle::FString);
        assert_eq!(preceding_text.as_str(), "Load ");
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
