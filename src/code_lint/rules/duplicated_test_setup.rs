//! Flags sibling test functions that repeat the same leading setup statements.

use crate::code_lint::ast::{self, ParsedFile, TestFunctionSetup};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, CountOption, Declaration, Example, ImpactedQuality,
    LanguageDefaults, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::collections::HashMap;
use std::path::Path;

const MIN_TESTS: CountOption = CountOption {
    key: "min-tests",
    doc: "Minimum test functions in a scope sharing the same leading setup prefix to be flagged. \
          Values below 2 behave as 2.",
    default: LanguageDefaults::new(3, &[]),
};

const MIN_STATEMENTS: CountOption = CountOption {
    key: "min-statements",
    doc: "Minimum leading non-assertion setup statements shared across test functions to be \
          flagged. Values below 1 behave as 1.",
    default: LanguageDefaults::new(3, &[]),
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Test `{function}` duplicates {count} leading setup statements with {duplicates}.",
    rationale: "Repeating the same arrange statements across sibling tests obscures which inputs differ in each scenario and requires updating every copy when construction changes.",
    suggestion: {
        Python => "Extract shared immutable values into module constants, or extract mutable setup into a helper function or `@pytest.fixture` and pass only the scenario-specific inputs in each test.",
        Rust => "Extract shared immutable values into `const` items, or extract mutable setup into a helper function and pass only the scenario-specific inputs in each test.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule<(CountOption, CountOption)> = CodeRule {
    declaration: Declaration {
        name: RuleName("duplicated-test-setup"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule((MIN_TESTS, MIN_STATEMENTS)),
        classification: Classification {
            topics: &[Topic::TESTING],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags sibling test functions that repeat the same leading setup statements.",
            what_it_does: indoc::indoc! {r"
                Groups direct test functions in each module, Python `class`, or Rust `mod` scope
                and extracts each test's leading non-assertion setup statements (skipping a
                leading Python docstring and nested function, class, or Rust local item
                declarations).

                Flags the first test function in a group when at least `min-tests` test functions
                in the same scope begin with the same `min-statements` (or more) setup
                statements."},
            why_is_this_bad: indoc::indoc! {r"
                When several tests begin with the same block of boilerplate construction, readers
                have to compare lines character by character to see what actually differs between
                scenarios, and any constructor or schema change breaks every copy at once.

                Extract shared immutable values into module constants, or extract a creation helper
                or fixture with sensible defaults so each test passes only the values relevant to
                the behavior it verifies."},
            known_problems: Some(indoc::indoc! {r"
                - Only identical leading statements are matched; setup statements that appear in a
                  different order or use different local variable names are not grouped together.
                - Imported helper calls that perform assertions without an `assert_` or `_assert_`
                  prefix are classified as setup statements."}),
            references: &[
                Reference {
                    title: "Software Engineering at Google, ch. 12: Shared Setup",
                    url: "https://abseil.io/resources/swe-book/html/ch12.html",
                },
                Reference {
                    title: "xUnit Test Patterns: Creation Method",
                    url: "http://xunitpatterns.com/Creation%20Method.html",
                },
            ],
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r#"
                        def test_admin_can_delete():
                            db = InMemoryDatabase()
                            repo = UserRepository(db)
                            service = AuthService(repo)
                            assert service.can_delete("admin")

                        def test_guest_cannot_delete():
                            db = InMemoryDatabase()
                            repo = UserRepository(db)
                            service = AuthService(repo)
                            assert not service.can_delete("guest")

                        def test_editor_cannot_delete():
                            db = InMemoryDatabase()
                            repo = UserRepository(db)
                            service = AuthService(repo)
                            assert not service.can_delete("editor")
                    "#},
                    flagged_span: "test_admin_can_delete",
                    fixed: indoc::indoc! {r#"
                        def make_auth_service() -> AuthService:
                            db = InMemoryDatabase()
                            repo = UserRepository(db)
                            return AuthService(repo)

                        def test_admin_can_delete():
                            service = make_auth_service()
                            assert service.can_delete("admin")

                        def test_guest_cannot_delete():
                            service = make_auth_service()
                            assert not service.can_delete("guest")

                        def test_editor_cannot_delete():
                            service = make_auth_service()
                            assert not service.can_delete("editor")
                    "#},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r#"
                        #[test]
                        fn test_admin_can_delete() {
                            let db = InMemoryDatabase::new();
                            let repo = UserRepository::new(db);
                            let service = AuthService::new(repo);
                            assert!(service.can_delete("admin"));
                        }

                        #[test]
                        fn test_guest_cannot_delete() {
                            let db = InMemoryDatabase::new();
                            let repo = UserRepository::new(db);
                            let service = AuthService::new(repo);
                            assert!(!service.can_delete("guest"));
                        }

                        #[test]
                        fn test_editor_cannot_delete() {
                            let db = InMemoryDatabase::new();
                            let repo = UserRepository::new(db);
                            let service = AuthService::new(repo);
                            assert!(!service.can_delete("editor"));
                        }
                    "#},
                    flagged_span: "test_admin_can_delete",
                    fixed: indoc::indoc! {r#"
                        fn make_auth_service() -> AuthService {
                            let db = InMemoryDatabase::new();
                            let repo = UserRepository::new(db);
                            AuthService::new(repo)
                        }

                        #[test]
                        fn test_admin_can_delete() {
                            let service = make_auth_service();
                            assert!(service.can_delete("admin"));
                        }

                        #[test]
                        fn test_guest_cannot_delete() {
                            let service = make_auth_service();
                            assert!(!service.can_delete("guest"));
                        }

                        #[test]
                        fn test_editor_cannot_delete() {
                            let service = make_auth_service();
                            assert!(!service.can_delete("editor"));
                        }
                    "#},
                },
            ],
        },
    },
    target: RuleTarget::TestsOnly,
    check: check_file,
};

fn check_file(
    rule: &CodeRule<(CountOption, CountOption)>,
    path: &Path,
    file: &ParsedFile,
    (min_tests, min_statements): (usize, usize),
) -> Vec<Diagnostic> {
    let effective_min_tests = min_tests.max(2);
    let effective_min_statements = min_statements.max(1);
    let mut diagnostics = Vec::new();

    for scope in ast::collect_test_setup_scopes(file) {
        let mut index_by_prefix: HashMap<Vec<String>, usize> = HashMap::new();
        let mut groups: Vec<Vec<TestFunctionSetup<'_>>> = Vec::new();
        for func in scope {
            if func.leading_setup_statements.len() < effective_min_statements {
                continue;
            }
            let prefix = func.leading_setup_statements[..effective_min_statements].to_vec();
            let group_idx = *index_by_prefix.entry(prefix).or_insert_with(|| {
                groups.push(Vec::new());
                groups.len() - 1
            });
            groups[group_idx].push(func);
        }

        for group in groups {
            if group.len() < effective_min_tests {
                continue;
            }
            let shared_count = shared_prefix_length(&group);
            let duplicates = group[1..]
                .iter()
                .map(|func| format!("`{}`", func.name))
                .collect::<Vec<_>>()
                .join(", ");
            diagnostics.push(rule.diagnostic_at_node(
                path,
                &group[0].name_node,
                &[
                    ("function", &group[0].name),
                    ("count", &shared_count.to_string()),
                    ("duplicates", &duplicates),
                ],
            ));
        }
    }

    diagnostics
}

fn shared_prefix_length(group: &[TestFunctionSetup<'_>]) -> usize {
    let Some(first) = group.first() else {
        return 0;
    };
    first
        .leading_setup_statements
        .iter()
        .enumerate()
        .take_while(|(idx, statement)| {
            group[1..].iter().all(|other| {
                other
                    .leading_setup_statements
                    .get(*idx)
                    .is_some_and(|candidate| candidate == *statement)
            })
        })
        .count()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                below_min_tests_threshold_allowed => r#"
                    def test_admin_can_delete():
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        service = AuthService(repo)
                        assert service.can_delete("admin")

                    def test_guest_cannot_delete():
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        service = AuthService(repo)
                        assert not service.can_delete("guest")
                "#,
                below_min_statements_threshold_allowed => r#"
                    def test_admin_role():
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        user = repo.create("admin")
                        assert user.is_admin

                    def test_guest_role():
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        user = repo.create("guest")
                        assert not user.is_admin

                    def test_editor_role():
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        user = repo.create("editor")
                        assert not user.is_admin
                "#,
                helper_function_exempt => r"
                    def build_alpha():
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        service = AuthService(repo)
                        return service

                    def build_beta():
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        service = AuthService(repo)
                        return service

                    def build_gamma():
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        service = AuthService(repo)
                        return service
                ",
                leading_docstring_not_counted_as_setup => r#"
                    def test_first():
                        """Shared test docstring."""
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        assert repo.count() == 0

                    def test_second():
                        """Shared test docstring."""
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        assert repo.is_empty()

                    def test_third():
                        """Shared test docstring."""
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        assert repo.all() == []
                "#,
                nested_function_not_counted_as_setup => r"
                    def test_first():
                        def helper():
                            return 1
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        assert repo.count() == 0

                    def test_second():
                        def helper():
                            return 1
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        assert repo.is_empty()

                    def test_third():
                        def helper():
                            return 1
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        assert repo.all() == []
                ",
                nested_class_not_counted_as_setup => r"
                    def test_first():
                        class Stub:
                            pass
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        assert repo.count() == 0

                    def test_second():
                        class Stub:
                            pass
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        assert repo.is_empty()

                    def test_third():
                        class Stub:
                            pass
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        assert repo.all() == []
                ",
                assertions_not_counted_as_setup => r"
                    def test_first():
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        assert repo.count() == 0

                    def test_second():
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        assert repo.count() == 0

                    def test_third():
                        db = InMemoryDatabase()
                        repo = UserRepository(db)
                        assert repo.count() == 0
                ",
                separate_test_classes_do_not_pool_across_scopes => r#"
                    class TestAdminAuth:
                        def test_can_delete(self):
                            db = InMemoryDatabase()
                            repo = UserRepository(db)
                            service = AuthService(repo)
                            assert service.can_delete("admin")

                        def test_can_create(self):
                            db = InMemoryDatabase()
                            repo = UserRepository(db)
                            service = AuthService(repo)
                            assert service.can_create("admin")

                    class TestGuestAuth:
                        def test_cannot_delete(self):
                            db = InMemoryDatabase()
                            repo = UserRepository(db)
                            service = AuthService(repo)
                            assert not service.can_delete("guest")
                "#,
            ],
            fail: [
                three_class_test_methods_sharing_setup => r#"
                    class TestOrderService:
                        def test_submit_order(self):
                            clock = FakeClock()
                            ledger = InMemoryLedger()
                            service = OrderService(clock, ledger)
                            assert service.submit("item")

                        def test_cancel_order(self):
                            clock = FakeClock()
                            ledger = InMemoryLedger()
                            service = OrderService(clock, ledger)
                            assert service.cancel("item")

                        def test_refund_order(self):
                            clock = FakeClock()
                            ledger = InMemoryLedger()
                            service = OrderService(clock, ledger)
                            assert service.refund("item")
                "# => "test_submit_order",
                narrowing_guard_included_in_setup_prefix => r#"
                    def test_profile_email(repo):
                        user = repo.find_by_id("u1")
                        assert user is not None
                        profile = user.load_profile()
                        assert profile.email == "a@example.com"

                    def test_profile_name(repo):
                        user = repo.find_by_id("u1")
                        assert user is not None
                        profile = user.load_profile()
                        assert profile.name == "Alice"

                    def test_profile_active(repo):
                        user = repo.find_by_id("u1")
                        assert user is not None
                        profile = user.load_profile()
                        assert profile.is_active
                "# => "test_profile_email",
            ],
        },
        Rust => {
            pass: [
                below_min_tests_threshold_allowed => r#"
                    #[test]
                    fn test_admin_can_delete() {
                        let db = InMemoryDatabase::new();
                        let repo = UserRepository::new(db);
                        let service = AuthService::new(repo);
                        assert!(service.can_delete("admin"));
                    }

                    #[test]
                    fn test_guest_cannot_delete() {
                        let db = InMemoryDatabase::new();
                        let repo = UserRepository::new(db);
                        let service = AuthService::new(repo);
                        assert!(!service.can_delete("guest"));
                    }
                "#,
                below_min_statements_threshold_allowed => r#"
                    #[test]
                    fn test_admin_role() {
                        let db = InMemoryDatabase::new();
                        let repo = UserRepository::new(db);
                        let user = repo.create("admin");
                        assert!(user.is_admin());
                    }

                    #[test]
                    fn test_guest_role() {
                        let db = InMemoryDatabase::new();
                        let repo = UserRepository::new(db);
                        let user = repo.create("guest");
                        assert!(!user.is_admin());
                    }

                    #[test]
                    fn test_editor_role() {
                        let db = InMemoryDatabase::new();
                        let repo = UserRepository::new(db);
                        let user = repo.create("editor");
                        assert!(!user.is_admin());
                    }
                "#,
                helper_function_exempt => r"
                    fn build_alpha() -> AuthService {
                        let db = InMemoryDatabase::new();
                        let repo = UserRepository::new(db);
                        let service = AuthService::new(repo);
                        service
                    }

                    fn build_beta() -> AuthService {
                        let db = InMemoryDatabase::new();
                        let repo = UserRepository::new(db);
                        let service = AuthService::new(repo);
                        service
                    }

                    fn build_gamma() -> AuthService {
                        let db = InMemoryDatabase::new();
                        let repo = UserRepository::new(db);
                        let service = AuthService::new(repo);
                        service
                    }
                ",
                local_item_declarations_not_counted_as_setup => r"
                    #[test]
                    fn test_first() {
                        struct Dummy;
                        let db = InMemoryDatabase::new();
                        let repo = UserRepository::new(db);
                        assert_eq!(repo.count(), 0);
                    }

                    #[test]
                    fn test_second() {
                        struct Dummy;
                        let db = InMemoryDatabase::new();
                        let repo = UserRepository::new(db);
                        assert!(repo.is_empty());
                    }

                    #[test]
                    fn test_third() {
                        struct Dummy;
                        let db = InMemoryDatabase::new();
                        let repo = UserRepository::new(db);
                        assert_eq!(repo.items().len(), 0);
                    }
                ",
                assertions_not_counted_as_setup => r"
                    #[test]
                    fn test_first() {
                        let db = InMemoryDatabase::new();
                        let repo = UserRepository::new(db);
                        assert_eq!(repo.count(), 0);
                    }

                    #[test]
                    fn test_second() {
                        let db = InMemoryDatabase::new();
                        let repo = UserRepository::new(db);
                        assert_eq!(repo.count(), 0);
                    }

                    #[test]
                    fn test_third() {
                        let db = InMemoryDatabase::new();
                        let repo = UserRepository::new(db);
                        assert_eq!(repo.count(), 0);
                    }
                ",
                separate_test_modules_do_not_pool_across_scopes => r#"
                    mod admin_tests {
                        #[test]
                        fn test_can_delete() {
                            let db = InMemoryDatabase::new();
                            let repo = UserRepository::new(db);
                            let service = AuthService::new(repo);
                            assert!(service.can_delete("admin"));
                        }

                        #[test]
                        fn test_can_create() {
                            let db = InMemoryDatabase::new();
                            let repo = UserRepository::new(db);
                            let service = AuthService::new(repo);
                            assert!(service.can_create("admin"));
                        }
                    }

                    mod guest_tests {
                        #[test]
                        fn test_cannot_delete() {
                            let db = InMemoryDatabase::new();
                            let repo = UserRepository::new(db);
                            let service = AuthService::new(repo);
                            assert!(!service.can_delete("guest"));
                        }
                    }
                "#,
            ],
            fail: [
                three_module_tests_sharing_setup => r#"
                    mod order_tests {
                        #[test]
                        fn test_submit_order() {
                            let clock = FakeClock::new();
                            let ledger = InMemoryLedger::new();
                            let service = OrderService::new(clock, ledger);
                            assert!(service.submit("item"));
                        }

                        #[test]
                        fn test_cancel_order() {
                            let clock = FakeClock::new();
                            let ledger = InMemoryLedger::new();
                            let service = OrderService::new(clock, ledger);
                            assert!(service.cancel("item"));
                        }

                        #[test]
                        fn test_refund_order() {
                            let clock = FakeClock::new();
                            let ledger = InMemoryLedger::new();
                            let service = OrderService::new(clock, ledger);
                            assert!(service.refund("item"));
                        }
                    }
                "# => "test_submit_order",
            ],
        }
    }
);
