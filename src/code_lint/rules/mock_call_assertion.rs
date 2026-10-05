//! Bans mock interaction assertions (`assert_called_once`, etc.) in tests.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::collections::HashSet;
use std::path::Path;

const BANNED: ListOption = ListOption {
    kind: ListKind::Deny,
    doc: "Mock assertion methods flagged when called.",
    default: FilterListDefaults {
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
        remove: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Test asserts on mock calls with `{callee}()`.",
    rationale: "Asserting on call counts or argument lists ties the test to the implementation's wiring rather than to its observable behavior, so a harmless refactor fails the test and a wrong result can pass it.",
    suggestion: "Assert on the returned value or on the state of an in-memory fake.",
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("mock-call-assertion"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(BANNED),
        classification: Classification {
            topics: &[Topic::TEST_ASSERTIONS, Topic::TEST_DOUBLES],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags assertions on how a mock was called in Python tests.",
            what_it_does: indoc::indoc! {r"
                Flags calls to the `unittest.mock` interaction assertions in Python test files:
                `assert_called`, `assert_called_once`, `assert_called_with`,
                `assert_called_once_with`, `assert_any_call`, `assert_has_calls` and
                `assert_not_called`, and their `assert_awaited*` / `assert_any_await` /
                `assert_has_awaits` / `assert_not_awaited` counterparts. They are flagged on any
                object, whether or not it is a mock. Other methods whose names start with `assert_`,
                such as `verifier.assert_valid_state()`, are not flagged."},
            why_is_this_bad: indoc::indoc! {r"
                These assertions check that the code under test made particular calls, in a
                particular way, rather than that it produced the right result. The test then encodes
                the implementation: reordering, batching or replacing a call breaks it even when the
                behaviour is unchanged, and it can still pass when the outcome is wrong.

                Assert on what callers can observe: the returned value, or the resulting state of an
                in-memory fake standing in for the dependency."},
            references: &[Reference {
                title: "Software Engineering at Google, ch. 13: Test Doubles",
                url: "https://abseil.io/resources/swe-book/html/ch13.html",
            }],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r"
                    def test_checkout_charges_order_total(gateway):
                        checkout(gateway, order_total=100)
                        gateway.charge.assert_called_once_with(100)
                "},
                flagged_span: "gateway.charge.assert_called_once_with(100)",
                fixed: indoc::indoc! {r"
                    def test_checkout_charges_order_total(gateway):
                        checkout(gateway, order_total=100)
                        assert gateway.charges == [100]
                "},
            }],
        },
    },
    target: RuleTarget::TestsOnly,
    check: check_file,
};

fn check_file(
    rule: &CodeRule<ListOption>,
    path: &Path,
    file: &ParsedFile,
    banned: &HashSet<String>,
) -> Vec<Diagnostic> {
    rule.check_banned_calls(path, file, banned)
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
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
