//! Flags wall-clock/async `sleep` calls (`no-sleep-in-tests`) and zero-duration sleeps (`no-zero-sleep-in-tests`) in test files.

use crate::code_lint::calls::CallMatch;
use crate::code_lint::{CodeRule, RuleTarget, SourceDoc};
use crate::core::{Config, FilterListDefaults, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
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

impl Rule for NoSleepInTests {
    fn name(&self) -> RuleName {
        RuleName("no-sleep-in-tests")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Testing]
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

impl Rule for NoZeroSleepInTests {
    fn name(&self) -> RuleName {
        RuleName("no-zero-sleep-in-tests")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Testing]
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
    if call_match.arguments.len() != 1 {
        return None;
    }
    let arg_text = call_match.arguments[0].text();
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

impl CodeRule for NoSleepInTests {
    fn target(&self) -> RuleTarget {
        RuleTarget::TestsOnly
    }

    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<SourceDoc>,
        config: &Config,
    ) -> Vec<Diagnostic> {
        self.find_configured_banned_calls(grep, config, &DEFAULT_BANNED_CALLS)
            .into_iter()
            .filter(|call_match| zero_duration_arg(call_match).is_none())
            .map(|call_match| {
                self.diagnostic_at_node(path, &call_match.node, &[("call", &call_match.callee)])
            })
            .collect()
    }
}

impl CodeRule for NoZeroSleepInTests {
    fn target(&self) -> RuleTarget {
        RuleTarget::TestsOnly
    }

    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<SourceDoc>,
        config: &Config,
    ) -> Vec<Diagnostic> {
        self.find_configured_banned_calls(grep, config, &DEFAULT_BANNED_CALLS)
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
crate::rule_test!(tests_no_sleep: NoSleepInTests, {
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
crate::rule_test!(tests_no_zero_sleep: NoZeroSleepInTests, {
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
