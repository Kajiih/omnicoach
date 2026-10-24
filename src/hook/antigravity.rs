//! Adapter for Antigravity agent lifecycle hooks (`hooks.json`).
//!
//! Antigravity's `PreToolUse` hook can only allow or deny a tool call; it cannot return text to
//! the model. Code lint warnings on a file edit are therefore queued in the conversation's
//! artifact directory and injected as an ephemeral message on the next `PreInvocation`.

use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::event::{FileEdit, PreToolDecision, PreToolUseEvent, TextReplacement, ToolCallPayload};
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
    match decision {
        PreToolDecision::NoObjection => Ok(String::new()),
        PreToolDecision::Deny { reason } => Ok(serde_json::to_string(&WirePreToolResult {
            decision: "deny",
            reason,
        })?),
    }
}

fn serialize_pre_invocation(warnings: &[String]) -> Result<String, AntigravityProtocolError> {
    if warnings.is_empty() {
        return Ok(String::new());
    }
    let warnings = warnings.join("\n\n");
    let inject_steps = vec![WireInjectStep {
        ephemeral_message: indoc::formatdoc! {"
            [omnicoach] Proposed file modification(s) introduced code lint warnings on modified lines. Please review and fix them if appropriate:

            {warnings}"},
    }];
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
        return ToolCallPayload::Other;
    };

    match call.name.as_str() {
        "run_command" => parse_run_command_args(&call.args, &call.name),
        "write_file" => parse_write_file_args(&call.args, &call.name),
        "write_to_file" => parse_write_to_file_args(&call.args, &call.name),
        "edit_file" => parse_edit_file_args(&call.args, &call.name),
        "replace_file_content" | "multi_replace_file_content" => {
            parse_replace_content_args(&call.args, &call.name)
        }
        _ => ToolCallPayload::Other,
    }
}

fn parse_run_command_args(args: &serde_json::Value, tool_name: &str) -> ToolCallPayload {
    let Some(parsed) = deserialize_tool_args::<RunCommandArgs>(args, tool_name) else {
        return ToolCallPayload::Other;
    };
    ToolCallPayload::RunCommand {
        command_line: parsed.command_line,
        cwd: non_empty_trimmed(parsed.cwd).map(PathBuf::from),
    }
}

fn parse_write_file_args(args: &serde_json::Value, tool_name: &str) -> ToolCallPayload {
    let Some(parsed) = deserialize_tool_args::<WriteFileArgs>(args, tool_name) else {
        return ToolCallPayload::Other;
    };
    ToolCallPayload::FileEdit {
        path: parsed.path,
        edit: if parsed.append {
            FileEdit::Append(parsed.content)
        } else {
            FileEdit::Overwrite(parsed.content)
        },
    }
}

fn parse_write_to_file_args(args: &serde_json::Value, tool_name: &str) -> ToolCallPayload {
    let Some(parsed) = deserialize_tool_args::<WriteToFileArgs>(args, tool_name) else {
        return ToolCallPayload::Other;
    };
    ToolCallPayload::FileEdit {
        path: parsed.target_file,
        edit: FileEdit::Overwrite(parsed.code_content),
    }
}

fn parse_edit_file_args(args: &serde_json::Value, tool_name: &str) -> ToolCallPayload {
    let Some(parsed) = deserialize_tool_args::<EditFileArgs>(args, tool_name) else {
        return ToolCallPayload::Other;
    };

    let replacements = if !parsed.edits.is_empty() {
        parsed
            .edits
            .into_iter()
            .map(|hunk| TextReplacement {
                target: hunk.target,
                replacement: hunk.replacement,
                line_range: None,
                allow_multiple: true,
            })
            .collect()
    } else if !parsed.target.is_empty() || !parsed.replacement.is_empty() {
        vec![TextReplacement {
            target: parsed.target,
            replacement: parsed.replacement,
            line_range: None,
            allow_multiple: true,
        }]
    } else {
        eprintln!(
            "[omnicoach hook] ignoring malformed {tool_name} args: missing target/replacement"
        );
        return ToolCallPayload::Other;
    };

    ToolCallPayload::FileEdit {
        path: parsed.path,
        edit: FileEdit::Replace(replacements),
    }
}

fn parse_replace_content_args(args: &serde_json::Value, tool_name: &str) -> ToolCallPayload {
    let Some(parsed) = deserialize_tool_args::<ReplaceContentArgs>(args, tool_name) else {
        return ToolCallPayload::Other;
    };

    let replacements = if !parsed.replacement_chunks.is_empty() {
        parsed
            .replacement_chunks
            .into_iter()
            .map(|chunk| TextReplacement {
                target: chunk.target_content,
                replacement: chunk.replacement_content,
                line_range: optional_line_range(chunk.start_line, chunk.end_line),
                allow_multiple: chunk.allow_multiple,
            })
            .collect()
    } else if !parsed.target_content.is_empty() || !parsed.replacement_content.is_empty() {
        vec![TextReplacement {
            target: parsed.target_content,
            replacement: parsed.replacement_content,
            line_range: optional_line_range(parsed.start_line, parsed.end_line),
            allow_multiple: parsed.allow_multiple,
        }]
    } else {
        eprintln!(
            "[omnicoach hook] ignoring malformed {tool_name} args: missing target_content/replacement_content"
        );
        return ToolCallPayload::Other;
    };

    ToolCallPayload::FileEdit {
        path: parsed.target_file,
        edit: FileEdit::Replace(replacements),
    }
}

fn deserialize_tool_args<T: serde::de::DeserializeOwned>(
    args: &serde_json::Value,
    tool_name: &str,
) -> Option<T> {
    match T::deserialize(args) {
        Ok(parsed) => Some(parsed),
        Err(error) => {
            eprintln!("[omnicoach hook] ignoring malformed {tool_name} args: {error}");
            None
        }
    }
}

const fn optional_line_range(
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> Option<RangeInclusive<usize>> {
    match (start_line, end_line) {
        (Some(start), Some(end)) if start > 0 && end >= start => Some(start..=end),
        (Some(line), None) | (None, Some(line)) if line > 0 => Some(line..=line),
        _ => None,
    }
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
struct WriteToFileArgs {
    target_file: PathBuf,
    code_content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct EditFileArgs {
    path: PathBuf,
    #[serde(default)]
    target: String,
    #[serde(default)]
    replacement: String,
    #[serde(default)]
    edits: Vec<EditHunkArgs>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct EditHunkArgs {
    target: String,
    replacement: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ReplaceContentArgs {
    target_file: PathBuf,
    #[serde(default)]
    target_content: String,
    #[serde(default)]
    replacement_content: String,
    #[serde(default)]
    allow_multiple: bool,
    #[serde(default)]
    start_line: Option<usize>,
    #[serde(default)]
    end_line: Option<usize>,
    #[serde(default)]
    replacement_chunks: Vec<ReplacementChunkArgs>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ReplacementChunkArgs {
    target_content: String,
    replacement_content: String,
    #[serde(default)]
    allow_multiple: bool,
    #[serde(default)]
    start_line: Option<usize>,
    #[serde(default)]
    end_line: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WirePreToolResult<'a> {
    decision: &'static str,
    reason: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WirePreInvocationResult {
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
                    payload: ToolCallPayload::FileEdit {
                        path: PathBuf::from("src/main.rs"),
                        edit: FileEdit::Append("fn main() {}\n".to_owned()),
                    },
                },
                warning_queue: None,
            }
        );
    }

    #[test]
    fn parse_pre_tool_use_write_to_file() {
        let json_text = indoc::indoc! {r#"
            {
                "toolCall": {
                    "name": "write_to_file",
                    "args": {
                        "TargetFile": "src/main.rs",
                        "CodeContent": "fn main() {}\n"
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
                    payload: ToolCallPayload::FileEdit {
                        path: PathBuf::from("src/main.rs"),
                        edit: FileEdit::Overwrite("fn main() {}\n".to_owned()),
                    },
                },
                warning_queue: None,
            }
        );
    }

    #[test]
    fn parse_pre_tool_use_edit_file_single() {
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
                    payload: ToolCallPayload::FileEdit {
                        path: PathBuf::from("src/main.rs"),
                        edit: FileEdit::Replace(vec![TextReplacement {
                            target: "old".to_owned(),
                            replacement: "new".to_owned(),
                            line_range: None,
                            allow_multiple: true,
                        }]),
                    },
                },
                warning_queue: None,
            }
        );
    }

    #[test]
    fn parse_pre_tool_use_edit_file_edits_array() {
        let json_text = indoc::indoc! {r#"
            {
                "toolCall": {
                    "name": "edit_file",
                    "args": {
                        "Path": "src/main.rs",
                        "Edits": [
                            { "Target": "old1", "Replacement": "new1" },
                            { "Target": "old2", "Replacement": "new2" }
                        ]
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
                    payload: ToolCallPayload::FileEdit {
                        path: PathBuf::from("src/main.rs"),
                        edit: FileEdit::Replace(vec![
                            TextReplacement {
                                target: "old1".to_owned(),
                                replacement: "new1".to_owned(),
                                line_range: None,
                                allow_multiple: true,
                            },
                            TextReplacement {
                                target: "old2".to_owned(),
                                replacement: "new2".to_owned(),
                                line_range: None,
                                allow_multiple: true,
                            },
                        ]),
                    },
                },
                warning_queue: None,
            }
        );
    }

    #[test]
    fn parse_pre_tool_use_replace_file_content() {
        let json_text = indoc::indoc! {r#"
            {
                "toolCall": {
                    "name": "replace_file_content",
                    "args": {
                        "TargetFile": "src/main.rs",
                        "TargetContent": "old",
                        "ReplacementContent": "new",
                        "StartLine": 3,
                        "EndLine": 7,
                        "AllowMultiple": true
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
                    payload: ToolCallPayload::FileEdit {
                        path: PathBuf::from("src/main.rs"),
                        edit: FileEdit::Replace(vec![TextReplacement {
                            target: "old".to_owned(),
                            replacement: "new".to_owned(),
                            line_range: Some(3..=7),
                            allow_multiple: true,
                        }]),
                    },
                },
                warning_queue: None,
            }
        );
    }

    #[test]
    fn parse_pre_tool_use_multi_replace_file_content() {
        let json_text = indoc::indoc! {r#"
            {
                "toolCall": {
                    "name": "multi_replace_file_content",
                    "args": {
                        "TargetFile": "src/main.rs",
                        "ReplacementChunks": [
                            {
                                "TargetContent": "chunk1",
                                "ReplacementContent": "repl1",
                                "StartLine": 10,
                                "EndLine": 12
                            }
                        ]
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
                    payload: ToolCallPayload::FileEdit {
                        path: PathBuf::from("src/main.rs"),
                        edit: FileEdit::Replace(vec![TextReplacement {
                            target: "chunk1".to_owned(),
                            replacement: "repl1".to_owned(),
                            line_range: Some(10..=12),
                            allow_multiple: false,
                        }]),
                    },
                },
                warning_queue: None,
            }
        );
    }

    #[test]
    fn parse_pre_tool_use_malformed_args_falls_back_to_other() {
        let json_text = indoc::indoc! {r#"
            {
                "toolCall": {
                    "name": "run_command",
                    "args": { "WrongField": 123 }
                }
            }
        "#};
        assert_eq!(
            parse_pre_tool_use(json_text)
                .expect("parse succeeds")
                .tool_call
                .payload,
            ToolCallPayload::Other
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
        let input = parse_pre_tool_use(json_text).expect("parse succeeds");
        assert_eq!(input.tool_call.workspace_paths, Vec::<PathBuf>::new());
        assert_eq!(input.tool_call.agent_owned_dir, None);
        assert_eq!(input.warning_queue, None);
    }

    #[test]
    fn serialize_pre_tool_decision_no_objection() {
        let serialized = serialize_pre_tool_decision(&PreToolDecision::NoObjection)
            .expect("serialization succeeds");
        assert_eq!(serialized, "");
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
            [omnicoach] Proposed file modification(s) introduced code lint warnings on modified lines. Please review and fix them if appropriate:

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
        assert_eq!(serialized, "");
    }
}
