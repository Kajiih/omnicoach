//! Flags unstructured task creation (`asyncio.create_task`, `ensure_future`, `loop.create_task`).

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::rule::{CodeDetector, RuleTarget};
use crate::core::{
    Detector, FilterListDefaults, ListKind, ListOption, OptionSpec, ResolvedOptions, RuleOptions,
};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::Rule;
use crate::rule_documentation::{Reference, RuleDoc};
use crate::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Topic};
use ast_grep_language::SupportLang;
use std::path::Path;

const BANNED_CALLS: ListOption = ListOption {
    kind: ListKind::Deny,
    doc: "Task creation calls flagged when called.",
    default: FilterListDefaults {
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
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Unstructured background task call `{callee}()`.",
    rationale: "Fire-and-forget tasks outlive their spawning scope and silently drop unhandled exceptions when not awaited, leaking resources on cancellation or shutdown.",
    suggestion: "Spawn concurrent tasks within an `asyncio.TaskGroup` (`async with asyncio.TaskGroup() as tg: tg.create_task(...)`) or `anyio.create_task_group()` so task lifetimes are bounded to the enclosing block.",
};

/// Rule that bans unstructured asyncio task creation.
struct NoUnstructuredTaskCreation;

/// The rule's declaration.
pub const RULE: Rule<dyn CodeDetector> = Rule {
    detector: &NoUnstructuredTaskCreation,
    classification: Classification {
        topics: &[Topic::ASYNC],
        precision: Precision::Heuristic,
        consensus: Consensus::Opinionated,
        impacted_quality: ImpactedQuality::Reliability,
    },
    doc: RuleDoc {
        summary: "Flags asyncio tasks spawned outside a task group.",
        what_it_does: "Flags Python calls that start a background task with no enclosing \
                       scope: `asyncio.create_task`, `asyncio.ensure_future`, a bare \
                       `create_task` or `ensure_future`, `loop.create_task`, \
                       `event_loop.create_task`, and `.create_task` on the result of a call, \
                       such as `asyncio.get_running_loop().create_task(...)`. Calls on a \
                       task group, such as `tg.create_task(...)` or `tg.start_soon(...)`, \
                       are not flagged. The rule matches the call alone: a task that is \
                       stored and awaited later is flagged too. It runs on source and test \
                       files.",
        why_is_this_bad: "A task started this way is not tied to the code that started \
                          it. The event loop keeps only a weak reference to it, so an \
                          unreferenced task can be garbage-collected before it finishes. If \
                          nobody awaits it, its exception is only logged when the task is \
                          destroyed, and it keeps running after its caller returns, fails \
                          or is cancelled.\n\n\
                          Start concurrent work inside `async with asyncio.TaskGroup() as \
                          tg:` or `anyio.create_task_group()`: the block waits for every \
                          task, cancels the others when one fails, and raises their \
                          errors.",
        references: &[
            Reference {
                title: "Python docs: asyncio.create_task",
                url: "https://docs.python.org/3/library/asyncio-task.html#asyncio.create_task",
            },
            Reference {
                title: "Python docs: asyncio Task Groups",
                url: "https://docs.python.org/3/library/asyncio-task.html#task-groups",
            },
        ],
    },
    options: RuleOptions {
        options: &[OptionSpec::List(&BANNED_CALLS)],
        ..RuleOptions::CODE_RULE
    },
};

impl Detector for NoUnstructuredTaskCreation {
    fn name(&self) -> RuleName {
        RuleName("no-unstructured-task-creation")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeDetector for NoUnstructuredTaskCreation {
    fn target(&self) -> RuleTarget {
        RuleTarget::All
    }

    fn check_file(
        &self,
        path: &Path,
        file: &ParsedFile,
        options: &ResolvedOptions<'_>,
    ) -> Vec<Diagnostic> {
        self.check_banned_calls(path, file, &options.list(&BANNED_CALLS))
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
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
