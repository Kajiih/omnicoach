//! Flags compound boolean conditions (`&&`, `and`) and boolean tuple equality packing in test assertions (`packed-assertion`).

use crate::code_lint::ast::{self, AstNode, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: {
        base: "Assertion packs several checks into {construct}.",
        Python => "`assert` packs several checks into {construct}.",
        Rust => "`{callee}!` packs several checks into {construct}.",
    },
    rationale: "When one assertion holds several independent checks, a failure does not say which check failed, and the diff shows the whole compound value instead of the mismatching part.",
    suggestion: {
        base: "Split the checks into separate assertions, or compare one domain object directly.",
        Python => "Split the checks into separate `assert` statements, or compare one domain object directly.",
        Rust => "Split the checks into separate `assert!` / `assert_eq!` calls, or compare one domain struct directly.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("packed-assertion"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python, SupportLang::Rust],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::TEST_ASSERTIONS],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags assertions that pack several checks into one condition.",
            what_it_does: "Flags two shapes of assertion in test files. A compound condition \
                           joined by a top-level `and` in a Python `assert` statement, or by a \
                           top-level `&&` in a Rust `assert!` or `debug_assert!`. And a \
                           comparison against a tuple, list or array of two or more boolean \
                           literals, such as `assert (valid, active) == (True, False)` or \
                           `assert_eq!((a, b), (true, true))` in any `assert_*` or \
                           `debug_assert_*` macro. Conditions joined by `or`, an `and` nested \
                           inside a function call, and collections holding anything other than \
                           boolean literals are not flagged. In Python only bare `assert` \
                           statements are checked, not `unittest` methods such as \
                           `self.assertTrue`.",
            why_is_this_bad: "When a packed assertion fails, the report only says the whole \
                              condition was false: it does not say which part failed, and a \
                              tuple of booleans shows `True`/`False` values with no name \
                              attached. Finding the culprit means rerunning the test or adding \
                              prints.\n\n\
                              Write one assertion per check, so each failure names its \
                              condition and shows its values, or compare the result against one \
                              expected object or struct.",
            references: &[Reference {
                title: "pytest: How to write and report assertions in tests",
                url: "https://docs.pytest.org/en/stable/how-to/assert.html",
            }],
            examples: &[
                Example {
                    language: SupportLang::Python,
                    flagged: indoc::indoc! {r"
                        def test_login():
                            assert user.is_active and user.is_verified
                    "},
                    flagged_span: "assert user.is_active and user.is_verified",
                    fixed: indoc::indoc! {r"
                        def test_login():
                            assert user.is_active
                            assert user.is_verified
                    "},
                },
                Example {
                    language: SupportLang::Rust,
                    flagged: indoc::indoc! {r"
                        #[test]
                        fn test_login() {
                            assert!(user.is_active && user.is_verified);
                        }
                    "},
                    flagged_span: "assert!(user.is_active && user.is_verified)",
                    fixed: indoc::indoc! {r"
                        #[test]
                        fn test_login() {
                            assert!(user.is_active);
                            assert!(user.is_verified);
                        }
                    "},
                },
            ],
        },
    },
    target: RuleTarget::TestsOnly,
    check: check_file,
};

/// Evaluates a single Rust assertion macro invocation node for packed conditions.
fn check_rust_assertion_macro(
    rule: &CodeRule,
    macro_node: &AstNode<'_>,
    path: &Path,
) -> Option<Diagnostic> {
    let macro_name = ast::rust::macro_terminal_name(macro_node);

    // 1. Compound boolean condition: assert!(a && b)
    if (macro_name == "assert" || macro_name == "debug_assert")
        && ast::rust::has_top_level_logical_and(macro_node)
    {
        return Some(rule.diagnostic_at_node(
            path,
            macro_node,
            &[
                ("construct", "a compound `&&` condition"),
                ("callee", &macro_name),
            ],
        ));
    }

    // 2. Boolean tuple/array equality packing: assert_eq!((a, b), (true, true))
    if (macro_name.starts_with("assert_") || macro_name.starts_with("debug_assert_"))
        && ast::rust::extract_macro_arguments(macro_node)
            .iter()
            .any(ast::rust::is_boolean_literal_collection)
    {
        return Some(rule.diagnostic_at_node(
            path,
            macro_node,
            &[
                (
                    "construct",
                    "a comparison against a collection of boolean literals",
                ),
                ("callee", &macro_name),
            ],
        ));
    }

    None
}

/// Evaluates a single Python `assert` statement node for packed conditions.
fn check_python_assert_statement(
    rule: &CodeRule,
    assert_node: &AstNode<'_>,
    path: &Path,
) -> Option<Diagnostic> {
    // 1. Compound boolean condition: assert a and b
    if ast::python::has_top_level_logical_and(assert_node) {
        return Some(rule.diagnostic_at_node(
            path,
            assert_node,
            &[("construct", "a compound `and` condition")],
        ));
    }

    // 2. Boolean tuple/list equality: assert (a, b) == (True, True)
    if ast::python::has_boolean_literal_comparison(assert_node) {
        return Some(rule.diagnostic_at_node(
            path,
            assert_node,
            &[(
                "construct",
                "a comparison against a collection of boolean literals",
            )],
        ));
    }

    None
}

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    match file.lang() {
        SupportLang::Rust => ast::rust::collect_macro_invocations(file)
            .iter()
            .filter_map(|node| check_rust_assertion_macro(rule, node, path))
            .collect(),
        _ => ast::python::collect_assert_statements(file)
            .iter()
            .filter_map(|node| check_python_assert_statement(rule, node, path))
            .collect(),
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
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
