//! Flags compound boolean conditions (`&&`, `and`) and boolean tuple equality packing in test assertions (`no-assertion-packing`).

use crate::code_lint::{AstNode, CodeRule, RuleTarget, SourceDoc};
use crate::core::{Config, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: {
        base: "{construct} in assertion.",
        Python => "{construct} in `assert` statement.",
        Rust => "{construct} in `{macro_name}!` assertion.",
    },
    rationale: "Packing multiple conditions or synthesized boolean tuples into a single assertion obscures which check failed, yields unhelpful diffs, and circumvents assertion limits.",
    suggestion: {
        base: "Split into separate atomic assertions or assert directly on domain objects/collections.",
        Python => "Split into separate atomic assertions (e.g. `assert a\\nassert b`) or assert directly on the domain model (`assert actual == expected`).",
        Rust => "Split into separate atomic assertions (`assert!(...); assert!(...);`) or assert directly on the domain model (`assert_eq!(actual, expected)`).",
    },
};

/// Rule that bans compound boolean conditions and boolean tuple packing in assertions.
pub struct NoAssertionPacking;

impl Rule for NoAssertionPacking {
    fn name(&self) -> RuleName {
        RuleName("no-assertion-packing")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Testing, Tag::Opinionated]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

/// Evaluates a single Rust assertion `macro_invocation` node for packed conditions.
fn check_rust_assertion_macro(macro_node: &AstNode<'_>, path: &Path) -> Option<Diagnostic> {
    let macro_name = crate::code_lint::ast_rust::macro_terminal_name(macro_node);

    // 1. Compound boolean condition: assert!(a && b)
    if (macro_name == "assert" || macro_name == "debug_assert")
        && crate::code_lint::ast_rust::has_top_level_logical_and(macro_node)
    {
        return Some(NoAssertionPacking.diagnostic_at_node(
            path,
            macro_node,
            &[
                ("construct", "Compound boolean condition (`&&`)"),
                ("macro_name", &macro_name),
            ],
        ));
    }

    // 2. Boolean tuple/array equality packing: assert_eq!((a, b), (true, true))
    if (macro_name.starts_with("assert_") || macro_name.starts_with("debug_assert_"))
        && crate::code_lint::ast_rust::extract_macro_arguments(macro_node)
            .iter()
            .any(crate::code_lint::ast_rust::is_boolean_literal_collection)
    {
        return Some(NoAssertionPacking.diagnostic_at_node(
            path,
            macro_node,
            &[
                ("construct", "Boolean tuple/collection equality"),
                ("macro_name", &macro_name),
            ],
        ));
    }

    None
}

/// Evaluates a single Python `assert_statement` node for packed conditions.
fn check_python_assert_statement(assert_node: &AstNode<'_>, path: &Path) -> Option<Diagnostic> {
    // 1. Compound boolean condition: assert a and b
    let has_and = assert_node
        .children()
        .any(|c| c.kind() == "boolean_operator" && c.children().any(|op| op.kind() == "and"));

    if has_and {
        return Some(NoAssertionPacking.diagnostic_at_node(
            path,
            assert_node,
            &[("construct", "Compound boolean condition (`and`)")],
        ));
    }

    // 2. Boolean tuple/list equality: assert (a, b) == (True, True)
    let comparison = assert_node
        .children()
        .find(|c| c.kind() == "comparison_operator")?;
    if comparison
        .children()
        .any(|c| crate::code_lint::ast_python::is_boolean_literal_collection(&c))
    {
        return Some(NoAssertionPacking.diagnostic_at_node(
            path,
            assert_node,
            &[("construct", "Boolean tuple/collection equality")],
        ));
    }

    None
}

impl CodeRule for NoAssertionPacking {
    fn target(&self) -> RuleTarget {
        RuleTarget::TestsOnly
    }

    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<SourceDoc>,
        _config: &Config,
    ) -> Vec<Diagnostic> {
        match grep.lang() {
            SupportLang::Rust => grep
                .root()
                .dfs()
                .filter(|node| node.kind() == "macro_invocation")
                .filter_map(|node| check_rust_assertion_macro(&node, path))
                .collect(),
            _ => grep
                .root()
                .dfs()
                .filter(|node| node.kind() == "assert_statement")
                .filter_map(|node| check_python_assert_statement(&node, path))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_code_rule_snapshot;
    use indoc::indoc;

    #[test]
    fn test_rust_assertion_packing_flagged() {
        let source = indoc! {r"
            fn assert_helper_packed(ready: bool, connected: bool) {
                assert!(ready && connected);
            }

            #[test]
            fn test_packed_assertions() {
                assert!(ready && connected);
                assert_eq!((valid, active), (true, true));
                assert_eq!((status, ready), (false, true));
                assert_eq!([first, second], [true, true]);
            }

            #[test]
            fn test_valid_assertions() {
                assert!(ready);
                assert!(connected);
                assert_eq!(count, 10);
                assert_eq!(coords, (10, 20));
                assert_eq!(flag, true);
                assert!(check_connection(ready && connected));
                assert_eq!([compute(true), compute(false)], expected);
            }
        "};

        insta::assert_snapshot!(
            assert_code_rule_snapshot(&NoAssertionPacking, source, "tests/test_packing.rs"),
            @"
        [no-assertion-packing] Line 2, Col 5: Compound boolean condition (`&&`) in `assert!` assertion.
        [no-assertion-packing] Line 7, Col 5: Compound boolean condition (`&&`) in `assert!` assertion.
        [no-assertion-packing] Line 8, Col 5: Boolean tuple/collection equality in `assert_eq!` assertion.
        [no-assertion-packing] Line 9, Col 5: Boolean tuple/collection equality in `assert_eq!` assertion.
        [no-assertion-packing] Line 10, Col 5: Boolean tuple/collection equality in `assert_eq!` assertion.
        "
        );
    }

    #[test]
    fn test_python_assertion_packing_flagged() {
        let source = indoc! {r"
            def check_helper_packed(ready: bool, connected: bool) -> None:
                assert ready and connected

            def test_packed():
                assert ready and connected
                assert (valid, active) == (True, True)
                assert (status, ready) == (False, True)
                assert [first, second] == [True, True]

            def test_valid():
                assert ready
                assert connected
                assert count == 10
                assert coords == (10, 20)
                assert flag == True
                assert check_connection(ready and connected)
        "};

        insta::assert_snapshot!(
            assert_code_rule_snapshot(&NoAssertionPacking, source, "tests/test_packing.py"),
            @"
        [no-assertion-packing] Line 2, Col 5: Compound boolean condition (`and`) in `assert` statement.
        [no-assertion-packing] Line 5, Col 5: Compound boolean condition (`and`) in `assert` statement.
        [no-assertion-packing] Line 6, Col 5: Boolean tuple/collection equality in `assert` statement.
        [no-assertion-packing] Line 7, Col 5: Boolean tuple/collection equality in `assert` statement.
        [no-assertion-packing] Line 8, Col 5: Boolean tuple/collection equality in `assert` statement.
        "
        );
    }
}
