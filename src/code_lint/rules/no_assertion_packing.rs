//! Flags compound boolean conditions (`&&`, `and`) and boolean tuple equality packing in test assertions (`no-assertion-packing`).

use crate::code_lint::ast::{self, AstNode, ParsedFile};
use crate::code_lint::rule::{CodeRule, RuleTarget};
use crate::core::{Config, Rule, Tag};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: {
        base: "{construct} in assertion.",
        Python => "{construct} in `assert` statement.",
        Rust => "{construct} in `{macro_name}!` assertion.",
    },
    rationale: "Packing multiple independent checks into a single assertion obscures which condition failed and produces unhelpful failure diffs.",
    suggestion: {
        base: "Split into separate atomic assertions or compare a single domain struct/object directly.",
        Python => "Split into separate `assert` statements or compare a single domain object directly.",
        Rust => "Split into separate `assert!` / `assert_eq!` macros or compare a single domain struct directly.",
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

/// Evaluates a single Rust assertion macro invocation node for packed conditions.
fn check_rust_assertion_macro(macro_node: &AstNode<'_>, path: &Path) -> Option<Diagnostic> {
    let macro_name = ast::rust::macro_terminal_name(macro_node);

    // 1. Compound boolean condition: assert!(a && b)
    if (macro_name == "assert" || macro_name == "debug_assert")
        && ast::rust::has_top_level_logical_and(macro_node)
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
        && ast::rust::extract_macro_arguments(macro_node)
            .iter()
            .any(ast::rust::is_boolean_literal_collection)
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

/// Evaluates a single Python `assert` statement node for packed conditions.
fn check_python_assert_statement(assert_node: &AstNode<'_>, path: &Path) -> Option<Diagnostic> {
    // 1. Compound boolean condition: assert a and b
    if ast::python::has_top_level_logical_and(assert_node) {
        return Some(NoAssertionPacking.diagnostic_at_node(
            path,
            assert_node,
            &[("construct", "Compound boolean condition (`and`)")],
        ));
    }

    // 2. Boolean tuple/list equality: assert (a, b) == (True, True)
    if ast::python::has_boolean_literal_comparison(assert_node) {
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

    fn check_file(&self, path: &Path, file: &ParsedFile, _config: &Config) -> Vec<Diagnostic> {
        match file.lang() {
            SupportLang::Rust => ast::rust::collect_macro_invocations(file)
                .iter()
                .filter_map(|node| check_rust_assertion_macro(node, path))
                .collect(),
            _ => ast::python::collect_assert_statements(file)
                .iter()
                .filter_map(|node| check_python_assert_statement(node, path))
                .collect(),
        }
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    NoAssertionPacking,
    {
        Python => {
            pass: [
                atomic_assertion => r#"
                    def test_atomic():
                        assert ready
                        assert actual == expected
                "#,
                disjunctive_or_condition => r#"
                    def test_disjunction():
                        assert a == 1 or b == 2
                "#,
                single_element_boolean_tuple => r#"
                    def test_single_element():
                        assert (flag,) == (True,)
                "#,
                logical_and_inside_function_call => r#"
                    def test_nested_call():
                        assert check_connection(ready and connected)
                "#,
                non_boolean_tuple_equality => r#"
                    def test_tuple_values():
                        assert (width, height) == (1920, 1080)
                "#,
                non_boolean_list_equality => r#"
                    def test_list_values():
                        assert [first, second] == [1, 2]
                "#,
            ],
            fail: [
                compound_and_in_assert => r#"
                    def test_example():
                        assert a == 1 and b == 2
                "# => r#"assert a == 1 and b == 2"#,
                boolean_tuple_equality => r#"
                    def test_example():
                        assert (valid, active) == (True, False)
                "# => r#"assert (valid, active) == (True, False)"#,
                boolean_list_equality => r#"
                    def test_example():
                        assert [first, second] == [True, True]
                "# => r#"assert [first, second] == [True, True]"#,
            ],
        },
        Rust => {
            pass: [
                atomic_assertion => r#"
                    #[test]
                    fn test_atomic() {
                        assert!(ready);
                        assert_eq!(actual, expected);
                    }
                "#,
                disjunctive_or_condition => r#"
                    #[test]
                    fn test_disjunction() {
                        assert!(a == 1 || b == 2);
                    }
                "#,
                single_element_boolean_tuple => r#"
                    #[test]
                    fn test_single_tuple() {
                        assert_eq!((flag,), (true,));
                    }
                "#,
                logical_and_inside_function_call => r#"
                    #[test]
                    fn test_nested_call() {
                        assert!(check_connection(ready && connected));
                    }
                "#,
                non_boolean_tuple_equality => r#"
                    #[test]
                    fn test_tuple_values() {
                        assert_eq!((width, height), (1920, 1080));
                    }
                "#,
                non_boolean_array_equality => r#"
                    #[test]
                    fn test_array_values() {
                        assert_eq!([compute(true), compute(false)], expected);
                    }
                "#,
            ],
            fail: [
                compound_and_in_assert => r#"
                    #[test]
                    fn test_example() {
                        assert!(a == 1 && b == 2);
                    }
                "# => r#"assert!(a == 1 && b == 2)"#,
                compound_and_in_debug_assert => r#"
                    #[test]
                    fn test_example() {
                        debug_assert!(ready && connected);
                    }
                "# => r#"debug_assert!(ready && connected)"#,
                boolean_tuple_equality => r#"
                    #[test]
                    fn test_example() {
                        assert_eq!((valid, active), (true, false));
                    }
                "# => r#"assert_eq!((valid, active), (true, false))"#,
                boolean_array_equality => r#"
                    #[test]
                    fn test_example() {
                        debug_assert_ne!([first, second], [true, true]);
                    }
                "# => r#"debug_assert_ne!([first, second], [true, true])"#,
            ],
        },
    }
);
