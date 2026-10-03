//! Bans dynamic mocks and monkeypatching in tests in favor of state-based Fakes.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::collections::HashSet;
use std::path::Path;

const BANNED: ListOption = ListOption {
    kind: ListKind::Deny,
    doc: "Mocking and monkeypatching calls flagged in tests.",
    default: FilterListDefaults {
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
        remove: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Test mocks or patches with `{callee}()`.",
    rationale: "A dynamic mock or patch couples the test to the internal call wiring and keeps passing when the real dependency's signature or contract changes.",
    suggestion: "Inject an in-memory fake (such as `FakeRepository` or `FakeHttpClient`) that implements the dependency's `Protocol`.",
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("mock-in-tests"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python],
        options: RuleOptions::code_rule(BANNED),
        classification: Classification {
            topics: &[Topic::TEST_DOUBLES],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
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
            examples: &[Example {
                language: SupportLang::Python,
                flagged: indoc::indoc! {r#"
                    def test_register_user():
                        repository = MagicMock()
                        register_user(repository, "alice@example.com")
                        assert repository.save.called
                "#},
                flagged_span: "MagicMock()",
                fixed: indoc::indoc! {r#"
                    def test_register_user():
                        repository = FakeUserRepository()
                        register_user(repository, "alice@example.com")
                        assert "alice@example.com" in repository.users
                "#},
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
                state_based_fake_repository => r#"
                    class FakeUserRepository(UserRepository):
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
