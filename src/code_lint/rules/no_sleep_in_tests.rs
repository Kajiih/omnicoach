//! Flags wall-clock/async `sleep` calls (`no-sleep-in-tests`) and zero-duration sleeps (`no-zero-sleep-in-tests`) in test files.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::rule::{CodeDetector, RuleTarget};
use crate::code_lint::semantic::calls::CallMatch;
use crate::core::{Config, Detector, FilterListDefaults};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_documentation::{ConfigShape, Reference, RuleDoc};
use crate::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Topic};
use ast_grep_language::SupportLang;
use std::path::Path;

/// Static default banned sleep call patterns across Python and Rust.
const DEFAULT_BANNED_CALLS: FilterListDefaults = FilterListDefaults {
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
    exempt: &[],
};

const SLEEP_TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Wall-clock or async sleep call `{call}()` in test.",
    rationale: "Sleeping for fixed durations slows down test execution and introduces timing-dependent flakiness under load.",
    suggestion: {
        base: "Synchronize on deterministic primitives (events, channels, conditions) or advance an injected virtual clock (`clock.sleep(...)`).",
        Python => "Synchronize on deterministic signals (`anyio.Event`, `asyncio.Event`, or a queue) or advance an injected virtual clock (`clock.sleep(...)`).",
        Rust => "Synchronize on deterministic primitives (`tokio::sync::Notify`, channels, `Condvar`) or pause time with `tokio::time::pause()`.",
    },
};

const ZERO_SLEEP_TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Zero-duration sleep call `{call}({arg})` in test.",
    rationale: "Using a zero-duration sleep to yield execution to the scheduler obscures intent and relies on side effects of the timer subsystem.",
    suggestion: {
        base: "Use an explicit scheduler yield or checkpoint primitive.",
        Python => "Use `await anyio.lowlevel.checkpoint()` to yield control to the event loop explicitly.",
        Rust => "Use `tokio::task::yield_now().await` to yield control to the async executor explicitly.",
    },
};

/// Rule that bans non-zero wall-clock and async sleeps in test files.
pub struct NoSleepInTests;

impl NoSleepInTests {
    /// The rule's declared facets.
    pub(crate) const CLASSIFICATION: Classification = Classification {
        topics: &[Topic::TEST_TIMING],
        precision: Precision::Exact,
        consensus: Consensus::Unopinionated,
        impacted_quality: ImpactedQuality::Reliability,
    };

    /// The rule's user-facing doc.
    pub(crate) const DOC: RuleDoc = RuleDoc {
        summary: "Flags fixed-duration sleeps in tests.",
        what_it_does: "Flags wall-clock and async sleep calls in test files: `time.sleep`, \
                       `asyncio.sleep`, `anyio.sleep` and `trio.sleep` in Python, \
                       `std::thread::sleep` and `tokio::time::sleep` in Rust, and a bare \
                       `sleep`. Calls on an injected object, such as `fake_clock.sleep(10)`, \
                       are not flagged. Zero-duration sleeps are left to \
                       `no-zero-sleep-in-tests`.",
        why_is_this_bad: "A fixed sleep guesses how long another thread, task or process \
                          needs. Too short, and the test fails when the machine is loaded: \
                          the test is flaky. Too long, and every run pays the full delay. \
                          Either way, the test no longer says what it waits for.\n\n\
                          Wait on the event itself (an event, a channel, a condition \
                          variable), or inject a clock that the test advances.",
        configuration: &[ConfigShape::DenyList],
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
    };
}

impl Detector for NoSleepInTests {
    fn name(&self) -> RuleName {
        RuleName("no-sleep-in-tests")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &SLEEP_TEMPLATE
    }
}

/// Rule that bans zero-duration sleeps (`sleep(0)`, `sleep(Duration::ZERO)`) in test files.
pub struct NoZeroSleepInTests;

impl NoZeroSleepInTests {
    /// The rule's declared facets.
    pub(crate) const CLASSIFICATION: Classification = Classification {
        topics: &[Topic::TEST_TIMING],
        precision: Precision::Exact,
        consensus: Consensus::Opinionated,
        impacted_quality: ImpactedQuality::Reliability,
    };

    /// The rule's user-facing doc.
    pub(crate) const DOC: RuleDoc = RuleDoc {
        summary: "Flags zero-duration sleeps used to yield in tests.",
        what_it_does: "Flags the same sleep calls as `no-sleep-in-tests` when their single \
                       argument is a literal zero duration: `0`, `0.0` or `0.` in Python, \
                       and `Duration::ZERO`, `Duration::from_secs(0)` or \
                       `Duration::from_millis(0)` in Rust. Other spellings of zero, such as \
                       a variable holding `0`, and calls with more than one argument are not \
                       flagged. Python is checked too, including `asyncio.sleep(0)`, even \
                       though the asyncio documentation presents it as a way to yield.",
        why_is_this_bad: "A zero-duration sleep is used for its side effect: letting other \
                          tasks run. The code says \"wait for no time\" when it means \"yield \
                          to the scheduler\", and whether it yields depends on how the \
                          runtime treats a zero timer. Tokio, for example, does not guarantee \
                          that `sleep(Duration::ZERO)` yields at all.\n\n\
                          Use the explicit yield primitive: `tokio::task::yield_now().await` \
                          in Rust, `await anyio.lowlevel.checkpoint()` in Python.",
        configuration: &[ConfigShape::DenyList],
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
    };
}

impl Detector for NoZeroSleepInTests {
    fn name(&self) -> RuleName {
        RuleName("no-zero-sleep-in-tests")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &ZERO_SLEEP_TEMPLATE
    }
}

/// Returns the trimmed argument string if the call is a zero-duration sleep (e.g. `sleep(0)`, `sleep(Duration::ZERO)`).
fn zero_duration_arg(call_match: &CallMatch<'_>) -> Option<String> {
    let [argument] = call_match.arguments.as_slice() else {
        return None;
    };
    let arg_text = argument.text();
    let trimmed = arg_text.trim();
    matches!(
        trimmed,
        "0" | "0.0"
            | "0."
            | "Duration::ZERO"
            | "std::time::Duration::ZERO"
            | "tokio::time::Duration::ZERO"
            | "Duration::from_secs(0)"
            | "Duration::from_millis(0)"
    )
    .then(|| trimmed.to_string())
}

impl CodeDetector for NoSleepInTests {
    fn target(&self) -> RuleTarget {
        RuleTarget::TestsOnly
    }

    fn check_file(&self, path: &Path, file: &ParsedFile, config: &Config) -> Vec<Diagnostic> {
        self.find_configured_banned_calls(file, config, &DEFAULT_BANNED_CALLS)
            .into_iter()
            .filter(|call_match| zero_duration_arg(call_match).is_none())
            .map(|call_match| {
                self.diagnostic_at_node(path, &call_match.node, &[("call", &call_match.callee)])
            })
            .collect()
    }
}

impl CodeDetector for NoZeroSleepInTests {
    fn target(&self) -> RuleTarget {
        RuleTarget::TestsOnly
    }

    fn check_file(&self, path: &Path, file: &ParsedFile, config: &Config) -> Vec<Diagnostic> {
        self.find_configured_banned_calls(file, config, &DEFAULT_BANNED_CALLS)
            .into_iter()
            .filter_map(|call_match| {
                let zero_arg = zero_duration_arg(&call_match)?;
                Some(self.diagnostic_at_node(
                    path,
                    &call_match.node,
                    &[("call", &call_match.callee), ("arg", &zero_arg)],
                ))
            })
            .collect()
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(tests_no_sleep: NoSleepInTests, {
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
crate::test_utils::rule_test!(tests_no_zero_sleep: NoZeroSleepInTests, {
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
