//! Shared data structures and language-agnostic algorithms for test-function and assertion analysis.

use super::AstNode;
use std::collections::HashSet;

/// An outermost test function and the number of behavioral assertions in its body.
pub struct TestAssertionCount<'a> {
    /// Identifier AST node of the test function declaration.
    pub name_node: AstNode<'a>,
    /// Name of the test function.
    pub name: String,
    /// Number of behavioral assertions in the test function body.
    pub count: usize,
}

/// A branching or loop construct directly inside an outermost test function.
pub struct TestLogicConstruct<'a> {
    /// AST node of the branching or loop construct.
    pub node: AstNode<'a>,
    /// Name of the enclosing test function.
    pub test_name: String,
    /// Human-readable description of the construct (e.g. "an `if` statement").
    pub construct: &'static str,
}

/// An outermost test function that interleaves actions and assertions across multiple phases.
pub struct MultiPhaseTest<'a> {
    /// Identifier AST node of the test function declaration.
    pub name_node: AstNode<'a>,
    /// Name of the test function.
    pub name: String,
}

/// A Python `assert` statement or Rust assertion macro invocation (`assert!`, `assert_eq!`,
/// `assert_ne!`, `debug_assert*!`) with its condition-packing properties.
pub struct AssertionCheck<'a> {
    /// The `assert` statement or macro invocation node.
    pub node: AstNode<'a>,
    /// Rust macro terminal name (`"assert"`, `"assert_eq"`, ...), or empty in Python.
    pub callee: String,
    /// True if the condition has a top-level `and` (Python) or `&&` (Rust `assert!` / `debug_assert!`).
    pub has_top_level_logical_and: bool,
    /// True if the assertion compares against a tuple/list/array/`vec!` of `>= 2` boolean literals.
    pub has_boolean_literal_comparison: bool,
    /// True if the assertion compares two or more multi-element collection literals.
    pub has_composite_collection_comparison: bool,
}

/// A test function and its normalized leading non-assertion setup statements.
pub struct TestFunctionSetup<'a> {
    /// Identifier AST node of the test function declaration.
    pub name_node: AstNode<'a>,
    /// Name of the test function.
    pub name: String,
    /// Normalized source text of each leading non-assertion setup statement, in source order.
    pub leading_setup_statements: Vec<String>,
}

/// A call inside a test file to another test function defined in the same file.
pub struct TestToTestCall<'a> {
    /// The call expression node.
    pub node: AstNode<'a>,
    /// The name of the enclosing function making the call.
    pub caller: String,
    /// The source callee text as written (`test_login`, `self.test_login`).
    pub callee: String,
}

/// Classification of a top-level statement in a test body for multi-phase test detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::code_lint::ast) enum TestStatementRole {
    /// A behavioral assertion statement or call.
    Assertion,
    /// A call-free local projection binding, capture readout, or narrowing guard assertion.
    InspectionOrGuard,
    /// Any other executable statement (an action or state mutation).
    Action,
}

/// Computes the fixed point of same-file assertion helper names from `candidates`.
pub(in crate::code_lint::ast) fn resolve_assertion_helpers<'a, T: 'a>(
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
pub(in crate::code_lint::ast) fn has_multi_phase_interleaving(
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
pub(in crate::code_lint::ast) fn normalize_statement_text(raw: &str) -> String {
    raw.lines().map(str::trim).collect::<Vec<_>>().join("\n")
}

/// Returns true if `name` is an assertion helper function or method name (`assert_*` or `_assert_*`).
#[must_use]
pub(in crate::code_lint::ast) fn is_assertion_function_name(name: &str) -> bool {
    name.starts_with("assert_") || name.starts_with("_assert_")
}
