//! Enforces a maximum number of assertions per test function.

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, CountOption, Declaration, Example, ImpactedQuality,
    LanguageDefaults, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const MAX_ASSERTIONS: CountOption = CountOption {
    key: "max-assertions",
    doc: "Maximum assertions allowed in one test function.",
    default: LanguageDefaults::new(4, &[]),
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Test `{function}` contains {count} assertions; the limit is {max_assertions}.",
    rationale: "A test with many assertions checks several unrelated behaviors at once and stops at the first failure, hiding the others and blurring what the test is about.",
    suggestion: {
        Python => "Split distinct scenarios into separate `test_*` functions, parameterize variations with `@pytest.mark.parametrize`, or compare one domain object directly.",
        Rust => "Split distinct scenarios into separate `#[test]` functions, parameterize cases with `#[rstest]`, or compare one domain struct directly.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule<CountOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("too-many-assertions"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(MAX_ASSERTIONS),
        classification: Classification {
            topics: &[Topic::TEST_ASSERTIONS],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags test functions with more assertions than the limit.",
            what_it_does: indoc::indoc! {r"
                Flags a test function with more assertions than `max-assertions`. Each `assert`
                statement, `assert*` / `_assert_*` call, `pytest.raises` / `pytest.warns` call, or
                call to a same-file assertion helper counts as one in Python, and each `assert*!` /
                `debug_assert*!` macro call, `assert_*` / `_assert_*` call, or call to a same-file
                assertion helper counts as one in Rust. In Python, `assert x is not None` and
                `assert isinstance(x, ...)` do not count when `x` is read in a later statement of
                the test."},
            why_is_this_bad: indoc::indoc! {r"
                A test with many assertions usually checks several behaviours at once. It stops at
                the first failing assertion, so the later ones are never reported, and its name
                cannot say which behaviour broke.

                Split independent scenarios into separate tests, parameterize variations
                (`@pytest.mark.parametrize`, `#[rstest]`), or compare the result against one
                expected value."},
            known_problems: Some(indoc::indoc! {r"
                - Assertions in an imported helper function without an `assert_` or `_assert_`
                  prefix are not counted, and a call to a same-file assertion helper counts as one
                  assertion regardless of how many checks its body runs.
                - An assertion inside a loop counts once, however many times it runs."}),
            references: &[Reference {
                title: "Software Engineering at Google, ch. 12: Test behaviors, not methods",
                url: "https://abseil.io/resources/swe-book/html/ch12.html",
            }],
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r#"
                        def test_parse_version():
                            version = parse_version("2.7.1-rc1+20261002")
                            assert version.major == 2
                            assert version.minor == 7
                            assert version.patch == 1
                            assert version.pre_release == "rc1"
                            assert version.build_metadata == "20261002"
                    "#},
                    flagged_span: "test_parse_version",
                    fixed: indoc::indoc! {r#"
                        def test_parse_version():
                            version = parse_version("2.7.1-rc1+20261002")
                            assert version == Version(
                                major=2,
                                minor=7,
                                patch=1,
                                pre_release="rc1",
                                build_metadata="20261002",
                            )
                    "#},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r#"
                        #[test]
                        fn test_parse_version() {
                            let version = parse_version("2.7.1-rc1+20261002");
                            assert_eq!(version.major, 2);
                            assert_eq!(version.minor, 7);
                            assert_eq!(version.patch, 1);
                            assert_eq!(version.pre_release, "rc1");
                            assert_eq!(version.build_metadata, "20261002");
                        }
                    "#},
                    flagged_span: "test_parse_version",
                    fixed: indoc::indoc! {r#"
                        #[test]
                        fn test_parse_version() {
                            let version = parse_version("2.7.1-rc1+20261002");
                            assert_eq!(
                                version,
                                Version {
                                    major: 2,
                                    minor: 7,
                                    patch: 1,
                                    pre_release: "rc1",
                                    build_metadata: "20261002",
                                }
                            );
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
    rule: &CodeRule<CountOption>,
    path: &Path,
    file: &ParsedFile,
    max_allowed: usize,
) -> Vec<Diagnostic> {
    ast::collect_test_function_assertion_counts(file)
        .into_iter()
        .filter(|test_fn| test_fn.count > max_allowed)
        .map(|test_fn| {
            let formatted_count = test_fn.count.to_string();
            let formatted_max = max_allowed.to_string();
            rule.diagnostic_at_node(
                path,
                &test_fn.name_node,
                &[
                    ("function", &test_fn.name),
                    ("count", &formatted_count),
                    ("max_assertions", &formatted_max),
                ],
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
                helper_function_exempt => r#"
                    def assert_helper(response):
                        assert response.status == 200
                        assert response.body is not None
                        assert response.headers
                        assert response.cookies
                        assert response.ok
                "#,
                exact_threshold_of_four_allowed => r#"
                    import pytest

                    def test_focused():
                        assert 1 + 1 == 2
                        assert 2 + 2 == 4
                        with pytest.raises(ValueError):
                            int("bad")
                        assert True
                "#,
                nested_function_assertions_not_counted => r#"
                    def test_with_nested_function():
                        def inner_verifier(item):
                            assert item > 0
                            assert item < 100

                        assert 1 == 1
                        assert 2 == 2
                        assert 3 == 3
                        assert 4 == 4
                "#,
                nested_class_assertions_not_counted => r#"
                    def test_with_nested_class():
                        class LocalCheck:
                            def verify(self):
                                assert True

                        assert 1 == 1
                        assert 2 == 2
                        assert 3 == 3
                        assert 4 == 4
                "#,
                narrowing_is_not_none_guard_exempt => r#"
                    def test_narrowed_record(store):
                        record = store.get_record("r1")
                        assert record is not None
                        assert record.status == "active"
                        assert record.owner == "alice"
                        assert record.version == 1
                        assert record.archived is False
                "#,
                narrowing_isinstance_guard_exempt => r#"
                    def test_narrowed_event(bus):
                        event = bus.next_event()
                        assert isinstance(event, CreatedEvent)
                        assert event.id == 1
                        assert event.actor == "alice"
                        assert event.version == 2
                        assert event.committed is True
                "#,
                value_returning_setup_helper_with_assert_not_counted => r#"
                    def _build_ready_client():
                        client = Client()
                        assert client.is_ready
                        return client

                    def test_client_endpoints():
                        client = _build_ready_client()
                        assert client.host == "localhost"
                        assert client.port == 8080
                        assert client.tls is True
                        assert client.timeout == 30
                "#,
            ],
            fail: [
                assert_statements_exceeding_threshold => r#"
                    def test_ok():
                        assert 1 == 1

                    def test_too_many():
                        assert 1 == 1
                        assert 2 == 2
                        assert 3 == 3
                        assert 4 == 4
                        assert 5 == 5
                "# => "test_too_many",
                terminal_is_not_none_counted => r#"
                    def test_terminal_none_check(store):
                        record = store.get_record("r1")
                        assert 1 == 1
                        assert 2 == 2
                        assert 3 == 3
                        assert 4 == 4
                        assert record is not None
                "# => "test_terminal_none_check",
                unittest_assertions_exceeding_threshold => r#"
                    class OrderTest:
                        def test_order_lifecycle(self):
                            self.assertEqual(1, 1)
                            self.assertTrue(True)
                            self.assertIsNotNone("ok")
                            self.assertIn("a", "abc")
                            self.fail("unreachable")
                "# => "test_order_lifecycle",
                pytest_raises_calls_exceeding_threshold => r#"
                    import pytest

                    def test_raises_lifecycle():
                        with pytest.raises(ValueError):
                            int("1")
                        with pytest.raises(ValueError):
                            int("2")
                        with pytest.raises(ValueError):
                            int("3")
                        with pytest.raises(ValueError):
                            int("4")
                        with pytest.raises(ValueError):
                            int("5")
                "# => "test_raises_lifecycle",
                bare_raises_calls_exceeding_threshold => r#"
                    from pytest import raises

                    def test_bare_raises():
                        with raises(ValueError):
                            int("1")
                        with raises(ValueError):
                            int("2")
                        with raises(ValueError):
                            int("3")
                        with raises(ValueError):
                            int("4")
                        with raises(ValueError):
                            int("5")
                "# => "test_bare_raises",
                pytest_warns_calls_exceeding_threshold => r#"
                    import pytest

                    def test_warns_lifecycle():
                        with pytest.warns(UserWarning):
                            pass
                        with pytest.warns(UserWarning):
                            pass
                        with pytest.warns(UserWarning):
                            pass
                        with pytest.warns(UserWarning):
                            pass
                        with pytest.warns(UserWarning):
                            pass
                "# => "test_warns_lifecycle",
                same_file_assertion_helper_calls_exceeding_threshold => r#"
                    def _verify_status(item, expected):
                        assert item.status == expected

                    def test_batch_statuses(items):
                        _verify_status(items[0], "open")
                        _verify_status(items[1], "open")
                        _verify_status(items[2], "closed")
                        _verify_status(items[3], "closed")
                        _verify_status(items[4], "archived")
                "# => "test_batch_statuses",
                exception_assertion_helper_returning_value_counted => r#"
                    import pytest

                    def _run_cli_expecting_exit(args: list[str]) -> int:
                        with pytest.raises(SystemExit) as exc_info:
                            run_cli(args)
                        return exc_info.value.code

                    def test_cli_invalid_flags():
                        assert _run_cli_expecting_exit(["--bad"]) == 2
                        assert _run_cli_expecting_exit(["--worse"]) == 2
                        assert _run_cli_expecting_exit(["--empty"]) == 2
                "# => "test_cli_invalid_flags",
            ],
        },
        Rust => {
            pass: [
                helper_function_exempt => r#"
                    fn assert_helper() {
                        assert_eq!(1, 1);
                        assert_eq!(2, 2);
                        assert_eq!(3, 3);
                        assert_eq!(4, 4);
                        assert_eq!(5, 5);
                    }
                "#,
                exact_threshold_of_four_allowed => r#"
                    #[test]
                    fn parses_valid_header() {
                        assert_eq!(1, 1);
                        assert_ne!(1, 2);
                        assert!(true);
                        debug_assert_eq!(3, 3);
                    }
                "#,
                nested_function_assertions_not_counted => r#"
                    #[test]
                    fn test_with_nested_function() {
                        fn inner_check() {
                            assert_eq!(10, 10);
                            assert_eq!(20, 20);
                        }
                        assert_eq!(1, 1);
                        assert_eq!(2, 2);
                        assert_eq!(3, 3);
                        assert_eq!(4, 4);
                    }
                "#,
                value_returning_setup_helper_with_assert_not_counted => r#"
                    fn build_ready_client() -> Client {
                        let client = Client::new();
                        assert!(client.is_ready());
                        client
                    }

                    #[test]
                    fn test_client_endpoints() {
                        let client = build_ready_client();
                        assert_eq!(client.host(), "localhost");
                        assert_eq!(client.port(), 8080);
                        assert!(client.tls());
                        assert_eq!(client.timeout(), 30);
                    }
                "#,
            ],
            fail: [
                attributed_test_function_exceeding_threshold => r#"
                    #[tokio::test]
                    async fn attributed_check() {
                        assert_eq!(1, 1);
                        assert_eq!(2, 2);
                        assert_eq!(3, 3);
                        assert_eq!(4, 4);
                        assert_eq!(5, 5);
                    }
                "# => "attributed_check",
                unattributed_test_function_exceeding_threshold => r#"
                    fn test_unattributed() {
                        assert_eq!(1, 1);
                        assert_eq!(2, 2);
                        assert_eq!(3, 3);
                        assert_eq!(4, 4);
                        assert_eq!(5, 5);
                    }
                "# => "test_unattributed",
                same_file_assertion_helper_calls_exceeding_threshold => r#"
                    fn verify_status(item: &Item, expected: &str) {
                        assert_eq!(item.status(), expected);
                    }

                    #[test]
                    fn test_batch_statuses() {
                        let items = load_items();
                        verify_status(&items[0], "open");
                        verify_status(&items[1], "open");
                        verify_status(&items[2], "closed");
                        verify_status(&items[3], "closed");
                        verify_status(&items[4], "archived");
                    }
                "# => "test_batch_statuses",
            ],
        },
    }
);
