//! Enforces a maximum number of assertions per test function (`max-test-assertions`).

use crate::code_lint::{AstNode, CodeRule, RuleTarget, SourceDoc};
use crate::core::{Config, DynamicRuleConfig, LanguageDefaults, Rule, RuleName, ThresholdConfig};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Default maximum assertions allowed per test function (`4`).
const DEFAULT_MAX_ASSERTIONS: LanguageDefaults<usize> = LanguageDefaults::new(4, &[]);

/// Configuration for the `MaxTestAssertions` rule.
pub type MaxTestAssertionsConfig = DynamicRuleConfig<ThresholdConfig>;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Test function `{func}` has {count} assertions, exceeding the maximum of {max}.",
    rationale: "Tests with too many assertions often verify multiple unrelated behaviors. Obscuring them with ad-hoc helper closures, filtering loops, or artificial compression hurts readability and makes failures harder to diagnose.",
    suggestion: {
        base: "Refactor `{func}` by choosing the appropriate pattern: (1) Split distinct steps or behaviors into separate focused test functions, (2) Snapshot formatted output or compare whole domain models directly, or (3) Parameterize test variations. Do NOT add accidental complexity with ad-hoc assertion helpers, extraction closures, filtering loops, or boolean tuples solely to reduce assertion count.",
        Python => "Refactor `{func}` by choosing the appropriate pattern: (1) Split distinct steps or behaviors into separate focused `test_*` functions, (2) Snapshot formatted output or compare whole domain models directly, or (3) Parameterize variations with `@pytest.mark.parametrize`. Do NOT add accidental complexity with ad-hoc assertion helpers, extraction closures, filtering loops, or boolean tuples solely to reduce assertion count.",
        Rust => "Refactor `{func}` by choosing the appropriate pattern: (1) Split distinct steps or behaviors into separate focused `#[test]` functions, (2) Snapshot formatted output (`insta::assert_snapshot!`) or compare whole domain models directly, or (3) Parameterize test cases with `#[rstest]` and `#[case(...)]`. Do NOT add accidental complexity with ad-hoc assertion helpers, extraction closures, filtering loops, or boolean tuples solely to reduce assertion count.",
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
        let rule_config: MaxTestAssertionsConfig = config.get_rule_config(self.name().0);
        let max_allowed = rule_config.effective_max_for_lang(lang, &DEFAULT_MAX_ASSERTIONS);

        crate::code_lint::collect_test_functions(&grep.root(), lang)
            .iter()
            .filter_map(|func_node| check_test_function(self, func_node, lang, path, max_allowed))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::RuleName;
    use crate::test_utils::{assert_code_rule_snapshot, assert_code_rule_snapshot_with_config};

    #[test]
    fn test_python_max_assertions_flagged() {
        let source = r"
import pytest

def assert_helper(response):
    assert response.status == 200
    assert response.body is not None
    assert response.headers
    assert response.cookies
    assert response.ok

def test_focused_behavior():
    assert 1 + 1 == 2
    assert 2 + 2 == 4
    with pytest.raises(ValueError):
        int('bad')
    assert True

def test_kitchen_sink_endpoint(self, mock_service):
    assert 1 == 1
    self.assertEqual(2, 2)
    mock_service.assert_called_once()
    with pytest.raises(KeyError):
        {}['missing']
    assert 5 == 5
";

        insta::assert_snapshot!(
            assert_code_rule_snapshot(&MaxTestAssertions, source, "tests/test_api.py"),
            @"[max-test-assertions] Line 18, Col 5: Test function `test_kitchen_sink_endpoint` has 5 assertions, exceeding the maximum of 4."
        );
    }

    #[test]
    fn test_rust_max_assertions_flagged() {
        let source = r#"
#[test]
fn parses_valid_header() {
    assert_eq!(1, 1);
    assert_ne!(1, 2);
    assert!(true);
    debug_assert_eq!(3, 3);
    insta::assert_snapshot!("ok");
}

#[tokio::test]
async fn focused_async_check() {
    assert_eq!(1, 1);
    assert!(matches!(Some(1), Some(_)));
}

#[cfg(test)]
fn verify_response_fields() {
    assert_eq!(1, 1);
    assert_eq!(2, 2);
    assert_eq!(3, 3);
    assert_eq!(4, 4);
    assert_eq!(5, 5);
}
"#;

        insta::assert_snapshot!(
            assert_code_rule_snapshot(&MaxTestAssertions, source, "tests/header_test.rs"),
            @"[max-test-assertions] Line 3, Col 4: Test function `parses_valid_header` has 5 assertions, exceeding the maximum of 4."
        );
    }

    #[test]
    fn test_configurable_threshold_and_suppression() {
        let source = r"
def test_three_assertions():
    assert 1 == 1
    assert 2 == 2
    assert 3 == 3

# omni:ignore [max-test-assertions] -- end-to-end state transition check
def test_suppressed_assertions():
    assert 1 == 1
    assert 2 == 2
    assert 3 == 3
";
        let config_toml = r"
[rules.max-test-assertions]
max = 2
";
        let config: Config = toml::from_str(config_toml).unwrap();
        let output = assert_code_rule_snapshot_with_config(
            &MaxTestAssertions,
            source,
            "tests/test_custom.py",
            &config,
        );
        insta::assert_snapshot!(
            output,
            @"
        [max-test-assertions] Line 2, Col 5: Test function `test_three_assertions` has 3 assertions, exceeding the maximum of 2.
        [max-test-assertions] Line 8, Col 5: Test function `test_suppressed_assertions` has 3 assertions, exceeding the maximum of 2.
        "
        );

        let diags = crate::code_lint::lint_file(Path::new("tests/test_custom.py"), source, &config);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_name, RuleName("max-test-assertions"));
    }
}
