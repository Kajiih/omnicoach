//! Flags unstructured task creation (`asyncio.create_task`, `ensure_future`, `loop.create_task`).

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
    doc: "Task creation calls flagged when called.",
    default: FilterListDefaults {
        base: &[
            "create_task",
            "ensure_future",
            "asyncio.create_task",
            "asyncio.ensure_future",
            "loop.create_task",
            "event_loop.create_task",
            "$LOOP($$$LOOP_ARGS).create_task",
        ],
        extend: &[],
        remove: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "`{callee}()` spawns a task outside a task group.",
    rationale: "A task spawned outside a task group is bound to no scope: when a sibling fails or the caller is cancelled before awaiting it, it keeps running orphaned in the background.",
    suggestion: "Spawn the task inside an `asyncio.TaskGroup` (`async with asyncio.TaskGroup() as group: group.create_task(...)`) or an `anyio.create_task_group()` block, so its lifetime ends with the block.",
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("unstructured-task"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(BANNED),
        classification: Classification {
            topics: &[Topic::CONCURRENCY],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags asyncio tasks spawned outside a task group.",
            what_it_does: indoc::indoc! {r"
                Flags Python calls that start a background task with no enclosing task group:
                `asyncio.create_task`, `asyncio.ensure_future`, a bare `create_task` or
                `ensure_future`, `loop.create_task`, `event_loop.create_task`, and `.create_task` on
                the result of a call, such as `asyncio.get_running_loop().create_task(...)`. Calls
                on a task group, such as `tg.create_task(...)` or `tg.start_soon(...)`, are not
                flagged. The rule matches the call alone: a task that is stored in a variable and
                awaited later is flagged too. It runs on source and test files."},
            why_is_this_bad: indoc::indoc! {r"
                A task started with `create_task` or `ensure_future` is not bound to a lexical
                scope. Even when assigned to a variable and awaited later, if an earlier statement
                raises or the caller is cancelled before reaching that `await`, the task is not
                cancelled and keeps running in the background; conversely, if the task fails early,
                its exception sits unobserved until the caller reaches `await task` (or is lost
                entirely if unreferenced).

                Start concurrent work inside `async with asyncio.TaskGroup() as tg:` or
                `anyio.create_task_group()`: the block waits for every task on exit, cancels sibling
                tasks immediately when one fails, and propagates their errors."},
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
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r"
                    async def refresh_dashboards(dashboards):
                        for dashboard in dashboards:
                            asyncio.create_task(dashboard.refresh())
                "},
                flagged_span: "asyncio.create_task(dashboard.refresh())",
                fixed: indoc::indoc! {r"
                    async def refresh_dashboards(dashboards):
                        async with asyncio.TaskGroup() as group:
                            for dashboard in dashboards:
                                group.create_task(dashboard.refresh())
                "},
            }],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

fn check_file(
    rule: &CodeRule<ListOption>,
    path: &Path,
    file: &ParsedFile,
    banned: &HashSet<String>,
) -> Vec<Diagnostic> {
    rule.check_banned_calls(path, file, banned)
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
