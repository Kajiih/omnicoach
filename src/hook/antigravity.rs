//! Adapter for Antigravity agent lifecycle hooks (`hooks.json`).
//!
//! Antigravity's `PreToolUse` hook can only allow or deny a tool call; it cannot return text to
//! the model. Code lint warnings on a file edit are therefore queued in the conversation's
//! artifact directory and injected as an ephemeral message on the next `PreInvocation`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::event::{PreToolDecision, PreToolUseEvent, ToolCallPayload};
use super::runner::evaluate_pre_tool_use;
use super::warning_queue::WarningQueue;

/// Subdirectory of the conversation's artifact directory that Antigravity reserves for scratch
/// files, used to hold the warning queue.
const ARTIFACT_SCRATCH_SUBDIR: &str = "scratch";

/// Antigravity lifecycle event that invoked the hook, passed on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum AntigravityEvent {
    /// Fired before the agent executes a tool call.
    #[value(name = "pre-tool-use")]
    PreToolUse,
    /// Fired before the model is invoked for a new reasoning step.
    #[value(name = "pre-invocation")]
    PreInvocation,
}

/// Errors encountered while parsing or serializing Antigravity hook payloads.
#[derive(Debug, thiserror::Error)]
pub enum AntigravityProtocolError {
    /// JSON deserialization or serialization failure.
    #[error("JSON protocol error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Runs the hook for an Antigravity event received as JSON on stdin, returning the JSON response.
///
/// # Errors
///
/// Returns [`AntigravityProtocolError`] if parsing input JSON or serializing response JSON fails.
pub fn run_antigravity_hook(
    event: AntigravityEvent,
    json_input: &str,
) -> Result<String, AntigravityProtocolError> {
    match event {
        AntigravityEvent::PreToolUse => {
            let input = parse_pre_tool_use(json_input)?;
            let outcome = evaluate_pre_tool_use(&input.tool_call);
            if let Some(queue) = &input.warning_queue {
                queue.push(&outcome.warnings);
            }
            serialize_pre_tool_decision(&outcome.decision)
        }
        AntigravityEvent::PreInvocation => {
            let warnings = parse_pre_invocation(json_input)?
                .map(|queue| queue.drain())
                .unwrap_or_default();
            serialize_pre_invocation(&warnings)
        }
    }
}

/// A `PreToolUse` payload translated into the agent-neutral tool call, plus the queue where the
/// warnings it produces wait for the next `PreInvocation`.
#[derive(Debug, PartialEq, Eq)]
struct PreToolUseInput {
    tool_call: PreToolUseEvent,
    warning_queue: Option<WarningQueue>,
}

fn parse_pre_tool_use(json_text: &str) -> Result<PreToolUseInput, AntigravityProtocolError> {
    let raw: RawPreToolUse = serde_json::from_str(json_text)?;
    let artifact_directory_path = non_empty_trimmed(raw.artifact_directory_path).map(PathBuf::from);
    let workspace_paths: Vec<PathBuf> = raw
        .workspace_paths
        .into_iter()
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .collect();

    Ok(PreToolUseInput {
        warning_queue: artifact_directory_path
            .as_deref()
            .map(artifact_warning_queue),
        tool_call: PreToolUseEvent {
            workspace_paths,
            agent_owned_dir: artifact_directory_path,
            payload: parse_tool_payload(raw.tool_call),
        },
    })
}

fn parse_pre_invocation(json_text: &str) -> Result<Option<WarningQueue>, AntigravityProtocolError> {
    let raw: RawPreInvocation = serde_json::from_str(json_text)?;
    Ok(non_empty_trimmed(raw.artifact_directory_path)
        .as_deref()
        .map(|artifact_directory_path| artifact_warning_queue(Path::new(artifact_directory_path))))
}

fn artifact_warning_queue(artifact_directory_path: &Path) -> WarningQueue {
    WarningQueue::in_dir(&artifact_directory_path.join(ARTIFACT_SCRATCH_SUBDIR))
}

fn serialize_pre_tool_decision(
    decision: &PreToolDecision,
) -> Result<String, AntigravityProtocolError> {
    let wire = match decision {
        PreToolDecision::Allow => WirePreToolResult {
            decision: "allow",
            reason: None,
        },
        PreToolDecision::Deny { reason } => WirePreToolResult {
            decision: "deny",
            reason: Some(reason.clone()),
        },
    };
    Ok(serde_json::to_string(&wire)?)
}

fn serialize_pre_invocation(warnings: &[String]) -> Result<String, AntigravityProtocolError> {
    let inject_steps = if warnings.is_empty() {
        Vec::new()
    } else {
        let warnings = warnings.join("\n\n");
        vec![WireInjectStep {
            ephemeral_message: indoc::formatdoc! {"
                [omnicoach] The previous file modification introduced code lint warnings on modified lines. Please review and fix them if appropriate:

                {warnings}"},
        }]
    };
    Ok(serde_json::to_string(&WirePreInvocationResult {
        inject_steps,
    })?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPreToolUse {
    #[serde(default)]
    workspace_paths: Vec<String>,
    #[serde(default)]
    artifact_directory_path: Option<String>,
    #[serde(default)]
    tool_call: Option<RawToolCall>,
}

#[derive(Debug, Deserialize)]
struct RawToolCall {
    name: String,
    #[serde(default)]
    args: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPreInvocation {
    #[serde(default)]
    artifact_directory_path: Option<String>,
}

fn parse_tool_payload(tool_call: Option<RawToolCall>) -> ToolCallPayload {
    let Some(call) = tool_call else {
        return ToolCallPayload::Other {
            tool_name: String::new(),
        };
    };

    match call.name.as_str() {
        "run_command" => serde_json::from_value::<RunCommandArgs>(call.args).map_or_else(
            |_| ToolCallPayload::Other {
                tool_name: call.name,
            },
            |args| ToolCallPayload::RunCommand {
                command_line: args.command_line,
                cwd: non_empty_trimmed(args.cwd).map(PathBuf::from),
            },
        ),
        "write_file" => serde_json::from_value::<WriteFileArgs>(call.args).map_or_else(
            |_| ToolCallPayload::Other {
                tool_name: call.name,
            },
            |args| ToolCallPayload::WriteFile {
                path: args.path,
                content: args.content,
                append: args.append,
            },
        ),
        "edit_file" => serde_json::from_value::<EditFileArgs>(call.args).map_or_else(
            |_| ToolCallPayload::Other {
                tool_name: call.name,
            },
            |args| ToolCallPayload::EditFile {
                path: args.path,
                target: args.target,
                replacement: args.replacement,
            },
        ),
        _ => ToolCallPayload::Other {
            tool_name: call.name,
        },
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RunCommandArgs {
    command_line: String,
    #[serde(default)]
    cwd: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WriteFileArgs {
    path: PathBuf,
    content: String,
    #[serde(default)]
    append: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct EditFileArgs {
    path: PathBuf,
    target: String,
    replacement: String,
}

fn non_empty_trimmed(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WirePreToolResult {
    decision: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WirePreInvocationResult {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    inject_steps: Vec<WireInjectStep>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireInjectStep {
    ephemeral_message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pre_tool_use_run_command() {
        let json_text = indoc::indoc! {r#"
            {
                "workspacePaths": ["/home/user/repo"],
                "artifactDirectoryPath": "/tmp/brain",
                "toolCall": {
                    "name": "run_command",
                    "args": {
                        "CommandLine": "jj status",
                        "Cwd": "/home/user/repo"
                    }
                }
            }
        "#};
        assert_eq!(
            parse_pre_tool_use(json_text).expect("parse succeeds"),
            PreToolUseInput {
                tool_call: PreToolUseEvent {
                    workspace_paths: vec![PathBuf::from("/home/user/repo")],
                    agent_owned_dir: Some(PathBuf::from("/tmp/brain")),
                    payload: ToolCallPayload::RunCommand {
                        command_line: "jj status".to_owned(),
                        cwd: Some(PathBuf::from("/home/user/repo")),
                    },
                },
                warning_queue: Some(WarningQueue::in_dir(Path::new("/tmp/brain/scratch"))),
            }
        );
    }

    #[test]
    fn parse_pre_tool_use_write_file() {
        let json_text = indoc::indoc! {r#"
            {
                "toolCall": {
                    "name": "write_file",
                    "args": {
                        "Path": "src/main.rs",
                        "Content": "fn main() {}\n",
                        "Append": true
                    }
                }
            }
        "#};
        assert_eq!(
            parse_pre_tool_use(json_text).expect("parse succeeds"),
            PreToolUseInput {
                tool_call: PreToolUseEvent {
                    workspace_paths: Vec::new(),
                    agent_owned_dir: None,
                    payload: ToolCallPayload::WriteFile {
                        path: PathBuf::from("src/main.rs"),
                        content: "fn main() {}\n".to_owned(),
                        append: true,
                    },
                },
                warning_queue: None,
            }
        );
    }

    #[test]
    fn parse_pre_tool_use_edit_file() {
        let json_text = indoc::indoc! {r#"
            {
                "toolCall": {
                    "name": "edit_file",
                    "args": {
                        "Path": "src/main.rs",
                        "Target": "old",
                        "Replacement": "new"
                    }
                }
            }
        "#};
        assert_eq!(
            parse_pre_tool_use(json_text).expect("parse succeeds"),
            PreToolUseInput {
                tool_call: PreToolUseEvent {
                    workspace_paths: Vec::new(),
                    agent_owned_dir: None,
                    payload: ToolCallPayload::EditFile {
                        path: PathBuf::from("src/main.rs"),
                        target: "old".to_owned(),
                        replacement: "new".to_owned(),
                    },
                },
                warning_queue: None,
            }
        );
    }

    #[test]
    fn parse_pre_invocation_locates_the_artifact_queue() {
        let json_text = r#"{"artifactDirectoryPath":"/tmp/brain"}"#;
        assert_eq!(
            parse_pre_invocation(json_text).expect("parse succeeds"),
            Some(WarningQueue::in_dir(Path::new("/tmp/brain/scratch")))
        );
    }

    #[test]
    fn parse_handles_unpopulated_empty_strings() {
        let json_text = indoc::indoc! {r#"
            {
                "artifactDirectoryPath": "",
                "workspacePaths": ["", "  "]
            }
        "#};
        assert_eq!(
            parse_pre_invocation(json_text).expect("parse succeeds"),
            None
        );
    }

    #[test]
    fn serialize_pre_tool_decision_allow() {
        let serialized =
            serialize_pre_tool_decision(&PreToolDecision::Allow).expect("serialization succeeds");
        assert_eq!(serialized, r#"{"decision":"allow"}"#);
    }

    #[test]
    fn serialize_pre_tool_decision_deny() {
        let serialized = serialize_pre_tool_decision(&PreToolDecision::Deny {
            reason: "blocked command".to_owned(),
        })
        .expect("serialization succeeds");
        assert_eq!(
            serialized,
            r#"{"decision":"deny","reason":"blocked command"}"#
        );
    }

    #[test]
    fn serialize_pre_invocation_injects_warnings_as_one_step() {
        let serialized = serialize_pre_invocation(&["first".to_owned(), "second".to_owned()])
            .expect("serialization succeeds");
        let expected_message = indoc::indoc! {"
            [omnicoach] The previous file modification introduced code lint warnings on modified lines. Please review and fix them if appropriate:

            first

            second"};
        assert_eq!(
            serialized,
            serde_json::json!({"injectSteps": [{"ephemeralMessage": expected_message}]})
                .to_string()
        );
    }

    #[test]
    fn serialize_pre_invocation_without_warnings_is_empty() {
        let serialized = serialize_pre_invocation(&[]).expect("serialization succeeds");
        assert_eq!(serialized, "{}");
    }
}
