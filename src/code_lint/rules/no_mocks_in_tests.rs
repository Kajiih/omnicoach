//! Bans dynamic mocks and monkeypatching in tests in favor of state-based Fakes.

architecture_component!(CodeLintRules);

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::rule::{CodeRule, RuleTarget};
use crate::core::{FilterListDefaults, Rule, Tag};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
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
    summary: "Dynamic mock or monkeypatch call `{callee}(...)` in test.",
    rationale: "Dynamic mocks and monkeypatching (`unittest.mock`, `MagicMock`, `patch`, `monkeypatch`) couple tests to internal call wiring and continue passing even when real dependency signatures or contracts change.",
    suggestion: "Inject a lightweight in-memory Fake (e.g., `FakeRepository`, `FakeHttpClient`) implementing the target `Protocol`.",
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
        file: &ParsedFile,
        config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        self.check_banned_calls(path, file, config, &DEFAULT_BANNED_MOCKS)
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    NoMocksInTests,
    {
        Python => {
            pass: [
                state_based_fake_repository => r#"
                    class FakeUserRepository:
                        def __init__(self):
                            self.users = {}

                        def save(self, user):
                            self.users[user.id] = user

                    def test_register_user():
                        repo = FakeUserRepository()
                        register_user(repo, "alice@example.com")
                        assert "alice@example.com" in repo.users
                "#,
                http_client_patch_method => r#"
                    def test_http_patch_request(http_client):
                        response = http_client.patch("/users/1", json={"active": True})
                        assert response.status_code == 200
                "#,
            ],
            fail: [
                magic_mock_instantiation => r#"
                    from unittest.mock import MagicMock

                    def test_service():
                        client = MagicMock()
                "# => "MagicMock()",
                unittest_mock_patch_decorator => r#"
                    from unittest.mock import patch

                    @patch("app.service.load_config")
                    def test_fetch(mock_config):
                        pass
                "# => r#"patch("app.service.load_config")"#,
                unittest_mock_patch_context_manager => r#"
                    from unittest.mock import patch

                    def test_fetch():
                        with patch("app.service.fetch_data") as mock_fetch:
                            pass
                "# => r#"patch("app.service.fetch_data")"#,
                pytest_mocker_patch_object => r#"
                    def test_overrides(mocker):
                        mocker.patch.object(Notifier, "send")
                "# => r#"mocker.patch.object(Notifier, "send")"#,
                pytest_monkeypatch_setattr => r#"
                    def test_overrides(monkeypatch):
                        monkeypatch.setattr(settings, "DEBUG", True)
                "# => r#"monkeypatch.setattr(settings, "DEBUG", True)"#,
                qualified_unittest_mock => r#"
                    import unittest.mock

                    def test_service():
                        client = unittest.mock.Mock()
                "# => r#"unittest.mock.Mock()"#,
                qualified_mock => r#"
                    import mock

                    def test_service():
                        client = mock.Mock()
                "# => r#"mock.Mock()"#,
            ],
        },
    }
);
