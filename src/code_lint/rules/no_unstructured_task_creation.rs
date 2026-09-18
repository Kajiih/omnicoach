//! Flags unstructured task creation (`asyncio.create_task`, `ensure_future`, `loop.create_task`).

use crate::code_lint::{calls, CodeRule, RuleTarget, SourceDoc};
use crate::core::{Config, DenyListConfig, DynamicRuleConfig, FilterListDefaults, Rule};
use crate::diagnostic::{
    Diagnostic, LocationContext, RuleName, SourceLocation, SourceSpan, ViolationMessage,
};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Static default banned unstructured task creation call patterns.
const DEFAULT_BANNED_CALLS: FilterListDefaults = FilterListDefaults {
    base: &[],
    extend: &[(
        SupportLang::Python,
        &[
            "create_task",
            "ensure_future",
            "asyncio.create_task",
            "asyncio.ensure_future",
            "loop.create_task",
            "event_loop.create_task",
            "$LOOP($$$LOOP_ARGS).create_task",
        ],
    )],
    exempt: &[],
};

/// Configuration for the `NoUnstructuredTaskCreation` rule.
pub type NoUnstructuredTaskCreationConfig = DynamicRuleConfig<DenyListConfig>;

/// Rule that bans unstructured asyncio task creation.
pub struct NoUnstructuredTaskCreation;

impl Rule for NoUnstructuredTaskCreation {
    fn name(&self) -> RuleName {
        RuleName("no-unstructured-task-creation")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Async]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }
}

impl CodeRule for NoUnstructuredTaskCreation {
    fn target(&self) -> RuleTarget {
        RuleTarget::All
    }

    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<SourceDoc>,
        config: &Config,
    ) -> Vec<Diagnostic> {
        let rule_config: NoUnstructuredTaskCreationConfig = config.get_rule_config(self.name().0);
        let effective_banned =
            rule_config.effective_banned_for_lang(*grep.lang(), &DEFAULT_BANNED_CALLS);

        calls::find_banned_calls(grep, &effective_banned)
            .into_iter()
            .map(|call_match| {
                let call_name = call_match.callee;
                Diagnostic::new(
                    self.name(),
                    ViolationMessage {
                        summary: format!("Unstructured task creation `{call_name}()` is discouraged."),
                        rationale: "Unstructured background tasks can fail silently, leak upon cancellation, and introduce race conditions.".to_string(),
                        suggestion: "Use structured concurrency with AnyIO (`async with anyio.create_task_group() as tg: tg.start_soon(...)`) or Python 3.11+ TaskGroup (`async with asyncio.TaskGroup() as tg: tg.create_task(...)`).".to_string(),
                    },
                    SourceLocation {
                        context: LocationContext::File(path.to_path_buf()),
                        span: SourceSpan {
                            start: call_match.node.range().start,
                            end: call_match.node.range().end,
                        },
                    },
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
    fn test_unstructured_calls_flagged() {
        let source = r"
import asyncio

async def worker():
    t1 = asyncio.create_task(do_work())
    t2 = asyncio.ensure_future(do_work())
    t3 = create_task(do_work())
    t4 = ensure_future(do_work())
    t5 = loop.create_task(do_work())
    t6 = event_loop.create_task(do_work())
    t7 = asyncio.get_event_loop().create_task(do_work())
";

        insta::assert_snapshot!(assert_code_rule_snapshot(&NoUnstructuredTaskCreation, source, "service.py"), @"
        [no-unstructured-task-creation] Line 5, Col 10: Unstructured task creation `asyncio.create_task()` is discouraged.
        [no-unstructured-task-creation] Line 6, Col 10: Unstructured task creation `asyncio.ensure_future()` is discouraged.
        [no-unstructured-task-creation] Line 7, Col 10: Unstructured task creation `create_task()` is discouraged.
        [no-unstructured-task-creation] Line 8, Col 10: Unstructured task creation `ensure_future()` is discouraged.
        [no-unstructured-task-creation] Line 9, Col 10: Unstructured task creation `loop.create_task()` is discouraged.
        [no-unstructured-task-creation] Line 10, Col 10: Unstructured task creation `event_loop.create_task()` is discouraged.
        [no-unstructured-task-creation] Line 11, Col 10: Unstructured task creation `asyncio.get_event_loop().create_task()` is discouraged.
        ");
    }

    #[test]
    fn test_structured_concurrency_allowed() {
        let source = r"
import anyio
import asyncio

async def handle_requests():
    async with anyio.create_task_group() as tg:
        tg.start_soon(process_one)
        tg.start_soon(process_two)

    async with asyncio.TaskGroup() as task_group:
        task_group.create_task(process_three)

    tg.create_task(custom_task)
    my_obj.create_task(other_task)
";

        let output = assert_code_rule_snapshot(&NoUnstructuredTaskCreation, source, "service.py");
        assert!(output.is_empty(), "Expected 0 violations, got:\n{output}");
    }

    #[test]
    fn test_inline_suppression_silences_rule() {
        let source = r"
import asyncio

async def background_poller():
    task = asyncio.create_task(poll())  # omni:ignore [no-unstructured-task-creation] -- legacy daemon loop
";

        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("daemon.py"), source, &config);
        assert!(diags.is_empty(), "Expected 0 diagnostics with suppression, got: {diags:?}");
    }

    #[test]
    fn test_config_extend_and_allowed() {
        let source = r"
async def worker():
    custom_scheduler.spawn_background(do_work())
    loop.create_task(do_work())
";
        let config_toml = r#"
[rules.no-unstructured-task-creation]
extend_banned = ["custom_scheduler.spawn_background"]
allowed = ["loop.create_task"]
"#;
        let config: Config = toml::from_str(config_toml).unwrap();
        let output = crate::test_utils::assert_code_rule_snapshot_with_config(
            &NoUnstructuredTaskCreation,
            source,
            "service.py",
            &config,
        );
        insta::assert_snapshot!(output, @"[no-unstructured-task-creation] Line 3, Col 5: Unstructured task creation `custom_scheduler.spawn_background()` is discouraged.");
    }
}
