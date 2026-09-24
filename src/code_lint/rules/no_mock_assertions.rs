//! Bans mock interaction assertions (`assert_called_once`, etc.) in tests.

use crate::code_lint::{CodeRule, RuleTarget};
use crate::core::{FilterListDefaults, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Static defaults for banned mock interaction assertion methods.
const DEFAULT_BANNED_METHODS: FilterListDefaults = FilterListDefaults {
    base: &[
        "$OBJ.assert_called",
        "$OBJ.assert_called_once",
        "$OBJ.assert_called_with",
        "$OBJ.assert_called_once_with",
        "$OBJ.assert_any_call",
        "$OBJ.assert_has_calls",
        "$OBJ.assert_not_called",
        "$OBJ.assert_awaited",
        "$OBJ.assert_awaited_once",
        "$OBJ.assert_awaited_with",
        "$OBJ.assert_awaited_once_with",
        "$OBJ.assert_any_await",
        "$OBJ.assert_has_awaits",
        "$OBJ.assert_not_awaited",
    ],
    extend: &[],
    exempt: &[],
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Mock interaction assertion `{callee}(...)` in test.",
    rationale: "Asserting on mock call counts or argument lists (`assert_called*`) couples tests to internal implementation wiring rather than observable behavior.",
    suggestion: "Assert on returned values or observable state transitions on an in-memory Fake.",
};

/// Rule that bans mock interaction assertions in test files.
pub struct NoMockAssertions;

impl Rule for NoMockAssertions {
    fn name(&self) -> RuleName {
        RuleName("no-mock-assertions")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Testing, Tag::Opinionated]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeRule for NoMockAssertions {
    fn target(&self) -> RuleTarget {
        RuleTarget::TestsOnly
    }

    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<crate::code_lint::SourceDoc>,
        config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        self.check_banned_calls(path, grep, config, &DEFAULT_BANNED_METHODS)
    }
}

#[cfg(test)]
crate::rule_test!(
    NoMockAssertions,
    {
        Python => {
            pass: [
                state_assertion_on_fake => r#"
                    def test_payment_flow(fake_gateway, fake_repo):
                        fake_gateway.charge(100)
                        assert fake_repo.balance == 100
                "#,
                return_value_assertion => r#"
                    def test_calculation():
                        result = compute_total([10, 20])
                        assert result == 30
                "#,
                unrelated_method_call => r#"
                    def test_custom_assertion(verifier):
                        verifier.assert_valid_state()
                "#,
            ],
            fail: [
                assert_called_once_with => r#"
                    def test_payment_flow(gateway):
                        gateway.charge.assert_called_once_with(100)
                "# => r#"gateway.charge.assert_called_once_with(100)"#,
                assert_not_called => r#"
                    def test_payment_flow(gateway):
                        gateway.refund.assert_not_called()
                "# => r#"gateway.refund.assert_not_called()"#,
                assert_has_calls => r#"
                    def test_payment_flow(mock_obj):
                        mock_obj.assert_has_calls([])
                "# => r#"mock_obj.assert_has_calls([])"#,
                assert_awaited_once => r#"
                    async def test_async_send(gateway):
                        gateway.async_send.assert_awaited_once()
                "# => r#"gateway.async_send.assert_awaited_once()"#,
                assert_not_awaited => r#"
                    async def test_async_send(gateway):
                        gateway.fallback.assert_not_awaited()
                "# => r#"gateway.fallback.assert_not_awaited()"#,
            ],
        },
    }
);
