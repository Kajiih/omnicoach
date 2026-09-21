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
        ".assert_called",
        ".assert_called_once",
        ".assert_called_with",
        ".assert_called_once_with",
        ".assert_any_call",
        ".assert_has_calls",
        ".assert_not_called",
        ".assert_awaited",
        ".assert_awaited_once",
        ".assert_awaited_with",
        ".assert_awaited_once_with",
        ".assert_any_await",
        ".assert_has_awaits",
        ".assert_not_awaited",
    ],
    extend: &[],
    exempt: &[],
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Mock interaction assertion `.{callee}(...)` is prohibited in tests.",
    rationale: "Asserting that a mock method was invoked with specific arguments tests internal implementation details rather than observable outputs and state transitions.",
    suggestion: "Assert on the returned value, state changes on an in-memory Fake, or observable domain outcomes instead of `.{callee}(...)`.",
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
mod tests {
    use super::*;
    use crate::test_utils::assert_code_rule_snapshot;

    #[test]
    fn test_python_mock_assertions() {
        let rule = NoMockAssertions;

        // TODO: Is it the idiomatic way to write multiline strings?
        let source = r"
def test_payment_flow(gateway, fake_repo):
    gateway.charge.assert_called_once_with(100)
    gateway.refund.assert_not_called()
    gateway.async_send.assert_awaited_once()
    assert fake_repo.balance == 100
";
        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, source, "test_pay.py"), @"
        [no-mock-assertions] Line 3, Col 5: Mock interaction assertion `.assert_called_once_with(...)` is prohibited in tests.
        [no-mock-assertions] Line 4, Col 5: Mock interaction assertion `.assert_not_called(...)` is prohibited in tests.
        [no-mock-assertions] Line 5, Col 5: Mock interaction assertion `.assert_awaited_once(...)` is prohibited in tests.
        ");
    }
}
