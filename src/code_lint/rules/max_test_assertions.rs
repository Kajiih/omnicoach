//! Enforces a maximum number of assertions per test function (`max-test-assertions`).

use crate::code_lint::{AstNode, CodeRule, RuleTarget, SourceDoc};
use crate::core::{Config, LanguageDefaults, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Default maximum assertions allowed per test function (`4`).
const DEFAULT_MAX_ASSERTIONS: LanguageDefaults<usize> = LanguageDefaults::new(4, &[]);

// TODO: In cases like this where the python and rust versions are almost the same, could we factorize this?
const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Test function `{func}` contains {count} assertions (maximum allowed is {max}).",
    rationale: "Tests with excessive assertions verify multiple unrelated behaviors at once and halt at the first failure, masking subsequent checks and complicating diagnosis.",
    suggestion: {
        base: "Assert on a single expected struct/value, split distinct scenarios into separate focused test functions, or parameterize test variations.",
        Python => "Assert on a single expected object/value, split distinct scenarios into separate `test_*` functions, or parameterize variations with `@pytest.mark.parametrize`.",
        Rust => "Assert on a single expected struct/value, split distinct scenarios into separate `#[test]` functions, or parameterize cases with `#[rstest]`.",
    },
};

/// Rule that limits the number of assertions inside a single test function.
pub struct MaxTestAssertions;

impl Rule for MaxTestAssertions {
    fn name(&self) -> RuleName {
        RuleName("max-test-assertions")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Testing, Tag::Heuristic, Tag::Opinionated]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

/// Recursively counts top-level assertion constructs in a Python test function body.
fn count_python_assertions(node: &AstNode<'_>) -> usize {
    let kind = node.kind();
    if matches!(kind.as_ref(), "function_definition" | "class_definition") {
        return 0;
    }
    if kind == "assert_statement"
        || (kind == "call" && crate::code_lint::ast_python::is_assertion_call(node))
    {
        return 1;
    }
    node.children()
        .map(|child| count_python_assertions(&child))
        .sum()
}

/// Recursively counts top-level assertion macro invocations in a Rust test function body.
fn count_rust_assertions(node: &AstNode<'_>) -> usize {
    let kind = node.kind();
    if kind == "function_item" {
        return 0;
    }
    if kind == "macro_invocation" && crate::code_lint::ast_rust::is_assertion_macro(node) {
        return 1;
    }
    node.children()
        .map(|child| count_rust_assertions(&child))
        .sum()
}

/// Evaluates a single test function against `max_allowed` assertions.
fn check_test_function(
    rule: &MaxTestAssertions,
    func_node: &AstNode<'_>,
    lang: SupportLang,
    path: &Path,
    max_allowed: usize,
) -> Option<Diagnostic> {
    let name_node = func_node.field("name")?;
    let body_node = func_node.field("body")?;
    let func_name = name_node.text();

    let assertion_count = match lang {
        SupportLang::Rust => count_rust_assertions(&body_node),
        _ => count_python_assertions(&body_node),
    };

    if assertion_count <= max_allowed {
        return None;
    }

    let formatted_count = assertion_count.to_string();
    let formatted_max = max_allowed.to_string();
    Some(rule.diagnostic_at_node(
        path,
        &name_node,
        &[
            ("func", &func_name),
            ("count", &formatted_count),
            ("max", &formatted_max),
        ],
    ))
}

impl CodeRule for MaxTestAssertions {
    fn target(&self) -> RuleTarget {
        RuleTarget::TestsOnly
    }

    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<SourceDoc>,
        config: &Config,
    ) -> Vec<Diagnostic> {
        let lang = *grep.lang();
        let max_allowed = self.effective_max_threshold(lang, config, &DEFAULT_MAX_ASSERTIONS);

        crate::code_lint::collect_test_functions(&grep.root(), lang)
            .iter()
            .filter_map(|func_node| check_test_function(self, func_node, lang, path, max_allowed))
            .collect()
    }
}

#[cfg(test)]
crate::rule_test!(
    MaxTestAssertions,
    {
        Python => {
            pass: [
                helper_function_exempt => r#"
                    def assert_helper(response):
                        assert response.status == 200
                        assert response.body is not None
                        assert response.headers
                        assert response.cookies
                        assert response.ok
                "#,
                exact_threshold_of_four_allowed => r#"
                    import pytest

                    def test_focused():
                        assert 1 + 1 == 2
                        assert 2 + 2 == 4
                        with pytest.raises(ValueError):
                            int("bad")
                        assert True
                "#,
                nested_function_assertions_not_counted => r#"
                    def test_with_nested_function():
                        def inner_verifier(item):
                            assert item > 0
                            assert item < 100

                        assert 1 == 1
                        assert 2 == 2
                        assert 3 == 3
                        assert 4 == 4
                "#,
                nested_class_assertions_not_counted => r#"
                    def test_with_nested_class():
                        class LocalCheck:
                            def verify(self):
                                assert True

                        assert 1 == 1
                        assert 2 == 2
                        assert 3 == 3
                        assert 4 == 4
                "#,
            ],
            fail: [
                assert_statements_exceeding_threshold => r#"
                    def test_ok():
                        assert 1 == 1

                    def test_too_many():
                        assert 1 == 1
                        assert 2 == 2
                        assert 3 == 3
                        assert 4 == 4
                        assert 5 == 5
                "# => "test_too_many",
                unittest_assertions_exceeding_threshold => r#"
                    class OrderTest:
                        def test_order_lifecycle(self):
                            self.assertEqual(1, 1)
                            self.assertTrue(True)
                            self.assertIsNotNone("ok")
                            self.assertIn("a", "abc")
                            self.fail("unreachable")
                "# => "test_order_lifecycle",
                pytest_raises_calls_exceeding_threshold => r#"
                    import pytest

                    def test_raises_lifecycle():
                        with pytest.raises(ValueError):
                            int("1")
                        with pytest.raises(ValueError):
                            int("2")
                        with pytest.raises(ValueError):
                            int("3")
                        with pytest.raises(ValueError):
                            int("4")
                        with pytest.raises(ValueError):
                            int("5")
                "# => "test_raises_lifecycle",
                bare_raises_calls_exceeding_threshold => r#"
                    from pytest import raises

                    def test_bare_raises():
                        with raises(ValueError):
                            int("1")
                        with raises(ValueError):
                            int("2")
                        with raises(ValueError):
                            int("3")
                        with raises(ValueError):
                            int("4")
                        with raises(ValueError):
                            int("5")
                "# => "test_bare_raises",
                pytest_warns_calls_exceeding_threshold => r#"
                    import pytest

                    def test_warns_lifecycle():
                        with pytest.warns(UserWarning):
                            pass
                        with pytest.warns(UserWarning):
                            pass
                        with pytest.warns(UserWarning):
                            pass
                        with pytest.warns(UserWarning):
                            pass
                        with pytest.warns(UserWarning):
                            pass
                "# => "test_warns_lifecycle",
            ],
        },
        Rust => {
            pass: [
                helper_function_exempt => r#"
                    fn assert_helper() {
                        assert_eq!(1, 1);
                        assert_eq!(2, 2);
                        assert_eq!(3, 3);
                        assert_eq!(4, 4);
                        assert_eq!(5, 5);
                    }
                "#,
                exact_threshold_of_four_allowed => r#"
                    #[test]
                    fn parses_valid_header() {
                        assert_eq!(1, 1);
                        assert_ne!(1, 2);
                        assert!(true);
                        debug_assert_eq!(3, 3);
                    }
                "#,
                nested_function_assertions_not_counted => r#"
                    #[test]
                    fn test_with_nested_function() {
                        fn inner_check() {
                            assert_eq!(10, 10);
                            assert_eq!(20, 20);
                        }
                        assert_eq!(1, 1);
                        assert_eq!(2, 2);
                        assert_eq!(3, 3);
                        assert_eq!(4, 4);
                    }
                "#,
            ],
            fail: [
                attributed_test_function_exceeding_threshold => r#"
                    #[tokio::test]
                    async fn attributed_check() {
                        assert_eq!(1, 1);
                        assert_eq!(2, 2);
                        assert_eq!(3, 3);
                        assert_eq!(4, 4);
                        assert_eq!(5, 5);
                    }
                "# => "attributed_check",
                unattributed_test_function_exceeding_threshold => r#"
                    fn test_unattributed() {
                        assert_eq!(1, 1);
                        assert_eq!(2, 2);
                        assert_eq!(3, 3);
                        assert_eq!(4, 4);
                        assert_eq!(5, 5);
                    }
                "# => "test_unattributed",
            ],
        },
    }
);
