//! Bans dynamic mocks and monkeypatching in tests in favor of state-based Fakes.

use crate::code_lint::{CodeRule, RuleTarget};
use crate::core::{FilterListDefaults, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Static defaults for banned mock and monkeypatching functions in tests.
const DEFAULT_BANNED_MOCKS: FilterListDefaults = FilterListDefaults {
    base: &[
        // Standalone unittest.mock symbols
        "Mock",
        "MagicMock",
        "AsyncMock",
        "NonCallableMock",
        "PropertyMock",
        "create_autospec",
        "patch",
        "patch.object",
        "patch.dict",
        "patch.multiple",
        // Qualified mock.* symbols
        "mock.Mock",
        "mock.MagicMock",
        "mock.AsyncMock",
        "mock.NonCallableMock",
        "mock.PropertyMock",
        "mock.create_autospec",
        "mock.patch",
        "mock.patch.object",
        "mock.patch.dict",
        "mock.patch.multiple",
        // Qualified unittest.mock.* symbols
        "unittest.mock.Mock",
        "unittest.mock.MagicMock",
        "unittest.mock.AsyncMock",
        "unittest.mock.NonCallableMock",
        "unittest.mock.PropertyMock",
        "unittest.mock.create_autospec",
        "unittest.mock.patch",
        "unittest.mock.patch.object",
        "unittest.mock.patch.dict",
        "unittest.mock.patch.multiple",
        // pytest-mock (mocker.*) symbols
        "mocker.Mock",
        "mocker.MagicMock",
        "mocker.AsyncMock",
        "mocker.NonCallableMock",
        "mocker.PropertyMock",
        "mocker.create_autospec",
        "mocker.patch",
        "mocker.patch.object",
        "mocker.patch.dict",
        "mocker.patch.multiple",
        "mocker.spy",
        "mocker.stub",
        "mocker.async_stub",
        // pytest monkeypatch symbols
        "MonkeyPatch",
        "pytest.MonkeyPatch",
        "monkeypatch.setattr",
        "monkeypatch.delattr",
        "monkeypatch.setitem",
        "monkeypatch.delitem",
    ],
    extend: &[],
    exempt: &[],
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Dynamic mock or monkeypatch `{callee}(...)` is prohibited in tests.",
    rationale: "Dynamic mocks and monkeypatching couple tests to internal implementation details, mask interface design flaws, and break during refactoring.",
    suggestion: "Replace `{callee}` with a state-based in-memory Fake (e.g. `FakeRepository`, `FakeHttpClient`) that explicitly implements the target Protocol or interface.",
};

/// Rule that bans dynamic mocks and monkeypatching in test files.
pub struct NoMocksInTests;

impl Rule for NoMocksInTests {
    fn name(&self) -> RuleName {
        RuleName("no-mocks-in-tests")
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

impl CodeRule for NoMocksInTests {
    fn target(&self) -> RuleTarget {
        RuleTarget::TestsOnly
    }

    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<crate::code_lint::SourceDoc>,
        config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        self.check_banned_calls(path, grep, config, &DEFAULT_BANNED_MOCKS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_code_rule_snapshot;

    #[test]
    fn test_python_mock_detection_and_http_patch_exemption() {
        let rule = NoMocksInTests;

        let source = r#"
from unittest.mock import MagicMock, patch

@patch("service.auth.verify_token")
def test_user_update(mocker, monkeypatch, api_client):
    gateway = MagicMock()
    mocker.patch.object(gateway, "charge")
    monkeypatch.setattr(gateway, "timeout", 5)
    # Real HTTP PATCH calls must NOT be flagged:
    response = api_client.patch("/v1/users/42", json={"name": "Alice"})
    httpx.patch("https://example.com/api")
"#;
        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, source, "test_auth.py"), @"
        [no-mocks-in-tests] Line 4, Col 2: Dynamic mock or monkeypatch `patch(...)` is prohibited in tests.
        [no-mocks-in-tests] Line 6, Col 15: Dynamic mock or monkeypatch `MagicMock(...)` is prohibited in tests.
        [no-mocks-in-tests] Line 7, Col 5: Dynamic mock or monkeypatch `mocker.patch.object(...)` is prohibited in tests.
        [no-mocks-in-tests] Line 8, Col 5: Dynamic mock or monkeypatch `monkeypatch.setattr(...)` is prohibited in tests.
        ");
    }
}
