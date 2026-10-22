//! Shared data structures and language-agnostic algorithms for test-function and assertion analysis.

use super::rust;
use super::tree::{AstNode, ParsedFile};
use crate::diagnostic::Language;
use std::collections::HashSet;

impl ParsedFile {
    /// Returns true if `offset` lies inside a Rust `#[cfg(test)]` or `#[test]` item.
    #[must_use]
    pub(super) fn is_in_rust_inline_test(&self, offset: usize) -> bool {
        self.lang() == Language::Rust
            && rust::collect_inline_test_ranges(self)
                .iter()
                .any(|range| range.contains(&offset))
    }
}

impl AstNode<'_> {
    /// Returns true if this node starts inside a Rust `#[cfg(test)]` or `#[test]` item.
    #[must_use]
    pub(in crate::code_lint) fn is_in_rust_inline_test(&self) -> bool {
        self.file.is_in_rust_inline_test(self.span.start)
    }
}

/// An outermost test function and the number of behavioral assertions in its body.
pub(in crate::code_lint) struct TestAssertionCount<'a> {
    /// Identifier AST node of the test function declaration.
    pub(in crate::code_lint) name_node: AstNode<'a>,
    /// Name of the test function.
    pub(in crate::code_lint) name: String,
    /// Number of behavioral assertions in the test function body.
    pub(in crate::code_lint) count: usize,
}

/// A branching or loop construct directly inside an outermost test function.
pub(in crate::code_lint) struct TestLogicConstruct<'a> {
    /// AST node of the branching or loop construct.
    pub(in crate::code_lint) node: AstNode<'a>,
    /// Name of the enclosing test function.
    pub(in crate::code_lint) test_name: String,
    /// Human-readable description of the construct (e.g. "an `if` statement").
    pub(in crate::code_lint) construct: &'static str,
}

/// An outermost test function that interleaves actions and assertions across multiple phases.
pub(in crate::code_lint) struct MultiPhaseTest<'a> {
    /// Identifier AST node of the test function declaration.
    pub(in crate::code_lint) name_node: AstNode<'a>,
    /// Name of the test function.
    pub(in crate::code_lint) name: String,
}

/// A Python `assert` statement or Rust assertion macro invocation (`assert!`, `assert_eq!`,
/// `assert_ne!`, `debug_assert*!`) with its condition-packing properties.
pub(in crate::code_lint) struct AssertionCheck<'a> {
    /// The `assert` statement or macro invocation node.
    pub(in crate::code_lint) node: AstNode<'a>,
    /// Rust macro terminal name (`"assert"`, `"assert_eq"`, ...), or empty in Python.
    pub(in crate::code_lint) callee: String,
    /// True if the condition has a top-level `and` (Python) or `&&` (Rust `assert!` / `debug_assert!`).
    pub(in crate::code_lint) has_top_level_logical_and: bool,
    /// True if the assertion compares against a tuple/list/array/`vec!` of `>= 2` boolean literals.
    pub(in crate::code_lint) has_boolean_literal_comparison: bool,
    /// True if the assertion compares two or more multi-element collection literals.
    pub(in crate::code_lint) has_composite_collection_comparison: bool,
}

/// A test function and its normalized leading non-assertion setup statements.
pub(in crate::code_lint) struct TestFunctionSetup<'a> {
    /// Identifier AST node of the test function declaration.
    pub(in crate::code_lint) name_node: AstNode<'a>,
    /// Name of the test function.
    pub(in crate::code_lint) name: String,
    /// Normalized source text of each leading non-assertion setup statement, in source order.
    pub(in crate::code_lint) leading_setup_statements: Vec<String>,
}

/// A call inside a test file to another test function defined in the same file.
pub(in crate::code_lint) struct TestToTestCall<'a> {
    /// The call expression node.
    pub(in crate::code_lint) node: AstNode<'a>,
    /// The name of the enclosing function making the call.
    pub(in crate::code_lint) caller: String,
    /// The source callee text as written (`test_login`, `self.test_login`).
    pub(in crate::code_lint) callee: String,
}

/// Classification of a top-level statement in a test body for multi-phase test detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TestStatementRole {
    /// A behavioral assertion statement or call.
    Assertion,
    /// A call-free local projection binding, capture readout, or narrowing guard assertion.
    InspectionOrGuard,
    /// Any other executable statement (an action or state mutation).
    Action,
}

/// Collects all outermost test functions in `file` along with their identifier node, name, and assertion count.
#[must_use]
pub(in crate::code_lint) fn collect_test_function_assertion_counts(
    file: &ParsedFile,
) -> Vec<TestAssertionCount<'_>> {
    dispatch_lang!(file.lang(), collect_test_function_assertion_counts(file))
}

/// Collects control-flow and conditional constructs directly inside outermost test functions in `file`.
#[must_use]
pub(in crate::code_lint) fn collect_test_logic_constructs(
    file: &ParsedFile,
) -> Vec<TestLogicConstruct<'_>> {
    dispatch_lang!(file.lang(), collect_test_logic_constructs(file))
}

/// Collects outermost test functions in `file` that interleave non-assertion statements between
/// behavioral assertions.
#[must_use]
pub(in crate::code_lint) fn collect_multi_phase_test_functions(
    file: &ParsedFile,
) -> Vec<MultiPhaseTest<'_>> {
    dispatch_lang!(file.lang(), collect_multi_phase_test_functions(file))
}

/// Collects all Python `assert` statements and Rust assertion macro invocations in `file`
/// with their condition-packing properties.
#[must_use]
pub(in crate::code_lint) fn collect_assertion_checks(file: &ParsedFile) -> Vec<AssertionCheck<'_>> {
    dispatch_lang!(file.lang(), collect_assertion_checks(file))
}

/// Collects test functions grouped by enclosing module, class, or `mod` scope (segmented on
/// duplicate function names) along with their leading non-assertion setup statements.
#[must_use]
pub(in crate::code_lint) fn collect_test_setup_scopes(
    file: &ParsedFile,
) -> Vec<Vec<TestFunctionSetup<'_>>> {
    dispatch_lang!(file.lang(), collect_test_setup_scopes(file))
}

/// Collects calls in `file` where a function calls a test function defined in the same file.
#[must_use]
pub(in crate::code_lint) fn collect_test_to_test_calls(
    file: &ParsedFile,
) -> Vec<TestToTestCall<'_>> {
    dispatch_lang!(file.lang(), collect_test_to_test_calls(file))
}

/// Computes the fixed point of same-file assertion helper names from `candidates`.
pub(super) fn resolve_assertion_helpers<'a, T: 'a>(
    candidates: &'a [T],
    name_of: impl Fn(&'a T) -> &'a str,
    is_helper: impl Fn(&'a T, &HashSet<String>) -> bool,
) -> HashSet<String> {
    let mut helpers = HashSet::new();
    loop {
        let mut added = false;
        for candidate in candidates {
            let name = name_of(candidate);
            if !helpers.contains(name) && is_helper(candidate, &helpers) {
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

/// Returns true if `roles` contains an `Action` after an `Assertion` followed by another `Assertion`.
pub(super) fn has_multi_phase_interleaving(
    roles: impl IntoIterator<Item = TestStatementRole>,
) -> bool {
    let mut seen_assertion = false;
    let mut seen_action_after_assertion = false;
    for role in roles {
        match role {
            TestStatementRole::InspectionOrGuard => {}
            TestStatementRole::Assertion => {
                if seen_action_after_assertion {
                    return true;
                }
                seen_assertion = true;
            }
            TestStatementRole::Action => {
                if seen_assertion {
                    seen_action_after_assertion = true;
                }
            }
        }
    }
    false
}

/// Normalizes a statement's source slice by trimming each line and joining with `\n`.
pub(super) fn normalize_statement_text(raw: &str) -> String {
    raw.lines().map(str::trim).collect::<Vec<_>>().join("\n")
}

/// Returns true if `name` is an assertion helper function or method name (`assert_*` or `_assert_*`).
#[must_use]
pub(super) fn is_assertion_function_name(name: &str) -> bool {
    name.starts_with("assert_") || name.starts_with("_assert_")
}
