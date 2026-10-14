//! Flags test functions that interleave actions and assertions across multiple phases.

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Test `{function}` interleaves actions and assertions across multiple phases.",
    rationale: "Alternating between actions and assertions in one test couples multiple scenarios together, so an early failure skips later phases and obscures which state transition broke.",
    suggestion: {
        Python => "Split each arrange-act-assert phase into its own `test_*` function, or move all setup actions above the assertions.",
        Rust => "Split each arrange-act-assert phase into its own `#[test]` function, or move all setup actions above the assertions.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("multi-phase-test"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::TEST_ASSERTIONS],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags test functions that perform further actions and assertions after an earlier assertion.",
            what_it_does: indoc::indoc! {r"
                Flags a test function whose top-level statements follow an assert-act-assert
                pattern: one or more assertions, followed by a non-assertion statement, followed
                by another assertion.

                In Python, `assert x is not None` and `assert isinstance(x, ...)` are treated as
                type-narrowing guards rather than behavioral assertions when `x` is read in a
                later statement of the test."},
            why_is_this_bad: indoc::indoc! {r"
                A test that mutates state, asserts, mutates state again, and asserts a second time
                is testing a sequence of scenarios inside a single function. When the first phase
                fails, the second phase never runs, and when the second phase fails, the reader has
                to trace the intermediate state mutations from the first phase to understand the
                preconditions.

                Give each behavior its own arrange-act-assert test function, or compute the
                values under test before running the assertions."},
            known_problems: Some(indoc::indoc! {r"
                - Stateful lifecycle or protocol handshake tests that intentionally step through a
                  sequence of transitions in one test need an `# omni:ignore[multi-phase-test]`
                  suppression with a reason.
                - Helper calls that perform assertions without an `assert` prefix are classified
                  as non-assertion statements."}),
            references: &[Reference {
                title: "Software Engineering at Google, ch. 12: Structure Tests to Emphasize Behaviors",
                url: "https://abseil.io/resources/swe-book/html/ch12.html",
            }],
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r#"
                        def test_cart_lifecycle():
                            cart = Cart()
                            cart.add("book")
                            assert len(cart.items) == 1
                            cart.clear()
                            assert len(cart.items) == 0
                    "#},
                    flagged_span: "test_cart_lifecycle",
                    fixed: indoc::indoc! {r#"
                        def test_cart_add_item():
                            cart = Cart()
                            cart.add("book")
                            assert len(cart.items) == 1

                        def test_cart_clear():
                            cart = Cart(items=["book"])
                            cart.clear()
                            assert len(cart.items) == 0
                    "#},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r#"
                        #[test]
                        fn test_cart_lifecycle() {
                            let mut cart = Cart::new();
                            cart.add("book");
                            assert_eq!(cart.len(), 1);
                            cart.clear();
                            assert_eq!(cart.len(), 0);
                        }
                    "#},
                    flagged_span: "test_cart_lifecycle",
                    fixed: indoc::indoc! {r#"
                        #[test]
                        fn test_cart_add_item() {
                            let mut cart = Cart::new();
                            cart.add("book");
                            assert_eq!(cart.len(), 1);
                        }

                        #[test]
                        fn test_cart_clear() {
                            let mut cart = Cart::from_items(["book"]);
                            cart.clear();
                            assert_eq!(cart.len(), 0);
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
    ast::collect_multi_phase_test_functions(file)
        .into_iter()
        .map(|(name_node, func_name)| {
            rule.diagnostic_at_node(path, &name_node, &[("function", &func_name)])
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
                    def verify_transitions(cart):
                        cart.add("book")
                        assert len(cart.items) == 1
                        cart.clear()
                        assert len(cart.items) == 0
                "#,
                single_phase_multiple_assertions_allowed => r#"
                    def test_parse_header():
                        header = parse_header("X-Request-Id: 42")
                        assert header.name == "X-Request-Id"
                        assert header.value == "42"
                "#,
                narrowing_is_not_none_before_action_exempt => r#"
                    def test_narrowed_user_profile(repo):
                        user = repo.find_by_id("u1")
                        assert user is not None
                        profile = user.load_profile()
                        assert profile.is_active
                "#,
                narrowing_isinstance_between_assertions_exempt => r#"
                    def test_narrowed_event_payload(bus):
                        event = bus.pop()
                        assert event.topic == "orders"
                        assert isinstance(event, OrderCreated)
                        assert event.order_id == 7
                "#,
                nested_function_assertions_not_counted => r"
                    def test_with_local_verifier():
                        def check_positive(val):
                            assert val > 0

                        result = compute_score()
                        assert result == 10
                ",
            ],
            fail: [
                act_between_assert_statements => r#"
                    def test_queue_push_pop():
                        queue = Queue()
                        queue.push("first")
                        assert queue.size() == 1
                        popped = queue.pop()
                        assert popped == "first"
                "# => "test_queue_push_pop",
                act_after_pytest_raises_block => r#"
                    import pytest

                    def test_error_then_recovery(service):
                        with pytest.raises(ValueError):
                            service.execute("invalid")
                        result = service.execute("valid")
                        assert result == "ok"
                "# => "test_error_then_recovery",
                act_between_unittest_assertions => r"
                    class OrderTest:
                        def test_submit_then_cancel(self):
                            order = Order()
                            self.assertTrue(order.is_draft())
                            order.cancel()
                            self.assertTrue(order.is_cancelled())
                " => "test_submit_then_cancel",
                unreferenced_is_not_none_counts_as_assertion => r#"
                    def test_two_independent_queries(repo):
                        first = repo.find("a")
                        assert first is not None
                        second = repo.find("b")
                        assert second == "b"
                "# => "test_two_independent_queries",
            ],
        },
        Rust => {
            pass: [
                helper_function_exempt => r#"
                    fn verify_transitions(cart: &mut Cart) {
                        cart.add("book");
                        assert_eq!(cart.len(), 1);
                        cart.clear();
                        assert_eq!(cart.len(), 0);
                    }
                "#,
                single_phase_multiple_assertions_allowed => r#"
                    #[test]
                    fn test_parse_header() {
                        let header = parse_header("X-Request-Id: 42");
                        assert_eq!(header.name, "X-Request-Id");
                        assert_eq!(header.value, "42");
                    }
                "#,
                local_item_declaration_between_assertions_exempt => r"
                    #[test]
                    fn test_with_local_type() {
                        assert_eq!(1 + 1, 2);
                        struct Marker;
                        assert_eq!(2 + 2, 4);
                    }
                ",
            ],
            fail: [
                method_call_between_assertions => r"
                    #[test]
                    fn test_stack_push_pop() {
                        let mut stack = Vec::new();
                        stack.push(10);
                        assert_eq!(stack.len(), 1);
                        stack.pop();
                        assert!(stack.is_empty());
                    }
                " => "test_stack_push_pop",
                let_binding_between_assertions => r"
                    #[test]
                    fn test_two_independent_calculations() {
                        let first = compute(1);
                        assert_eq!(first, 2);
                        let second = compute(2);
                        assert_eq!(second, 4);
                    }
                " => "test_two_independent_calculations",
                tail_expression_assertion_after_action => r"
                    #[test]
                    fn test_tail_assertion() {
                        let mut count = 1;
                        assert_eq!(count, 1);
                        count += 1;
                        assert_eq!(count, 2)
                    }
                " => "test_tail_assertion",
            ],
        }
    }
);
