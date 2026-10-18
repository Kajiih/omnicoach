//! Flags scheduler-yield and spin-wait calls inside loops in test files.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::collections::HashSet;
use std::path::Path;

const BANNED: ListOption = ListOption {
    kind: ListKind::Deny,
    doc: "Scheduler-yield and spin-wait calls flagged when called inside a loop in tests.",
    default: FilterListDefaults {
        base: &["checkpoint", "yield_now", "spin_loop", "sched_yield"],
        extend: &[
            (
                Language::Python,
                &[
                    "anyio.lowlevel.checkpoint",
                    "anyio.lowlevel.checkpoint_if_cancelled",
                    "anyio.lowlevel.cancel_shielded_checkpoint",
                    "trio.lowlevel.checkpoint",
                    "trio.lowlevel.checkpoint_if_cancelled",
                    "trio.lowlevel.cancel_shielded_checkpoint",
                    "trio.testing.wait_all_tasks_blocked",
                    "os.sched_yield",
                ],
            ),
            (
                Language::Rust,
                &[
                    "tokio::task::yield_now",
                    "task::yield_now",
                    "async_std::task::yield_now",
                    "futures_lite::future::yield_now",
                    "smol::future::yield_now",
                    "std::thread::yield_now",
                    "thread::yield_now",
                    "hint::spin_loop",
                    "std::hint::spin_loop",
                    "core::hint::spin_loop",
                ],
            ),
        ],
        remove: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Test calls `{callee}()` inside a loop.",
    rationale: "Yielding in a loop spin-polls for background work; how many scheduler turns that takes varies between runs and under load.",
    suggestion: {
        Python => "Wait on an `asyncio.Event`, an `anyio.Event` or a queue.",
        Rust => "Wait on a `tokio::sync::Notify`, a channel or a `Condvar`.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("yield-loop-in-tests"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(BANNED),
        classification: Classification {
            topics: &[Topic::TEST_TIMING],
            precision: Precision::Exact,
            consensus: Consensus::Unopinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags scheduler-yield and spin-wait calls inside loops in tests.",
            what_it_does: indoc::indoc! {r"
                Flags a call from the `banned` list, such as `anyio.lowlevel.checkpoint()`,
                `trio.lowlevel.checkpoint()`, `os.sched_yield()`, `tokio::task::yield_now()`,
                `std::thread::yield_now()` or `std::hint::spin_loop()`, when it lies inside the
                condition or body of a `for`, `while` or `loop` statement in a test file.

                A single yield outside a loop is not flagged. Calls in a `for` loop's iterator
                expression, in a Python loop's `else` clause, or inside a nested function,
                lambda or closure defined inside a loop are not flagged."},
            why_is_this_bad: indoc::indoc! {r"
                Calling a yield or spin primitive in a loop (`while not ready: await checkpoint()` or
                `for _ in 0..10 { yield_now().await; }`) busy-polls the scheduler until another task
                reaches a target state. Like `sleep(0)`, the number of turns a background task needs
                depends on its internal `await` points and on runtime scheduling, so a fixed loop bound
                is flaky when the task gains an extra step and an unbounded `while` loop hangs when the
                task fails.

                Have the background work signal readiness through an event, a channel or a notification
                primitive and wait on that signal once."},
            known_problems: Some(indoc::indoc! {r"
                A yield inside a helper function that is itself called in a loop is only flagged when
                the loop lives inside the helper."}),
            references: &[
                Reference {
                    title: "Martin Fowler: Eradicating Non-Determinism in Tests",
                    url: "https://martinfowler.com/articles/nonDeterminism.html",
                },
                Reference {
                    title: "Software Engineering at Google, ch. 14: Larger Tests",
                    url: "https://abseil.io/resources/swe-book/html/ch14.html",
                },
                Reference {
                    title: "tokio::task::yield_now",
                    url: "https://docs.rs/tokio/latest/tokio/task/fn.yield_now.html",
                },
                Reference {
                    title: "AnyIO documentation: Testing",
                    url: "https://anyio.readthedocs.io/en/stable/testing.html",
                },
                Reference::NAME_RESOLUTION,
            ],
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r#"
                        async def wait_until_delivered(subscriber):
                            while not subscriber.received:
                                await anyio.lowlevel.checkpoint()


                        async def test_publish_notifies_subscriber(bus, subscriber):
                            bus.publish("order.created")
                            await wait_until_delivered(subscriber)
                            assert subscriber.received == ["order.created"]
                    "#},
                    flagged_span: "anyio.lowlevel.checkpoint()",
                    fixed: indoc::indoc! {r#"
                        async def test_publish_notifies_subscriber(bus, subscriber):
                            bus.publish("order.created")
                            await subscriber.delivered.wait()
                            assert subscriber.received == ["order.created"]
                    "#},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r#"
                        async fn wait_until_delivered(subscriber: &Subscriber) {
                            while subscriber.received().is_empty() {
                                tokio::task::yield_now().await;
                            }
                        }

                        #[tokio::test]
                        async fn test_publish_notifies_subscriber() {
                            let (bus, subscriber) = spawn_bus();
                            bus.publish("order.created");
                            wait_until_delivered(&subscriber).await;
                            assert_eq!(subscriber.received(), ["order.created"]);
                        }
                    "#},
                    flagged_span: "tokio::task::yield_now()",
                    fixed: indoc::indoc! {r#"
                        #[tokio::test]
                        async fn test_publish_notifies_subscriber() {
                            let (bus, subscriber) = spawn_bus();
                            bus.publish("order.created");
                            subscriber.delivered().notified().await;
                            assert_eq!(subscriber.received(), ["order.created"]);
                        }
                    "#},
                },
            ],
        },
    },
    target: RuleTarget::TestsOnly,
    check,
};

fn check(
    rule: &CodeRule<ListOption>,
    path: &Path,
    file: &ParsedFile,
    banned: &HashSet<String>,
) -> Vec<Diagnostic> {
    rule.check_banned_calls_where(path, file, banned, |call_match| call_match.is_in_loop)
}

#[cfg(test)]
crate::test_utils::rule_test!(RULE, {
    Python => {
        pass: [
            single_checkpoint_outside_loop => r"
                import anyio.lowlevel

                async def test_yield_once():
                    await anyio.lowlevel.checkpoint()
            ",
            for_loop_iter_expression_exempt => r"
                import anyio.lowlevel

                async def drain_items():
                    for item in collect_after(await anyio.lowlevel.checkpoint()):
                        consume(item)
            ",
            for_loop_else_clause_exempt => r"
                import anyio.lowlevel

                async def drain_items(items):
                    for item in items:
                        consume(item)
                    else:
                        await anyio.lowlevel.checkpoint()
            ",
            while_loop_else_clause_exempt => r"
                import anyio.lowlevel

                async def drain_until_empty(queue):
                    while not queue.empty():
                        queue.get_nowait()
                    else:
                        await anyio.lowlevel.checkpoint()
            ",
            nested_function_inside_loop_exempt => r"
                import anyio.lowlevel

                def build_callbacks(items):
                    for _ in items:
                        async def on_step():
                            await anyio.lowlevel.checkpoint()
            ",
            lambda_inside_loop_exempt => r"
                def build_thunks(items):
                    for _ in items:
                        thunk = lambda: checkpoint()
            ",
        ],
        fail: [
            anyio_checkpoint_in_while_loop => r"
                import anyio.lowlevel

                async def wait_until_ready(worker):
                    while not worker.is_ready():
                        await anyio.lowlevel.checkpoint()
            " => "anyio.lowlevel.checkpoint()",
            trio_checkpoint_in_for_loop => r"
                import trio.lowlevel

                async def flush_turns():
                    for _ in range(10):
                        await trio.lowlevel.checkpoint()
            " => "trio.lowlevel.checkpoint()",
            wait_all_tasks_blocked_in_loop => r"
                import trio.testing

                async def settle_tasks():
                    for _ in range(5):
                        await trio.testing.wait_all_tasks_blocked()
            " => "trio.testing.wait_all_tasks_blocked()",
            checkpoint_in_while_condition => r"
                import anyio.lowlevel

                async def spin_condition():
                    while await anyio.lowlevel.checkpoint() is None:
                        break
            " => "anyio.lowlevel.checkpoint()",
            unqualified_checkpoint_in_loop => r"
                async def wait_for_state():
                    while not is_ready():
                        await checkpoint()
            " => "checkpoint()",
            os_sched_yield_in_loop => r"
                import os

                def wait_for_thread(worker):
                    while not worker.is_done():
                        os.sched_yield()
            " => "os.sched_yield()",
        ],
    },
    Rust => {
        pass: [
            single_yield_now_outside_loop => r"
                #[tokio::test]
                async fn test_yield_once() {
                    tokio::task::yield_now().await;
                }
            ",
            for_loop_iterable_expression_exempt => r"
                async fn drain_items() {
                    for item in collect_after(tokio::task::yield_now().await) {
                        consume(item);
                    }
                }
            ",
            nested_function_inside_loop_exempt => r"
                fn register_steps() {
                    for _ in 0..3 {
                        async fn step() {
                            tokio::task::yield_now().await;
                        }
                    }
                }
            ",
            closure_inside_loop_exempt => r"
                fn build_spinners() {
                    for _ in 0..3 {
                        let spin = || std::hint::spin_loop();
                        register(spin);
                    }
                }
            ",
        ],
        fail: [
            tokio_yield_now_in_while_loop => r"
                async fn wait_until_ready(worker: &Worker) {
                    while !worker.is_ready() {
                        tokio::task::yield_now().await;
                    }
                }
            " => "tokio::task::yield_now()",
            tokio_yield_now_in_for_loop => r"
                async fn flush_turns() {
                    for _ in 0..10 {
                        tokio::task::yield_now().await;
                    }
                }
            " => "tokio::task::yield_now()",
            std_hint_spin_loop_in_loop => r"
                fn spin_until_ready(flag: &AtomicBool) {
                    loop {
                        if flag.load(Ordering::Acquire) {
                            break;
                        }
                        std::hint::spin_loop();
                    }
                }
            " => "std::hint::spin_loop()",
            std_thread_yield_now_in_loop => r"
                fn wait_for_thread(flag: &AtomicBool) {
                    while !flag.load(Ordering::Acquire) {
                        std::thread::yield_now();
                    }
                }
            " => "std::thread::yield_now()",
            unqualified_yield_now_in_while_loop => r"
                async fn wait_for_state() {
                    while !is_ready() {
                        yield_now().await;
                    }
                }
            " => "yield_now()",
        ],
    },
});
