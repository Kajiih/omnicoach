//! Flags unstructured task creation (`asyncio.create_task`, `ensure_future`, `loop.create_task`).

use crate::code_lint::{CodeRule, RuleTarget, SourceDoc};
use crate::core::{Config, FilterListDefaults, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
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

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Unstructured background task call `{callee}()`.",
    rationale: "Fire-and-forget tasks outlive their spawning scope and silently drop unhandled exceptions when not awaited, leaking resources on cancellation or shutdown.",
    suggestion: "Spawn concurrent tasks within an `asyncio.TaskGroup` (`async with asyncio.TaskGroup() as tg: tg.create_task(...)`) or `anyio.create_task_group()` so task lifetimes are bounded to the enclosing block.",
};

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

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
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
        self.check_banned_calls(path, grep, config, &DEFAULT_BANNED_CALLS)
    }
}

#[cfg(test)]
crate::rule_test!(
    NoUnstructuredTaskCreation,
    {
        Python => {
            pass: [
                task_group_allowed => r#"
                    async def handle_requests():
                        async with asyncio.TaskGroup() as tg:
                            tg.create_task(process_item())
                "#,
                anyio_task_group_allowed => r#"
                    async def handle_requests():
                        async with anyio.create_task_group() as tg:
                            tg.start_soon(process_item)
                "#,
                unrelated_method_allowed => r#"
                    def run(scheduler):
                        scheduler.create_task_record("job")
                "#,
            ],
            fail: [
                asyncio_create_task => r#"
                    import asyncio

                    async def handle():
                        asyncio.create_task(background_sync())
                "# => "asyncio.create_task(background_sync())",
                asyncio_ensure_future => r#"
                    import asyncio

                    async def handle():
                        asyncio.ensure_future(legacy_job())
                "# => "asyncio.ensure_future(legacy_job())",
                bare_create_task => r#"
                    from asyncio import create_task

                    async def handle():
                        create_task(background_sync())
                "# => "create_task(background_sync())",
                bare_ensure_future => r#"
                    from asyncio import ensure_future

                    async def handle():
                        ensure_future(legacy_job())
                "# => "ensure_future(legacy_job())",
                loop_create_task => r#"
                    async def handle(loop):
                        loop.create_task(worker())
                "# => "loop.create_task(worker())",
                event_loop_create_task => r#"
                    async def handle(event_loop):
                        event_loop.create_task(worker())
                "# => "event_loop.create_task(worker())",
                get_running_loop_create_task => r#"
                    import asyncio

                    async def handle():
                        asyncio.get_running_loop().create_task(worker())
                "# => "asyncio.get_running_loop().create_task(worker())",
            ],
        },
    }
);
