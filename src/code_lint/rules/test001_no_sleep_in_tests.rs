//! TEST-001: Flags `sleep` calls in Python and Rust test files (`no-sleep-in-tests`).

use crate::code_lint::calls::{self, CallMatch};
use crate::code_lint::{CodeRule, RuleTarget, SourceDoc};
use crate::core::{Config, DenyListConfig, DynamicRuleConfig, FilterListDefaults, Rule};
use crate::diagnostic::{
    violation_template, Diagnostic, RuleCode, RuleName, SourceLocation, ViolationMessage,
    ViolationTemplate,
};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Static default banned sleep call patterns across Python and Rust.
const DEFAULT_BANNED_CALLS: FilterListDefaults = FilterListDefaults {
    base: &["sleep"],
    extend: &[
        (SupportLang::Python, &["time.sleep", "asyncio.sleep", "anyio.sleep", "trio.sleep"]),
        (
            SupportLang::Rust,
            &["thread::sleep", "std::thread::sleep", "time::sleep", "tokio::time::sleep"],
        ),
    ],
    exempt: &[],
};

/// Configuration for the `NoSleepInTests` rule.
pub type NoSleepInTestsConfig = DynamicRuleConfig<DenyListConfig>;

/// Rule that bans wall-clock and async sleeps in test files.
pub struct NoSleepInTests;

impl Rule for NoSleepInTests {
    fn code(&self) -> RuleCode {
        RuleCode("TEST-001")
    }

    fn name(&self) -> RuleName {
        RuleName("no-sleep-in-tests")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Testing]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }
}

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

fn format_violation_message(call_match: &CallMatch<'_>, lang: SupportLang) -> ViolationMessage {
    let call_name = &call_match.callee;
    zero_duration_arg(call_match).map_or_else(
        || SLEEP_TEMPLATE.render(lang, &[("call", call_name)]),
        |zero_arg| ZERO_SLEEP_TEMPLATE.render(lang, &[("call", call_name), ("arg", &zero_arg)]),
    )
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
            .map(|call_match| {
                let message = format_violation_message(&call_match, lang);
                self.create_diagnostic(
                    message,
                    SourceLocation::file_range(path, call_match.node.range()),
                )
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
            @r###"
        [TEST-001] Line 7, Col 5: Wall-clock or async sleep `time.sleep()` in test is discouraged.
        [TEST-001] Line 8, Col 11: Wall-clock or async sleep `asyncio.sleep()` in test is discouraged.
        [TEST-001] Line 9, Col 11: Wall-clock or async sleep `anyio.sleep()` in test is discouraged.
        [TEST-001] Line 10, Col 5: Wall-clock or async sleep `sleep()` in test is discouraged.
        [TEST-001] Line 11, Col 11: Zero-duration sleep `asyncio.sleep(0)` in test is discouraged.
        "###
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
            @r###"
        [TEST-001] Line 6, Col 5: Wall-clock or async sleep `std::thread::sleep()` in test is discouraged.
        [TEST-001] Line 7, Col 5: Wall-clock or async sleep `thread::sleep()` in test is discouraged.
        [TEST-001] Line 8, Col 5: Wall-clock or async sleep `tokio::time::sleep()` in test is discouraged.
        [TEST-001] Line 9, Col 5: Wall-clock or async sleep `sleep()` in test is discouraged.
        [TEST-001] Line 10, Col 5: Zero-duration sleep `tokio::time::sleep(Duration::ZERO)` in test is discouraged.
        "###
        );
    }

    #[test]
    fn test_zero_duration_distinct_suggestions() {
        let py_grep = AstGrep::new("await asyncio.sleep(0)", SupportLang::Python);
        let py_diags =
            NoSleepInTests.check_file(Path::new("tests/test_app.py"), &py_grep, &Config::default());
        assert_eq!(py_diags.len(), 1);
        assert!(py_diags[0].message.suggestion.contains("anyio.lowlevel.checkpoint()"));

        let rs_grep =
            AstGrep::new("fn test_it() { tokio::time::sleep(Duration::ZERO); }", SupportLang::Rust);
        let rs_diags =
            NoSleepInTests.check_file(Path::new("tests/app_test.rs"), &rs_grep, &Config::default());
        assert_eq!(rs_diags.len(), 1);
        assert!(rs_diags[0].message.suggestion.contains("tokio::task::yield_now().await"));
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
            "Expected TEST-001 to skip production file, got: {prod_diags:?}"
        );

        let test_diags =
            crate::code_lint::lint_file(Path::new("tests/test_daemon.py"), source, &config);
        assert_eq!(test_diags.len(), 1);
        assert_eq!(test_diags[0].rule_code, RuleCode("TEST-001"));
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
        assert_eq!(diags[0].rule_code, RuleCode("TEST-001"));
    }
}
