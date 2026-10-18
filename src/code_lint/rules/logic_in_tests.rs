//! Flags control-flow statements and conditional expressions in test functions.

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Test `{function}` contains {construct}.",
    rationale: "Branching and loops in a test obscure the expected value, require mental simulation to verify, and can silently pass when a branch or loop body does not execute.",
    suggestion: {
        Python => "Split conditional branches into separate `test_*` functions, parameterize variations with `@pytest.mark.parametrize`, or assert against the full expected collection directly.",
        Rust => "Split conditional branches into separate `#[test]` functions, parameterize cases with `#[rstest]`, or assert against the full expected collection directly.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("logic-in-tests"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::TEST_ASSERTIONS],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags control-flow statements and conditional expressions inside test functions.",
            what_it_does: indoc::indoc! {r"
                Flags branching and loop constructs directly inside a test function body: `if`,
                `match`, `for`, `while`, `try`, conditional `if`-`else` expressions, and
                `if`-filtered comprehensions in Python; `if`, `match`, `for`, `while`, and `loop`
                expressions in Rust.

                Nested helper functions, classes, and closures or `lambda` expressions defined
                inside a test are not inspected, so fake implementations and sort callbacks may
                use normal control flow."},
            why_is_this_bad: indoc::indoc! {r"
                A unit test is straight-line code whose expected outcome can be read at a glance.
                When a test computes expectations with loops or branches, a bug in the test's own
                logic can make a broken test pass—for example, a `for` loop or filtered
                comprehension over an unexpectedly empty list executes zero assertions and
                succeeds.

                Write out the expected value directly, split branches into dedicated tests, or
                parameterize input variations with `@pytest.mark.parametrize` or `#[rstest]`."},
            known_problems: None,
            references: &[
                Reference {
                    title: "Software Engineering at Google, ch. 12: Don't Put Logic in Tests",
                    url: "https://abseil.io/resources/swe-book/html/ch12.html",
                },
                Reference {
                    title: "Don't Write Conditional Logic in Tests",
                    url: "https://dev.to/ethancarlsson/dont-write-conditional-logic-in-tests-5aa",
                },
            ],
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r#"
                        def test_active_usernames():
                            users = [User("alice", active=True), User("bob", active=False)]
                            for user in users:
                                if user.active:
                                    assert user.name == "alice"
                    "#},
                    flagged_span: indoc::indoc! {r#"
                        for user in users:
                            if user.active:
                                assert user.name == "alice"
                    "#},
                    fixed: indoc::indoc! {r#"
                        def test_active_usernames():
                            users = [User("alice", active=True), User("bob", active=False)]
                            assert [user.name for user in users] == ["alice", "bob"]
                    "#},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r#"
                        #[test]
                        fn test_parse_port() {
                            let result = parse_port("8080");
                            if let Ok(port) = result {
                                assert_eq!(port, 8080);
                            }
                        }
                    "#},
                    flagged_span: indoc::indoc! {r"
                        if let Ok(port) = result {
                            assert_eq!(port, 8080);
                        }
                    "},
                    fixed: indoc::indoc! {r#"
                        #[test]
                        fn test_parse_port() {
                            let result = parse_port("8080");
                            assert_eq!(result, Ok(8080));
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
    ast::collect_test_logic_constructs(file)
        .into_iter()
        .map(|item| {
            rule.diagnostic_at_node(
                path,
                &item.node,
                &[("function", &item.test_name), ("construct", item.construct)],
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
                helper_function_exempt => r"
                    def verify_users(users):
                        for user in users:
                            if user.is_active:
                                assert user.name
                ",
                nested_function_exempt => r#"
                    def test_with_local_helper():
                        def classify(score):
                            if score >= 50:
                                return "pass"
                            return "fail"

                        assert classify(80) == "pass"
                "#,
                nested_class_exempt => r#"
                    def test_with_local_fake():
                        class FakeStore:
                            def get(self, key):
                                if key == "missing":
                                    return None
                                return "value"

                        store = FakeStore()
                        assert store.get("present") == "value"
                "#,
                lambda_callback_exempt => r"
                    def test_sorted_items(items):
                        ordered = sorted(items, key=lambda item: 0 if item.pinned else 1)
                        assert len(ordered) == 2
                ",
                unfiltered_comprehensions_allowed => r#"
                    def test_projected_collections(users):
                        names = [user.name for user in users]
                        ids = {user.id for user in users}
                        by_id = {user.id: user.name for user in users}
                        lazy_names = tuple(user.name for user in users)
                        assert names == ["alice"]
                        assert ids == {1}
                        assert by_id == {1: "alice"}
                        assert lazy_names == ("alice",)
                "#,
            ],
            fail: [
                if_statement => r#"
                    def test_branch(response):
                        if response.status == 200:
                            assert response.body == "ok"
                "# => r#"
                    if response.status == 200:
                        assert response.body == "ok"
                "#,
                match_statement => r#"
                    def test_match(event):
                        match event.kind:
                            case "created":
                                assert event.id == 1
                "# => r#"
                    match event.kind:
                        case "created":
                            assert event.id == 1
                "#,
                for_loop => r"
                    def test_loop(items):
                        for item in items:
                            assert item > 0
                " => r"
                    for item in items:
                        assert item > 0
                ",
                while_loop => r"
                    def test_while(queue):
                        while not queue.empty():
                            assert queue.pop() is not None
                " => r"
                    while not queue.empty():
                        assert queue.pop() is not None
                ",
                try_statement => r#"
                    def test_try_except():
                        try:
                            parse("bad")
                        except ValueError as err:
                            assert str(err) == "bad"
                "# => r#"
                    try:
                        parse("bad")
                    except ValueError as err:
                        assert str(err) == "bad"
                "#,
                ternary_if_expression => r#"
                    def test_ternary(flag):
                        expected = "yes" if flag else "no"
                        assert format_flag(flag) == expected
                "# => r#""yes" if flag else "no""#,
                filtered_list_comprehension => r#"
                    def test_filtered_list(users):
                        active = [user.name for user in users if user.is_active]
                        assert active == ["alice"]
                "# => "[user.name for user in users if user.is_active]",
                filtered_set_comprehension => r"
                    def test_filtered_set(users):
                        active_ids = {user.id for user in users if user.is_active}
                        assert active_ids == {1}
                " => "{user.id for user in users if user.is_active}",
                filtered_dict_comprehension => r#"
                    def test_filtered_dict(users):
                        active_map = {user.id: user.name for user in users if user.is_active}
                        assert active_map == {1: "alice"}
                "# => "{user.id: user.name for user in users if user.is_active}",
                filtered_generator_expression => r#"
                    def test_filtered_generator(users):
                        active_tuple = tuple(user.name for user in users if user.is_active)
                        assert active_tuple == ("alice",)
                "# => "user.name for user in users if user.is_active",
            ],
        },
        Rust => {
            pass: [
                helper_function_exempt => r"
                    fn assert_all_positive(values: &[i32]) {
                        for value in values {
                            if *value > 0 {
                                assert!(*value > 0);
                            }
                        }
                    }
                ",
                nested_function_exempt => r#"
                    #[test]
                    fn test_with_local_helper() {
                        fn classify(score: u32) -> &'static str {
                            if score >= 50 { "pass" } else { "fail" }
                        }
                        assert_eq!(classify(80), "pass");
                    }
                "#,
                closure_callback_exempt => r#"
                    #[test]
                    fn test_closure_mapping() {
                        let scores = [30, 80];
                        let labels: Vec<&str> = scores
                            .iter()
                            .map(|score| if *score >= 50 { "pass" } else { "fail" })
                            .collect();
                        assert_eq!(labels, vec!["fail", "pass"]);
                    }
                "#,
                matches_macro_allowed => r"
                    #[test]
                    fn test_status_variant() {
                        let status = Some(42);
                        assert!(matches!(status, Some(42)));
                    }
                ",
            ],
            fail: [
                if_expression => r"
                    #[test]
                    fn test_if(value: Option<i32>) {
                        if let Some(inner) = value {
                            assert_eq!(inner, 10);
                        }
                    }
                " => r"
                    if let Some(inner) = value {
                        assert_eq!(inner, 10);
                    }
                ",
                match_expression => r#"
                    #[test]
                    fn test_match(result: Result<i32, &str>) {
                        match result {
                            Ok(val) => assert_eq!(val, 1),
                            Err(err) => panic!("{err}"),
                        }
                    }
                "# => r#"
                    match result {
                        Ok(val) => assert_eq!(val, 1),
                        Err(err) => panic!("{err}"),
                    }
                "#,
                for_loop => r"
                    #[test]
                    fn test_for_loop() {
                        for item in [1, 2, 3] {
                            assert!(item > 0);
                        }
                    }
                " => r"
                    for item in [1, 2, 3] {
                        assert!(item > 0);
                    }
                ",
                while_loop => r"
                    #[test]
                    fn test_while_loop() {
                        let mut remaining = 2;
                        while remaining > 0 {
                            remaining -= 1;
                            assert!(remaining >= 0);
                        }
                    }
                " => r"
                    while remaining > 0 {
                        remaining -= 1;
                        assert!(remaining >= 0);
                    }
                ",
                loop_expression => r"
                    #[test]
                    fn test_loop_expr() {
                        let mut attempts = 0;
                        loop {
                            attempts += 1;
                            assert_eq!(attempts, 1);
                            break;
                        }
                    }
                " => r"
                    loop {
                        attempts += 1;
                        assert_eq!(attempts, 1);
                        break;
                    }
                ",
            ],
        }
    }
);
