//! ASYNC-001: Flags unstructured task creation (`asyncio.create_task`, `ensure_future`, `loop.create_task`).

use crate::code_lint::{AstNode, CodeRule, RuleTarget, SourceDoc};
use crate::core::{Config, Rule};
use crate::diagnostic::{
    Diagnostic, LocationContext, RuleCode, RuleName, SourceLocation, SourceSpan, ViolationMessage,
};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Rule that bans unstructured asyncio task creation.
pub struct NoUnstructuredTaskCreation;

impl Rule for NoUnstructuredTaskCreation {
    fn code(&self) -> RuleCode {
        RuleCode("ASYNC-001")
    }

    fn name(&self) -> RuleName {
        RuleName("no-unstructured-task-creation")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Async, Tag::Python]
    }
}

/// Recursively collects all AST nodes of kind `"call"`.
fn collect_call_nodes<'a>(node: &AstNode<'a>, calls: &mut Vec<AstNode<'a>>) {
    if node.kind() == "call" {
        calls.push(node.clone());
    }
    for child in node.children() {
        collect_call_nodes(&child, calls);
    }
}

/// Inspects a `"call"` node to determine if it represents an unstructured task creation call.
fn check_unstructured_call(call_node: &AstNode<'_>) -> Option<String> {
    let func = call_node.field("function")?;
    match func.kind().as_ref() {
        "identifier" => {
            let name = func.text();
            if name == "create_task" || name == "ensure_future" {
                Some(name.to_string())
            } else {
                None
            }
        }
        "attribute" => {
            let attr = func.field("attribute")?;
            let attr_name = attr.text();
            let object = func.field("object")?;
            let obj_text = object.text();

            if attr_name == "ensure_future" {
                if obj_text == "asyncio" {
                    Some("asyncio.ensure_future".to_string())
                } else {
                    None
                }
            } else if attr_name == "create_task" {
                if obj_text == "asyncio"
                    || obj_text == "loop"
                    || obj_text == "event_loop"
                    || object.kind() == "call"
                {
                    Some(format!("{obj_text}.create_task"))
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

impl CodeRule for NoUnstructuredTaskCreation {
    fn target(&self) -> RuleTarget {
        RuleTarget::All
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }

    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<SourceDoc>,
        _config: &Config,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let mut call_nodes = Vec::new();
        collect_call_nodes(&grep.root(), &mut call_nodes);

        for call_node in call_nodes {
            if let Some(call_name) = check_unstructured_call(&call_node) {
                diagnostics.push(Diagnostic::new(
                    self.code(),
                    self.name(),
                    ViolationMessage {
                        summary: format!("Unstructured task creation `{call_name}()` is discouraged."),
                        rationale: "Unstructured background tasks can fail silently, leak upon cancellation, and introduce race conditions.".to_string(),
                        suggestion: "Use structured concurrency with AnyIO (`async with anyio.create_task_group() as tg: tg.start_soon(...)`) or Python 3.11+ TaskGroup (`async with asyncio.TaskGroup() as tg: tg.create_task(...)`).".to_string(),
                    },
                    SourceLocation {
                        context: LocationContext::File(path.to_path_buf()),
                        span: SourceSpan {
                            start: call_node.range().start,
                            end: call_node.range().end,
                        },
                    },
                ));
            }
        }

        diagnostics
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

        insta::assert_snapshot!(assert_code_rule_snapshot(&NoUnstructuredTaskCreation, source, "service.py"), @r###"
        [ASYNC-001] Line 5, Col 10: Unstructured task creation `asyncio.create_task()` is discouraged.
        [ASYNC-001] Line 6, Col 10: Unstructured task creation `asyncio.ensure_future()` is discouraged.
        [ASYNC-001] Line 7, Col 10: Unstructured task creation `create_task()` is discouraged.
        [ASYNC-001] Line 8, Col 10: Unstructured task creation `ensure_future()` is discouraged.
        [ASYNC-001] Line 9, Col 10: Unstructured task creation `loop.create_task()` is discouraged.
        [ASYNC-001] Line 10, Col 10: Unstructured task creation `event_loop.create_task()` is discouraged.
        [ASYNC-001] Line 11, Col 10: Unstructured task creation `asyncio.get_event_loop().create_task()` is discouraged.
        "###);
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
    task = asyncio.create_task(poll())  # omni:ignore [ASYNC-001] -- legacy daemon loop
";

        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("daemon.py"), source, &config);
        assert!(diags.is_empty(), "Expected 0 diagnostics with suppression, got: {diags:?}");
    }
}
