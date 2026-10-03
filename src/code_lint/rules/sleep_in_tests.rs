//! Flags wall-clock/async `sleep` calls and zero-duration sleeps in test files.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::code_lint::semantic::calls::{self, CallMatch};
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
    doc: "Sleep calls flagged in tests.",
    default: FilterListDefaults {
        base: &["sleep"],
        extend: &[
            (
                SupportLang::Python,
                &["time.sleep", "asyncio.sleep", "anyio.sleep", "trio.sleep"],
            ),
            (
                SupportLang::Rust,
                &[
                    "thread::sleep",
                    "std::thread::sleep",
                    "time::sleep",
                    "tokio::time::sleep",
                ],
            ),
        ],
        remove: &[],
    },
};

const SLEEP_TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Test calls `{callee}()`.",
    rationale: "Sleeping in a test slows the suite and makes the outcome depend on timing: the test passes on a fast machine and fails under load.",
    suggestion: {
        base: "Wait on a deterministic signal (an event, a channel, a condition) or advance an injected clock.",
        Python => "Wait on an `asyncio.Event`, an `anyio.Event` or a queue, or advance an injected clock (`clock.sleep(...)`).",
        Rust => "Wait on a `tokio::sync::Notify`, a channel or a `Condvar`, or pause time with `tokio::time::pause()`.",
    },
};

const ZERO_SLEEP_TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Test calls `{callee}()` with a zero duration.",
    rationale: "A zero-duration sleep yields to the scheduler in the hope that background work completes; how many ticks that takes varies between runs and runtimes.",
    suggestion: {
        base: "Wait on an explicit event or queue, or call the runtime's explicit yield primitive.",
        Python => "Wait on an `asyncio.Event` or `asyncio.Queue`, or call `await anyio.lowlevel.checkpoint()` under AnyIO.",
        Rust => "Wait on an explicit signal, or call `tokio::task::yield_now().await` to yield once explicitly.",
    },
};

/// The `sleep-in-tests` rule's declaration.
pub const SLEEP_IN_TESTS: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("sleep-in-tests"),
        template: &SLEEP_TEMPLATE,
        languages: &[SupportLang::Python, SupportLang::Rust],
        options: RuleOptions::code_rule(BANNED),
        classification: Classification {
            topics: &[Topic::TEST_TIMING],
            precision: Precision::Exact,
            consensus: Consensus::Unopinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags fixed-duration sleeps in tests.",
            what_it_does: "Flags wall-clock and async sleep calls in test files: `time.sleep`, \
                           `asyncio.sleep`, `anyio.sleep` and `trio.sleep` in Python, \
                           `std::thread::sleep` and `tokio::time::sleep` in Rust, and a bare \
                           `sleep`. Calls on an injected object, such as `fake_clock.sleep(10)`, \
                           are not flagged. Zero-duration sleeps are left to \
                           `zero-sleep-in-tests`.",
            why_is_this_bad: "A fixed sleep guesses how long another thread, task or process \
                              needs. Too short, and the test fails when the machine is loaded: \
                              the test is flaky. Too long, and every run pays the full delay. \
                              Either way, the test no longer says what it waits for.\n\n\
                              Wait on the event itself (an event, a channel, a condition \
                              variable), or inject a clock that the test advances.",
            references: &[
                Reference {
                    title: "Eradicating Non-Determinism in Tests (Martin Fowler)",
                    url: "https://martinfowler.com/articles/nonDeterminism.html",
                },
                Reference {
                    title: "Flaky Tests at Google and How We Mitigate Them",
                    url: "https://testing.googleblog.com/2016/05/flaky-tests-at-google-and-how-we.html",
                },
            ],
            examples: &[
                Example {
                    language: SupportLang::Python,
                    flagged: indoc::indoc! {r#"
                        def test_session_expires(sessions):
                            sessions.open("alice", ttl=30)
                            time.sleep(31)
                            assert not sessions.is_active("alice")
                    "#},
                    flagged_span: "time.sleep(31)",
                    fixed: indoc::indoc! {r#"
                        def test_session_expires(sessions, fake_clock):
                            sessions.open("alice", ttl=30)
                            fake_clock.advance(31)
                            assert not sessions.is_active("alice")
                    "#},
                },
                Example {
                    language: SupportLang::Rust,
                    flagged: indoc::indoc! {r#"
                        #[tokio::test]
                        async fn test_session_expires() {
                            let sessions = SessionStore::with_ttl(Duration::from_secs(30));
                            sessions.open("alice");
                            tokio::time::sleep(Duration::from_secs(31)).await;
                            assert!(!sessions.is_active("alice"));
                        }
                    "#},
                    flagged_span: "tokio::time::sleep(Duration::from_secs(31))",
                    fixed: indoc::indoc! {r#"
                        #[tokio::test(start_paused = true)]
                        async fn test_session_expires() {
                            let sessions = SessionStore::with_ttl(Duration::from_secs(30));
                            sessions.open("alice");
                            tokio::time::advance(Duration::from_secs(31)).await;
                            assert!(!sessions.is_active("alice"));
                        }
                    "#},
                },
            ],
        },
    },
    target: RuleTarget::TestsOnly,
    check: check_sleep,
};

/// The `zero-sleep-in-tests` rule's declaration.
pub const ZERO_SLEEP_IN_TESTS: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("zero-sleep-in-tests"),
        template: &ZERO_SLEEP_TEMPLATE,
        languages: &[SupportLang::Python, SupportLang::Rust],
        options: RuleOptions::code_rule(BANNED),
        classification: Classification {
            topics: &[Topic::TEST_TIMING],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags zero-duration sleeps used to yield in tests.",
            what_it_does: "Flags the same sleep calls as `sleep-in-tests` when their single \
                           argument is a literal zero duration: `0`, `0.0` or `0.` in Python, \
                           and `Duration::ZERO`, `Duration::from_secs(0)` or \
                           `Duration::from_millis(0)` in Rust. Other spellings of zero, such as \
                           a variable holding `0`, and calls with more than one argument are not \
                           flagged. Python is checked too, including `asyncio.sleep(0)`.",
            why_is_this_bad: "In a test, a zero-duration sleep is used for its side effect: \
                              letting another task advance by one scheduler turn. If that task \
                              later gains a second `await` point, a single `sleep(0)` is no \
                              longer enough, and whether a zero timer yields at all depends on \
                              the runtime (`tokio::time::sleep(Duration::ZERO)` does not \
                              guarantee a yield).\n\n\
                              Wait on an explicit signal (`asyncio.Event`, `asyncio.Queue`) or \
                              use the runtime's dedicated yield primitive \
                              (`tokio::task::yield_now().await` in Rust, \
                              `await anyio.lowlevel.checkpoint()` in AnyIO).",
            references: &[
                Reference {
                    title: "tokio::task::yield_now",
                    url: "https://docs.rs/tokio/latest/tokio/task/fn.yield_now.html",
                },
                Reference {
                    title: "asyncio.sleep (Python documentation)",
                    url: "https://docs.python.org/3/library/asyncio-task.html#asyncio.sleep",
                },
            ],
            examples: &[
                Example {
                    language: SupportLang::Python,
                    flagged: indoc::indoc! {r#"
                        async def test_publish_notifies_subscriber(bus, subscriber):
                            bus.publish("order.created")
                            await asyncio.sleep(0)
                            assert subscriber.received == ["order.created"]
                    "#},
                    flagged_span: "asyncio.sleep(0)",
                    fixed: indoc::indoc! {r#"
                        async def test_publish_notifies_subscriber(bus, subscriber):
                            bus.publish("order.created")
                            await subscriber.delivered.wait()
                            assert subscriber.received == ["order.created"]
                    "#},
                },
                Example {
                    language: SupportLang::Rust,
                    flagged: indoc::indoc! {r#"
                        #[tokio::test]
                        async fn test_publish_notifies_subscriber() {
                            let (bus, subscriber) = spawn_bus();
                            bus.publish("order.created");
                            tokio::time::sleep(Duration::ZERO).await;
                            assert_eq!(subscriber.received(), ["order.created"]);
                        }
                    "#},
                    flagged_span: "tokio::time::sleep(Duration::ZERO)",
                    fixed: indoc::indoc! {r#"
                        #[tokio::test]
                        async fn test_publish_notifies_subscriber() {
                            let (bus, subscriber) = spawn_bus();
                            bus.publish("order.created");
                            tokio::task::yield_now().await;
                            assert_eq!(subscriber.received(), ["order.created"]);
                        }
                    "#},
                },
            ],
        },
    },
    target: RuleTarget::TestsOnly,
    check: check_zero_sleep,
};

/// Returns true if the call's single argument is a literal zero duration, such as `sleep(0)` or
/// `sleep(Duration::ZERO)`.
fn has_zero_duration_argument(call_match: &CallMatch<'_>) -> bool {
    let [argument] = call_match.arguments.as_slice() else {
        return false;
    };
    matches!(
        argument.text().trim(),
        "0" | "0.0"
            | "0."
            | "Duration::ZERO"
            | "std::time::Duration::ZERO"
            | "tokio::time::Duration::ZERO"
            | "Duration::from_secs(0)"
            | "Duration::from_millis(0)"
    )
}

/// Template placeholder naming the sleep call.
const CALLEE: &str = "callee";

fn check_sleep(
    rule: &CodeRule<ListOption>,
    path: &Path,
    file: &ParsedFile,
    banned: &HashSet<String>,
) -> Vec<Diagnostic> {
    calls::find_banned_calls(file, banned)
        .into_iter()
        .filter(|call_match| !has_zero_duration_argument(call_match))
        .map(|call_match| {
            rule.diagnostic_at_node(path, &call_match.node, &[(CALLEE, &call_match.callee)])
        })
        .collect()
}

fn check_zero_sleep(
    rule: &CodeRule<ListOption>,
    path: &Path,
    file: &ParsedFile,
    banned: &HashSet<String>,
) -> Vec<Diagnostic> {
    calls::find_banned_calls(file, banned)
        .into_iter()
        .filter(has_zero_duration_argument)
        .map(|call_match| {
            rule.diagnostic_at_node(path, &call_match.node, &[(CALLEE, &call_match.callee)])
        })
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(tests_sleep_in_tests: SLEEP_IN_TESTS, {
    Python => {
        pass: [
            zero_duration_sleep_handled_separately => r"
                import asyncio

                async def test_yield():
                    await asyncio.sleep(0)
            ",
            injected_fake_clock_sleep => r"
                async def test_timeout(fake_clock):
                    await fake_clock.sleep(10)
            ",
            custom_receiver_method => r"
                def test_worker(worker):
                    worker.sleep(5)
            ",
        ],
        fail: [
            time_sleep => r"
                import time

                def test_polling():
                    time.sleep(1)
            " => "time.sleep(1)",
            asyncio_sleep => r"
                import asyncio

                async def test_polling():
                    await asyncio.sleep(0.5)
            " => "asyncio.sleep(0.5)",
            anyio_sleep => r"
                import anyio

                async def test_polling():
                    await anyio.sleep(2)
            " => "anyio.sleep(2)",
            trio_sleep => r"
                import trio

                async def test_polling():
                    await trio.sleep(1)
            " => "trio.sleep(1)",
            unqualified_sleep => r"
                def test_polling():
                    sleep(1)
            " => "sleep(1)",
            multi_arg_sleep_not_exempt => r#"
                def test_polling():
                    sleep(0, "custom_tag")
            "# => r#"sleep(0, "custom_tag")"#,
        ],
    },
    Rust => {
        pass: [
            zero_duration_sleep_handled_separately => r"
                use std::time::Duration;

                #[tokio::test]
                async fn test_yield() {
                    tokio::time::sleep(Duration::ZERO).await;
                }
            ",
            injected_fake_clock_method => r"
                use std::time::Duration;

                #[tokio::test]
                async fn test_backoff() {
                    fake_clock.sleep(Duration::from_secs(5)).await;
                }
            ",
        ],
        fail: [
            thread_sleep => r"
                use std::time::Duration;

                #[test]
                fn test_retry() {
                    thread::sleep(Duration::from_millis(50));
                }
            " => "thread::sleep(Duration::from_millis(50))",
            std_thread_sleep => r"
                use std::time::Duration;

                #[test]
                fn test_retry() {
                    std::thread::sleep(Duration::from_millis(50));
                }
            " => "std::thread::sleep(Duration::from_millis(50))",
            tokio_time_sleep => r"
                use std::time::Duration;

                #[tokio::test]
                async fn test_retry() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            " => "tokio::time::sleep(Duration::from_millis(10))",
            time_sleep => r"
                use std::time::Duration;
                use tokio::time;

                #[tokio::test]
                async fn test_retry() {
                    time::sleep(Duration::from_millis(20)).await;
                }
            " => "time::sleep(Duration::from_millis(20))",
            unqualified_sleep => r"
                use std::time::Duration;

                #[test]
                fn test_retry() {
                    sleep(Duration::from_millis(5));
                }
            " => "sleep(Duration::from_millis(5))",
        ],
    },
});

#[cfg(test)]
crate::test_utils::rule_test!(tests_zero_sleep_in_tests: ZERO_SLEEP_IN_TESTS, {
    Python => {
        pass: [
            non_zero_sleep => r"
                import asyncio

                async def test_wait():
                    await asyncio.sleep(1)
            ",
            multi_arg_sleep => r#"
                def test_wait():
                    sleep(0, "custom_tag")
            "#,
            anyio_checkpoint => r"
                import anyio.lowlevel

                async def test_yield():
                    await anyio.lowlevel.checkpoint()
            ",
        ],
        fail: [
            sleep_zero_integer => r"
                import asyncio

                async def test_yield():
                    await asyncio.sleep(0)
            " => "asyncio.sleep(0)",
            sleep_zero_float => r"
                import anyio

                async def test_yield():
                    await anyio.sleep(0.0)
            " => "anyio.sleep(0.0)",
            sleep_zero_trailing_dot => r"
                def test_yield():
                    sleep(0.)
            " => "sleep(0.)",
        ],
    },
    Rust => {
        pass: [
            non_zero_sleep => r"
                use std::time::Duration;

                #[tokio::test]
                async fn test_wait() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            ",
            tokio_yield_now => r"
                #[tokio::test]
                async fn test_yield() {
                    tokio::task::yield_now().await;
                }
            ",
        ],
        fail: [
            duration_zero => r"
                use std::time::Duration;

                #[tokio::test]
                async fn test_yield() {
                    tokio::time::sleep(Duration::ZERO).await;
                }
            " => "tokio::time::sleep(Duration::ZERO)",
            std_time_duration_zero => r"
                #[tokio::test]
                async fn test_yield() {
                    tokio::time::sleep(std::time::Duration::ZERO).await;
                }
            " => "tokio::time::sleep(std::time::Duration::ZERO)",
            tokio_time_duration_zero => r"
                #[tokio::test]
                async fn test_yield() {
                    tokio::time::sleep(tokio::time::Duration::ZERO).await;
                }
            " => "tokio::time::sleep(tokio::time::Duration::ZERO)",
            duration_from_secs_zero => r"
                use std::time::Duration;

                #[test]
                fn test_yield() {
                    std::thread::sleep(Duration::from_secs(0));
                }
            " => "std::thread::sleep(Duration::from_secs(0))",
            duration_from_millis_zero => r"
                use std::time::Duration;

                #[test]
                fn test_yield() {
                    std::thread::sleep(Duration::from_millis(0));
                }
            " => "std::thread::sleep(Duration::from_millis(0))",
        ],
    },
});
