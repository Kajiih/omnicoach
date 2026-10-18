//! Python test-function and assertion extractors.

use super::classes::SELF_RECEIVER;
use super::{docstrings, returns, scopes};
use crate::code_lint::ast::{
    AstCallCandidate, AstNode, EnclosingFunction, ParsedFile, TestFunctionSetup, TestToTestCall,
    collect_call_candidates, is_assertion_function_name, normalize_statement_text,
    span_from_ruff_range, split_at_redefinitions,
};
use crate::diagnostic::SourceSpan;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, Stmt, StmtClassDef, StmtFunctionDef};
use ruff_text_size::Ranged as _;
use std::collections::{HashMap, HashSet};

const PYTEST_RAISES: &str = "raises";
const ISINSTANCE_NAME: &str = "isinstance";

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
pub(in crate::code_lint::ast) fn collect_test_function_assertion_counts(
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
pub(in crate::code_lint::ast) fn collect_test_logic_constructs(
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
pub(in crate::code_lint::ast) fn collect_multi_phase_test_functions(
    file: &ParsedFile,
) -> Vec<(AstNode<'_>, String)> {
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
pub(in crate::code_lint::ast) fn collect_test_setup_scopes(
    file: &ParsedFile,
) -> Vec<Vec<TestFunctionSetup<'_>>> {
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
    let mut setups = Vec::new();
    for statement in statements {
        let Stmt::FunctionDef(func_def) = statement else {
            continue;
        };
        if !is_test_function_def(func_def) {
            continue;
        }
        setups.push(TestFunctionSetup {
            name_node: AstNode::from_span(file, span_from_ruff_range(func_def.name.range)),
            name: func_def.name.to_string(),
            leading_setup_statements: extract_python_leading_setup_statements(
                func_def,
                file,
                assertion_helpers,
            ),
        });
    }
    out.extend(split_at_redefinitions(setups, |setup| &setup.name));
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

/// Collects calls in a Python test file where a function calls a test function defined in the same
/// file.
#[must_use]
pub(in crate::code_lint::ast) fn collect_test_to_test_calls(
    file: &ParsedFile,
) -> Vec<TestToTestCall<'_>> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let module_body = &parsed.syntax().body;
    let function_locals = collect_python_function_locals_by_name_span(module_body, file);
    let mut module_tests = HashSet::new();
    let mut class_tests: HashMap<&str, HashSet<&str>> = HashMap::new();
    for statement in module_body {
        match statement {
            Stmt::FunctionDef(func_def) if is_test_function_def(func_def) => {
                module_tests.insert(func_def.name.as_str());
            }
            Stmt::ClassDef(class_def) if is_test_class_def(class_def) => {
                let methods: HashSet<&str> = class_def
                    .body
                    .iter()
                    .filter_map(|stmt| match stmt {
                        Stmt::FunctionDef(method_def) if is_test_function_def(method_def) => {
                            Some(method_def.name.as_str())
                        }
                        _ => None,
                    })
                    .collect();
                class_tests
                    .entry(class_def.name.as_str())
                    .or_default()
                    .extend(methods);
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for candidate in collect_call_candidates(file) {
        let Some(caller_fn) = candidate.enclosing_functions.first() else {
            continue;
        };
        let Some(target) =
            resolve_called_python_test(&candidate, &module_tests, &class_tests, &function_locals)
        else {
            continue;
        };
        if is_recursive_python_test_call(&candidate, caller_fn, &target) {
            continue;
        }
        out.push(TestToTestCall {
            node: candidate.node,
            caller: caller_fn.name.clone(),
            callee: candidate.callee,
        });
    }
    out
}

/// A Python test function or method target in the same file.
#[derive(PartialEq, Eq)]
enum PythonTestTarget<'a> {
    /// A module-level `def test_*` function.
    Module(&'a str),
    /// A `def test_*` method on a test class (`Test*` or `*TestCase`).
    Class {
        class_name: &'a str,
        method: &'a str,
    },
}

/// Resolves `candidate` to a declared Python test function or test method in the same file, if any.
fn resolve_called_python_test<'a>(
    candidate: &'a AstCallCandidate<'_>,
    module_tests: &HashSet<&'a str>,
    class_tests: &HashMap<&'a str, HashSet<&'a str>>,
    function_locals: &HashMap<SourceSpan, HashSet<String>>,
) -> Option<PythonTestTarget<'a>> {
    if !candidate.callee.contains('.') {
        let callee = candidate.callee.as_str();
        let is_local = candidate.enclosing_functions.iter().any(|func| {
            function_locals
                .get(&func.name_span)
                .is_some_and(|locals| locals.contains(callee))
        });
        (module_tests.contains(callee) && !is_local).then_some(PythonTestTarget::Module(callee))
    } else if let Some(method) = candidate
        .callee
        .strip_prefix("self.")
        .or_else(|| candidate.callee.strip_prefix("cls."))
    {
        let class_name = candidate
            .enclosing_functions
            .iter()
            .find_map(|func| func.enclosing_class.as_deref())?;
        class_tests
            .get(class_name)
            .filter(|methods| methods.contains(method))
            .map(|_| PythonTestTarget::Class { class_name, method })
    } else {
        None
    }
}

/// Returns true if `candidate` is a recursive self-call to the enclosing test function or method.
fn is_recursive_python_test_call(
    candidate: &AstCallCandidate<'_>,
    caller_fn: &EnclosingFunction,
    target: &PythonTestTarget<'_>,
) -> bool {
    match *target {
        PythonTestTarget::Module(callee) => {
            caller_fn.enclosing_class.is_none() && caller_fn.name == callee
        }
        PythonTestTarget::Class { class_name, method } => candidate
            .enclosing_functions
            .iter()
            .any(|func| func.enclosing_class.as_deref() == Some(class_name) && func.name == method),
    }
}

/// Collects local bound names for each `StmtFunctionDef` in `body` indexed by its declared name span.
fn collect_python_function_locals_by_name_span(
    body: &[Stmt],
    file: &ParsedFile,
) -> HashMap<SourceSpan, HashSet<String>> {
    struct FunctionLocalsCollector<'a> {
        file: &'a ParsedFile,
        out: HashMap<SourceSpan, HashSet<String>>,
    }

    impl<'a> SourceOrderVisitor<'a> for FunctionLocalsCollector<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::FunctionDef(func_def) = statement {
                self.out.insert(
                    span_from_ruff_range(func_def.name.range),
                    scopes::collect_local_bound_names(func_def, self.file),
                );
            }
            walk_stmt(self, statement);
        }
    }

    let mut collector = FunctionLocalsCollector {
        file,
        out: HashMap::new(),
    };
    collector.visit_body(body);
    collector.out
}

/// Returns true if a Python `StmtClassDef` is a test class (`Test*` or inheriting `*TestCase`).
fn is_test_class_def(class_def: &StmtClassDef) -> bool {
    class_def.name.as_str().starts_with("Test")
        || class_def.bases().iter().any(|base| {
            let base_name = match base {
                Expr::Name(name) => Some(name.id.as_str()),
                Expr::Attribute(attr) => Some(attr.attr.as_str()),
                _ => None,
            };
            base_name.is_some_and(|name| name.ends_with("TestCase"))
        })
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
            is_assertion_function_name(id) || assertion_helpers.contains(id)
        }
        Expr::Attribute(attr) => {
            let attr_name = attr.attr.as_str();
            if attr_name.starts_with("assert") || is_assertion_function_name(attr_name) {
                return true;
            }
            matches!(attr.value.as_ref(), Expr::Name(obj) if obj.id.as_str() == SELF_RECEIVER)
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
