//! Flags wall-clock/async `sleep` calls (`no-sleep-in-tests`) and zero-duration sleeps (`no-zero-sleep-in-tests`) in test files.

use crate::code_lint::calls::{self, CallMatch};
use crate::code_lint::{CodeRule, RuleTarget, SourceDoc};
use crate::core::{Config, DenyListConfig, DynamicRuleConfig, FilterListDefaults, Rule, RuleName};
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

/// Configuration for the `NoSleepInTests` and `NoZeroSleepInTests` rules.
pub type NoSleepInTestsConfig = DynamicRuleConfig<DenyListConfig>;

const SLEEP_TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Wall-clock or async sleep `{call}()` in test is discouraged.",
    rationale: "Sleeping in tests slows down the test suite and introduces timing-dependent flakiness under load.",
    suggestion: {
        base: "Synchronize on deterministic signals or primitives (events, channels, conditions) or inject a virtual/fake clock (`clock.sleep(...)`).",
        Python => "Synchronize on deterministic signals (`anyio.Event`, `asyncio.Event`, or a queue/condition) or inject a virtual/fake clock (`clock.sleep(...)`).",
        Rust => "Synchronize on deterministic primitives (`tokio::sync::Notify`, channels, `Condvar`) or use virtual time (`tokio::time::pause()`) or an injected clock (`clock.sleep(...)`).",
    },
};

const ZERO_SLEEP_TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Zero-duration sleep `{call}({arg})` in test is discouraged.",
    rationale: "Zero-duration sleep is an indirect way to yield execution to the scheduler and obscures intent.",
    suggestion: {
        base: "To yield control to the scheduler or runtime without delaying, use an explicit yield or checkpoint primitive.",
        Python => "To yield control to the event loop without delaying, use `await anyio.lowlevel.checkpoint()`.",
        Rust => "To yield control to the executor without delaying, use `tokio::task::yield_now().await`.",
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
        let lang = *grep.lang();
        let rule_config: NoSleepInTestsConfig = config.get_rule_config(self.name().0);
        let effective_banned = rule_config.effective_banned_for_lang(lang, &DEFAULT_BANNED_CALLS);

        calls::find_banned_calls(grep, &effective_banned)
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
        let lang = *grep.lang();
        let rule_config: NoSleepInTestsConfig = config.get_rule_config(self.name().0);
        let effective_banned = rule_config.effective_banned_for_lang(lang, &DEFAULT_BANNED_CALLS);

        calls::find_banned_calls(grep, &effective_banned)
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
mod tests {
    use super::*;
    use crate::test_utils::assert_code_rule_snapshot;

    #[test]
    fn test_python_sleep_in_tests_flagged() {
        let source = r"
import asyncio
import anyio
import time

async def test_polling():
    time.sleep(1)
    await asyncio.sleep(0.5)
    await anyio.sleep(2)
    sleep(1)
    await asyncio.sleep(0)
    await fake_clock.sleep(10)
    self.sleep(5)
";

        insta::assert_snapshot!(
            assert_code_rule_snapshot(&NoSleepInTests, source, "tests/test_worker.py"),
            @"
        [no-sleep-in-tests] Line 7, Col 5: Wall-clock or async sleep `time.sleep()` in test is discouraged.
        [no-sleep-in-tests] Line 8, Col 11: Wall-clock or async sleep `asyncio.sleep()` in test is discouraged.
        [no-sleep-in-tests] Line 9, Col 11: Wall-clock or async sleep `anyio.sleep()` in test is discouraged.
        [no-sleep-in-tests] Line 10, Col 5: Wall-clock or async sleep `sleep()` in test is discouraged.
        "
        );

        insta::assert_snapshot!(
            assert_code_rule_snapshot(&NoZeroSleepInTests, source, "tests/test_worker.py"),
            @"[no-zero-sleep-in-tests] Line 11, Col 11: Zero-duration sleep `asyncio.sleep(0)` in test is discouraged."
        );
    }

    #[test]
    fn test_rust_sleep_in_tests_flagged() {
        let source = r"
use std::time::Duration;

#[tokio::test]
async fn test_retry_backoff() {
    std::thread::sleep(Duration::from_millis(50));
    thread::sleep(Duration::from_secs(1));
    tokio::time::sleep(Duration::from_millis(10)).await;
    sleep(Duration::from_millis(5));
    tokio::time::sleep(Duration::ZERO).await;
    fake_clock.sleep(Duration::from_secs(5)).await;
}
";

        insta::assert_snapshot!(
            assert_code_rule_snapshot(&NoSleepInTests, source, "tests/retry_test.rs"),
            @"
        [no-sleep-in-tests] Line 6, Col 5: Wall-clock or async sleep `std::thread::sleep()` in test is discouraged.
        [no-sleep-in-tests] Line 7, Col 5: Wall-clock or async sleep `thread::sleep()` in test is discouraged.
        [no-sleep-in-tests] Line 8, Col 5: Wall-clock or async sleep `tokio::time::sleep()` in test is discouraged.
        [no-sleep-in-tests] Line 9, Col 5: Wall-clock or async sleep `sleep()` in test is discouraged.
        "
        );

        insta::assert_snapshot!(
            assert_code_rule_snapshot(&NoZeroSleepInTests, source, "tests/retry_test.rs"),
            @"[no-zero-sleep-in-tests] Line 10, Col 5: Zero-duration sleep `tokio::time::sleep(Duration::ZERO)` in test is discouraged."
        );
    }

    #[test]
    fn test_production_files_skipped_by_tests_only_target() {
        let source = r"
import time

def run_daemon():
    time.sleep(5)
";
        let config = Config::default();
        let prod_diags = crate::code_lint::lint_file(Path::new("src/daemon.py"), source, &config);
        assert!(
            prod_diags.is_empty(),
            "Expected no-sleep-in-tests to skip production file, got: {prod_diags:?}"
        );

        let test_diags =
            crate::code_lint::lint_file(Path::new("tests/test_daemon.py"), source, &config);
        assert_eq!(test_diags.len(), 1);
        assert_eq!(test_diags[0].rule_name, RuleName("no-sleep-in-tests"));
    }

    #[test]
    fn test_rust_inline_conditional_test_scoped_in_source_file() {
        let source = r"
pub fn run_worker() {
    std::thread::sleep(std::time::Duration::from_secs(1));
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_worker() {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("src/worker.rs"), source, &config);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_name, RuleName("no-sleep-in-tests"));
    }
}
