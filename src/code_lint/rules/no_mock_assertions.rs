//! Bans mock interaction assertions (`assert_called_once`, etc.) in tests.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::rule::{CodeDetector, RuleTarget};
use crate::core::{Detector, FilterListDefaults};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_documentation::{ConfigShape, Reference, RuleDoc};
use crate::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Topic};
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

impl NoMockAssertions {
    /// The rule's declared facets (ADR 007).
    pub(crate) const CLASSIFICATION: Classification = Classification {
        topics: &[Topic::TEST_ASSERTIONS, Topic::TEST_DOUBLES],
        precision: Precision::Exact,
        consensus: Consensus::Opinionated,
        impacted_quality: ImpactedQuality::Maintainability,
    };

    /// The rule's user-facing doc.
    pub(crate) const DOC: RuleDoc = RuleDoc {
        summary: "Flags assertions on how a mock was called in Python tests.",
        what_it_does: "Flags calls to the `unittest.mock` interaction assertions in Python \
                       test files: `assert_called`, `assert_called_once`, \
                       `assert_called_with`, `assert_called_once_with`, `assert_any_call`, \
                       `assert_has_calls` and `assert_not_called`, and their `assert_awaited*` \
                       / `assert_any_await` / `assert_has_awaits` / `assert_not_awaited` \
                       counterparts. They are flagged on any object, whether or not it is a \
                       mock. Other methods whose names start with `assert_`, such as \
                       `verifier.assert_valid_state()`, are not flagged.",
        why_is_this_bad: "These assertions check that the code under test made particular \
                          calls, in a particular way, rather than that it produced the right \
                          result. The test then encodes the implementation: reordering, \
                          batching or replacing a call breaks it even when the behaviour is \
                          unchanged, and it can still pass when the outcome is wrong.\n\n\
                          Assert on what callers can observe: the returned value, or the \
                          resulting state of an in-memory fake standing in for the \
                          dependency.",
        configuration: &[ConfigShape::DenyList],
        references: &[Reference {
            title: "Software Engineering at Google, ch. 13: Test Doubles",
            url: "https://abseil.io/resources/swe-book/html/ch13.html",
        }],
    };
}

impl Detector for NoMockAssertions {
    fn name(&self) -> RuleName {
        RuleName("no-mock-assertions")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeDetector for NoMockAssertions {
    fn target(&self) -> RuleTarget {
        RuleTarget::TestsOnly
    }

    fn check_file(
        &self,
        path: &Path,
        file: &ParsedFile,
        config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        self.check_banned_calls(path, file, config, &DEFAULT_BANNED_METHODS)
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
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
