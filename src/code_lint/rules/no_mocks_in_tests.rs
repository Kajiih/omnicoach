//! Bans dynamic mocks and monkeypatching in tests in favor of state-based Fakes.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::rule::{CodeDetector, RuleTarget};
use crate::core::{Detector, FilterListDefaults};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_documentation::{ConfigShape, Reference, RuleDoc};
use crate::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Topic};
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

impl NoMocksInTests {
    /// The rule's declared facets.
    pub(crate) const CLASSIFICATION: Classification = Classification {
        topics: &[Topic::TEST_DOUBLES],
        precision: Precision::Exact,
        consensus: Consensus::Opinionated,
        impacted_quality: ImpactedQuality::Reliability,
    };

    /// The rule's user-facing doc.
    pub(crate) const DOC: RuleDoc = RuleDoc {
        summary: "Flags dynamic mocks and monkeypatching in Python tests.",
        what_it_does: "Flags calls that create mocks or patch code at run time in Python \
                       test files: `Mock`, `MagicMock`, `AsyncMock`, `NonCallableMock`, \
                       `PropertyMock`, `create_autospec` and `patch` (with `patch.object`, \
                       `patch.dict` and `patch.multiple`), whether called bare or through \
                       `mock.` or `unittest.mock.`. It also flags pytest-mock's `mocker.*` \
                       equivalents plus `mocker.spy`, `mocker.stub` and `mocker.async_stub`, \
                       and pytest's `MonkeyPatch` and `monkeypatch.setattr`, `delattr`, \
                       `setitem` and `delitem`. Calls are matched by name, not by import: a \
                       bare `patch(...)` is flagged even when imported from another library, \
                       while a method such as `http_client.patch(...)` is not, and the \
                       `mocker` and `monkeypatch` calls are only matched under those exact \
                       names.",
        why_is_this_bad: "A mock replaces a real collaborator with an object that accepts \
                          any call and returns whatever the test told it to. Patching \
                          swaps code by its import path. Both tie the test to how the code \
                          is wired internally rather than to what it does, so refactors \
                          break tests that should pass, and tests keep passing when the \
                          real dependency changes its signature or behaviour.\n\n\
                          Pass dependencies in explicitly and use a fake in tests: a small \
                          working in-memory implementation of the same interface (for \
                          example a `FakeRepository` backed by a dict), then assert on its \
                          state.",
        configuration: &[ConfigShape::DenyList],
        references: &[
            Reference {
                title: "Software Engineering at Google, ch. 13: Test Doubles",
                url: "https://abseil.io/resources/swe-book/html/ch13.html",
            },
            Reference {
                title: "Mocks Aren't Stubs (Martin Fowler)",
                url: "https://martinfowler.com/articles/mocksArentStubs.html",
            },
        ],
    };
}

impl Detector for NoMocksInTests {
    fn name(&self) -> RuleName {
        RuleName("no-mocks-in-tests")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeDetector for NoMocksInTests {
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
