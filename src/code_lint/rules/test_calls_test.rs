//! Flags calls from a test or test helper to another test function in the same file.

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "`{caller}` calls test function `{callee}()`.",
    rationale: "Calling a test function from another test or helper couples their outcomes, runs the callee's assertions again inside the caller, and bypasses fixture and teardown lifecycles.",
    suggestion: {
        Python => "Extract the shared steps into a non-test helper function or `@pytest.fixture` and use it in each test.",
        Rust => "Extract the shared steps into a helper function without `#[test]` and call it from each test.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("test-calls-test"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::TESTING],
            precision: Precision::Exact,
            consensus: Consensus::Unopinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags functions in test files that call another test function.",
            what_it_does: indoc::indoc! {r"
                Flags calls inside a test file where a test or helper function invokes another test
                function defined in the same file: a module-level `def test_*` or test-class
                (`Test*` or `*TestCase`) method `self.test_*` / `cls.test_*` in Python, or a
                function annotated with `#[test]`, `#[tokio::test]`, `#[rstest]`, or `#[test_case]`
                in Rust.

                Recursive self-calls are left to `logic-in-tests`."},
            why_is_this_bad: indoc::indoc! {r"
                Each test function is an independent scenario managed by the test runner. When
                `test_delete_user` calls `test_create_user()` directly to reuse its state, any
                assertion failure inside `test_create_user` is reported twice, fixtures and
                teardown hooks bound to `test_create_user` are bypassed, and changing one test's
                assertions silently changes the preconditions of every test that calls it.

                Extract shared setup or actions into a plain helper function (or a
                `@pytest.fixture` in Python) and have each test call that helper."},
            known_problems: Some(indoc::indoc! {r"
                Calls to test functions imported from another file and, in Rust, calls inside macro
                arguments (`assert!(test_seed())`) are not flagged."}),
            references: &[
                Reference {
                    title: "Software Engineering at Google, ch. 12: Unit Testing",
                    url: "https://abseil.io/resources/swe-book/html/ch12.html",
                },
                Reference {
                    title: "xUnit Test Patterns: Chained Tests",
                    url: "http://xunitpatterns.com/Chained%20Tests.html",
                },
                Reference {
                    title: "pytest Good Practices: Conventions for Python Test Discovery",
                    url: "https://docs.pytest.org/en/stable/explanation/goodpractices.html#conventions-for-python-test-discovery",
                },
            ],
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r#"
                        def test_creates_user(db: Database) -> User:
                            user = db.insert_user("alice")
                            assert user.name == "alice"
                            return user

                        def test_deletes_user(db: Database) -> None:
                            user = test_creates_user(db)
                            db.delete_user(user.id)
                            assert db.find_user(user.id) is None
                    "#},
                    flagged_span: "test_creates_user(db)",
                    fixed: indoc::indoc! {r#"
                        def seed_user(db: Database) -> User:
                            return db.insert_user("alice")

                        def test_creates_user(db: Database) -> None:
                            user = seed_user(db)
                            assert user.name == "alice"

                        def test_deletes_user(db: Database) -> None:
                            user = seed_user(db)
                            db.delete_user(user.id)
                            assert db.find_user(user.id) is None
                    "#},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r#"
                        #[test]
                        fn test_creates_user() {
                            let db = Database::in_memory();
                            let user = db.insert_user("alice");
                            assert_eq!(user.name, "alice");
                        }

                        #[test]
                        fn test_deletes_user() {
                            test_creates_user();
                            let db = Database::in_memory();
                            assert!(db.find_user("alice").is_none());
                        }
                    "#},
                    flagged_span: "test_creates_user()",
                    fixed: indoc::indoc! {r#"
                        fn seed_user(db: &Database) -> User {
                            db.insert_user("alice")
                        }

                        #[test]
                        fn test_creates_user() {
                            let db = Database::in_memory();
                            let user = seed_user(&db);
                            assert_eq!(user.name, "alice");
                        }

                        #[test]
                        fn test_deletes_user() {
                            let db = Database::in_memory();
                            let user = seed_user(&db);
                            db.delete_user(user.id);
                            assert!(db.find_user(user.id).is_none());
                        }
                    "#},
                },
            ],
        },
    },
    target: RuleTarget::TestsOnly,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    ast::collect_test_to_test_calls(file)
        .into_iter()
        .map(|call| {
            rule.diagnostic_at_node(
                path,
                &call.node,
                &[("caller", &call.caller), ("callee", &call.callee)],
            )
        })
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                shared_non_test_helper => r#"
                    def seed_admin_session() -> str:
                        return "admin-token"

                    def test_admin_can_list_users() -> None:
                        token = seed_admin_session()
                        assert token == "admin-token"

                    def test_admin_can_delete_user() -> None:
                        token = seed_admin_session()
                        assert token.startswith("admin")
                "#,
                method_named_test_on_collaborator_exempt => r"
                    def test_connection() -> None:
                        assert True

                    def test_login(client: AuthClient) -> None:
                        assert client.test_connection()
                ",
                method_on_non_test_class_exempt => r"
                    class AuthHelper:
                        def test_token(self) -> bool:
                            return True

                        def test_session(self) -> None:
                            assert self.test_token()
                ",
                parameter_named_test_callable_exempt => r"
                    def test_step() -> bool:
                        return True

                    def test_pipeline(test_step: Callable[[], bool]) -> None:
                        assert test_step()
                ",
                recursive_self_call_exempt => r"
                    def test_countdown(remaining: int) -> None:
                        if remaining > 0:
                            test_countdown(remaining - 1)
                        assert remaining >= 0
                ",
                module_level_call_outside_function_exempt => r"
                    def test_smoke() -> bool:
                        return True

                    _SMOKE_OK = test_smoke()
                ",
            ],
            fail: [
                test_calls_sibling_test => r#"
                    def test_create_account() -> str:
                        account_id = "acc-1"
                        assert account_id
                        return account_id

                    def test_close_account() -> None:
                        account_id = test_create_account()
                        assert account_id == "acc-1"
                "# => "test_create_account()",
                test_method_calls_sibling_test_method_on_self => r"
                    class TestAccountSuite:
                        def test_open(self) -> None:
                            assert True

                        def test_close(self) -> None:
                            self.test_open()
                            assert True
                " => "self.test_open()",
                test_method_calls_sibling_test_method_on_cls => r"
                    class TestAccountSuite:
                        @classmethod
                        def test_open(cls) -> None:
                            assert True

                        @classmethod
                        def test_close(cls) -> None:
                            cls.test_open()
                            assert True
                " => "cls.test_open()",
                unittest_testcase_subclass_method_call => r"
                    import unittest

                    class AccountTests(unittest.TestCase):
                        def test_open(self) -> None:
                            self.assertTrue(True)

                        def test_close(self) -> None:
                            self.test_open()
                            self.assertTrue(True)
                " => "self.test_open()",
                class_method_calls_same_named_module_test => r"
                    def test_open() -> None:
                        assert True

                    class TestAccountSuite:
                        def test_open(self) -> None:
                            test_open()
                            assert True
                " => "test_open()",
                helper_in_test_file_calls_test_function => r"
                    def test_seed_schema() -> None:
                        assert True

                    def run_full_suite() -> None:
                        test_seed_schema()
                " => "test_seed_schema()",
            ],
        },
        Rust => {
            pass: [
                shared_helper_without_test_attribute => r#"
                    fn seed_admin() -> String {
                        String::from("admin")
                    }

                    #[test]
                    fn test_admin_login() {
                        let user = seed_admin();
                        assert_eq!(user, "admin");
                    }
                "#,
                helper_named_test_prefix_without_test_attribute_exempt => r#"
                    fn test_connection_builder() -> String {
                        String::from("postgres://localhost/test")
                    }

                    #[test]
                    fn test_connects_to_database() {
                        let url = test_connection_builder();
                        assert!(!url.is_empty());
                    }
                "#,
                method_named_test_on_struct_exempt => r"
                    #[test]
                    fn test_connection() {
                        assert!(true);
                    }

                    #[test]
                    fn test_client_health() {
                        let client = Client::new();
                        let ok = client.test_connection();
                        assert!(ok);
                    }
                ",
                associated_function_on_type_exempt => r"
                    #[test]
                    fn test_connection() {
                        assert!(true);
                    }

                    #[test]
                    fn test_associated_call() {
                        let ok = Client::test_connection();
                        assert!(ok);
                    }
                ",
                parameter_callable_exempt => r"
                    #[test]
                    fn test_step() {
                        assert!(true);
                    }

                    fn run_with_callback(test_step: fn() -> bool) -> bool {
                        test_step()
                    }
                ",
                recursive_self_call_exempt => r"
                    #[test]
                    fn test_countdown() {
                        if false {
                            test_countdown();
                        }
                        assert!(true);
                    }
                ",
                helper_in_sibling_module_exempt => r"
                    mod alpha {
                        #[test]
                        fn runs() {
                            assert!(true);
                        }
                    }

                    mod beta {
                        fn runs() -> bool {
                            true
                        }

                        #[test]
                        fn test_beta_runs() {
                            let ok = runs();
                            assert!(ok);
                        }
                    }
                ",
            ],
            fail: [
                test_calls_sibling_test => r"
                    #[test]
                    fn test_insert_row() {
                        assert_eq!(1 + 1, 2);
                    }

                    #[test]
                    fn test_delete_row() {
                        test_insert_row();
                        assert_eq!(2 - 1, 1);
                    }
                " => "test_insert_row()",
                tokio_test_calls_sibling_tokio_test => r"
                    #[tokio::test]
                    async fn test_connect() {
                        assert!(true);
                    }

                    #[tokio::test]
                    async fn test_disconnect() {
                        test_connect().await;
                        assert!(true);
                    }
                " => "test_connect()",
                helper_in_test_module_calls_test_function => r"
                    #[test]
                    fn test_seed() {
                        assert!(true);
                    }

                    fn run_all() {
                        test_seed();
                    }
                " => "test_seed()",
                qualified_self_or_super_test_call => r"
                    #[test]
                    fn test_seed() {
                        assert!(true);
                    }

                    #[test]
                    fn test_followup() {
                        self::test_seed();
                    }
                " => "self::test_seed()",
                nested_module_calls_super_test => r"
                    #[test]
                    fn test_seed() {
                        assert!(true);
                    }

                    mod nested {
                        #[test]
                        fn test_from_nested() {
                            super::test_seed();
                        }
                    }
                " => "super::test_seed()",
            ],
        },
    }
);
