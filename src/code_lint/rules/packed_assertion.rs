//! Flags compound boolean conditions (`&&`, `and`) and boolean tuple equality packing in test assertions.

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: {
        Python => "`assert` packs several checks into {construct}.",
        Rust => "`{callee}!` packs several checks into {construct}.",
    },
    rationale: "When one assertion holds several independent checks, a failure does not say which check failed, and the diff shows the whole compound value instead of the mismatching part.",
    suggestion: {
        Python => "Split the checks into separate `assert` statements, or compare one domain object directly.",
        Rust => "Split the checks into separate `assert!` / `assert_eq!` calls, or compare one domain struct directly.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("packed-assertion"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::TEST_ASSERTIONS],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags assertions that pack several checks into one condition.",
            what_it_does: indoc::indoc! {r"
                Flags a test assertion that joins checks with a top-level `and` (Python
                `assert`) or `&&` (Rust `assert!`), compares two tuple, list, array, set or
                dict literals of two or more elements (such as `assert (a, b) == (1, 2)`), or
                compares against a collection of two or more boolean literals (such as
                `assert_eq!(flags, (true, false))`). Conditions joined by `or` and comparisons
                of a single value against an expected collection (such as
                `assert dimensions == (1920, 1080)`) are not flagged."},
            why_is_this_bad: indoc::indoc! {r"
                When a packed assertion fails, the report only says the whole condition was false:
                it does not say which part failed, and a tuple of booleans shows `True`/`False`
                values with no name attached. Finding the culprit means rerunning the test or adding
                prints.

                Write one assertion per check, so each failure names its condition and shows its
                values, or compare the result against one expected object or struct."},
            known_problems: Some(indoc::indoc! {r"
                In Python, only `assert` statements are checked, not `unittest` methods such as
                `self.assertTrue(a and b)`."}),
            references: &[Reference {
                title: "pytest: How to write and report assertions in tests",
                url: "https://docs.pytest.org/en/stable/how-to/assert.html",
            }],
            examples: &[
                Example {
                    language: Language::Python,
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
                    language: Language::Rust,
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

/// Template placeholder naming the packed construct.
const CONSTRUCT: &str = "construct";
/// Template placeholder naming the Rust assertion macro.
const CALLEE: &str = "callee";
/// `construct` value shared by the Python and Rust boolean collection checks.
const BOOLEAN_COLLECTION_COMPARISON: &str = "a comparison against a collection of boolean literals";
/// `construct` value shared by the Python and Rust composite collection checks.
const COMPOSITE_COLLECTION_COMPARISON: &str = "a composite collection comparison";

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    ast::collect_assertion_checks(file)
        .iter()
        .filter_map(|check| check_assertion(rule, check, file.lang(), path))
        .collect()
}

/// Evaluates a single assertion check for packed conditions.
fn check_assertion(
    rule: &CodeRule,
    check: &ast::AssertionCheck<'_>,
    lang: Language,
    path: &Path,
) -> Option<Diagnostic> {
    let construct = if check.has_top_level_logical_and {
        match lang {
            Language::Python => "a compound `and` condition",
            Language::Rust => "a compound `&&` condition",
        }
    } else if check.has_boolean_literal_comparison {
        BOOLEAN_COLLECTION_COMPARISON
    } else if check.has_composite_collection_comparison {
        COMPOSITE_COLLECTION_COMPARISON
    } else {
        return None;
    };

    Some(rule.diagnostic_at_node(
        path,
        &check.node,
        &[(CONSTRUCT, construct), (CALLEE, &check.callee)],
    ))
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
                        assert dimensions == (1920, 1080)
                "#,
                non_boolean_list_equality => r#"
                    def test_list_values():
                        assert items == [1, 2]
                "#,
                single_value_dict_equality => r#"
                    def test_dict_values():
                        assert payload == {"status": "ok", "count": 2}
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
                composite_tuple_equality => r#"
                    def test_example():
                        assert (width, height) == (1920, 1080)
                "# => r#"assert (width, height) == (1920, 1080)"#,
                composite_list_equality => r#"
                    def test_example():
                        assert [first, second] == [1, 2]
                "# => r#"assert [first, second] == [1, 2]"#,
                composite_dict_equality => r#"
                    def test_example():
                        assert {"a": first, "b": second} == {"a": 1, "b": 2}
                "# => r#"assert {"a": first, "b": second} == {"a": 1, "b": 2}"#,
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
                        assert_eq!(dimensions, (1920, 1080));
                    }
                "#,
                non_boolean_array_equality => r#"
                    #[test]
                    fn test_array_values() {
                        assert_eq!([compute(true), compute(false)], expected);
                    }
                "#,
                call_with_collection_result => r#"
                    #[test]
                    fn test_ancestors() {
                        assert_eq!(local_ancestors(&classes, 3), [1, 0, 2]);
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
                composite_tuple_equality => r#"
                    #[test]
                    fn test_example() {
                        assert_eq!((width, height), (1920, 1080));
                    }
                "# => r#"assert_eq!((width, height), (1920, 1080))"#,
                composite_array_equality => r#"
                    #[test]
                    fn test_example() {
                        assert_eq!([first, second], [1, 2]);
                    }
                "# => r#"assert_eq!([first, second], [1, 2])"#,
            ],
        },
    }
);
