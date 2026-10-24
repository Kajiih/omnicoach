//! Parsing and serialization for Antigravity agent lifecycle hook JSON payloads.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Lifecycle hook event kind passed on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum HookEventKind {
    /// Fired before an agent executes a tool call.
    #[value(name = "pre-tool-use")]
    PreToolUse,
    /// Fired before the model is invoked for a new reasoning step.
    #[value(name = "pre-invocation")]
    PreInvocation,
}

/// Errors encountered while parsing or serializing hook protocol payloads.
#[derive(Debug, thiserror::Error)]
pub enum HookProtocolError {
    /// JSON deserialization or serialization failure.
    #[error("JSON protocol error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Strongly-typed representation of a parsed agent lifecycle hook event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum HookEvent {
    PreToolUse(PreToolUseEvent),
    PreInvocation(PreInvocationEvent),
}

/// Event payload for `PreToolUse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PreToolUseEvent {
    pub(super) workspace_paths: Vec<PathBuf>,
    pub(super) artifact_directory_path: Option<PathBuf>,
    pub(super) payload: ToolCallPayload,
}

/// Event payload for `PreInvocation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PreInvocationEvent {
    pub(super) artifact_directory_path: Option<PathBuf>,
}

/// Parsed tool call payload variants intercepted by the pre-tool-use hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ToolCallPayload {
    RunCommand {
        command_line: String,
        cwd: Option<PathBuf>,
    },
    WriteFile {
        path: PathBuf,
        content: String,
        append: bool,
    },
    EditFile {
        path: PathBuf,
        target: String,
        replacement: String,
    },
    Other {
        tool_name: String,
    },
}

impl ToolCallPayload {
    #[must_use]
    pub(super) fn target_file_path(&self) -> Option<&Path> {
        match self {
            Self::WriteFile { path, .. } | Self::EditFile { path, .. } => Some(path.as_path()),
            Self::RunCommand { .. } | Self::Other { .. } => None,
        }
    }
}

/// Outgoing hook evaluation response returned to the agent harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum HookResponse {
    PreToolUse(PreToolDecision),
    PreInvocation(PreInvocationOutput),
}

/// Permission decision for a tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PreToolDecision {
    Allow,
    Deny { reason: String },
}

/// Step to inject into agent context before invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InjectStep {
    pub(super) ephemeral_message: String,
}

/// Output data returned for `PreInvocation` events.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct PreInvocationOutput {
    pub(super) inject_steps: Vec<InjectStep>,
}

impl PreInvocationOutput {
    #[must_use]
    pub(super) const fn empty() -> Self {
        Self {
            inject_steps: Vec::new(),
        }
    }

    #[must_use]
    pub(super) fn with_ephemeral_message(ephemeral_message: impl Into<String>) -> Self {
        Self {
            inject_steps: vec![InjectStep {
                ephemeral_message: ephemeral_message.into(),
            }],
        }
    }
}

pub(super) fn parse_hook_input(
    kind: HookEventKind,
    json_text: &str,
) -> Result<HookEvent, HookProtocolError> {
    match kind {
        HookEventKind::PreToolUse => Ok(HookEvent::PreToolUse(parse_pre_tool_use(json_text)?)),
        HookEventKind::PreInvocation => {
            Ok(HookEvent::PreInvocation(parse_pre_invocation(json_text)?))
        }
    }
}

pub(super) fn serialize_hook_response(
    response: &HookResponse,
) -> Result<String, HookProtocolError> {
    match response {
        HookResponse::PreToolUse(decision) => {
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
        HookResponse::PreInvocation(output) => {
            let wire = WirePreInvocationResult {
                inject_steps: output.inject_steps.clone(),
            };
            Ok(serde_json::to_string(&wire)?)
        }
    }
}

fn parse_pre_tool_use(json_text: &str) -> Result<PreToolUseEvent, HookProtocolError> {
    let raw: RawPreToolUse = serde_json::from_str(json_text)?;
    let artifact_directory_path = non_empty_trimmed(raw.artifact_directory_path).map(PathBuf::from);
    let workspace_paths: Vec<PathBuf> = raw
        .workspace_paths
        .into_iter()
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .collect();
    let payload = parse_tool_payload(raw.tool_call);

    Ok(PreToolUseEvent {
        workspace_paths,
        artifact_directory_path,
        payload,
    })
}

fn parse_pre_invocation(json_text: &str) -> Result<PreInvocationEvent, HookProtocolError> {
    let raw: RawPreInvocation = serde_json::from_str(json_text)?;
    let artifact_directory_path = non_empty_trimmed(raw.artifact_directory_path).map(PathBuf::from);

    Ok(PreInvocationEvent {
        artifact_directory_path,
    })
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
    inject_steps: Vec<InjectStep>,
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
        let event = parse_hook_input(HookEventKind::PreToolUse, json_text).expect("parse succeeds");
        assert_eq!(
            event,
            HookEvent::PreToolUse(PreToolUseEvent {
                workspace_paths: vec![PathBuf::from("/home/user/repo")],
                artifact_directory_path: Some(PathBuf::from("/tmp/brain")),
                payload: ToolCallPayload::RunCommand {
                    command_line: "jj status".to_owned(),
                    cwd: Some(PathBuf::from("/home/user/repo")),
                },
            })
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
        let event = parse_hook_input(HookEventKind::PreToolUse, json_text).expect("parse succeeds");
        assert_eq!(
            event,
            HookEvent::PreToolUse(PreToolUseEvent {
                workspace_paths: Vec::new(),
                artifact_directory_path: None,
                payload: ToolCallPayload::WriteFile {
                    path: PathBuf::from("src/main.rs"),
                    content: "fn main() {}\n".to_owned(),
                    append: true,
                },
            })
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
        let event = parse_hook_input(HookEventKind::PreToolUse, json_text).expect("parse succeeds");
        assert_eq!(
            event,
            HookEvent::PreToolUse(PreToolUseEvent {
                workspace_paths: Vec::new(),
                artifact_directory_path: None,
                payload: ToolCallPayload::EditFile {
                    path: PathBuf::from("src/main.rs"),
                    target: "old".to_owned(),
                    replacement: "new".to_owned(),
                },
            })
        );
    }

    #[test]
    fn parse_pre_invocation_event() {
        let json_text = r#"{"artifactDirectoryPath":"/tmp/brain"}"#;
        let event =
            parse_hook_input(HookEventKind::PreInvocation, json_text).expect("parse succeeds");
        assert_eq!(
            event,
            HookEvent::PreInvocation(PreInvocationEvent {
                artifact_directory_path: Some(PathBuf::from("/tmp/brain")),
            })
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
        let event =
            parse_hook_input(HookEventKind::PreInvocation, json_text).expect("parse succeeds");
        assert_eq!(
            event,
            HookEvent::PreInvocation(PreInvocationEvent {
                artifact_directory_path: None,
            })
        );
    }

    #[test]
    fn serialize_pre_tool_decision_allow() {
        let response = HookResponse::PreToolUse(PreToolDecision::Allow);
        let serialized = serialize_hook_response(&response).expect("serialization succeeds");
        assert_eq!(serialized, r#"{"decision":"allow"}"#);
    }

    #[test]
    fn serialize_pre_tool_decision_deny() {
        let response = HookResponse::PreToolUse(PreToolDecision::Deny {
            reason: "blocked command".to_owned(),
        });
        let serialized = serialize_hook_response(&response).expect("serialization succeeds");
        assert_eq!(
            serialized,
            r#"{"decision":"deny","reason":"blocked command"}"#
        );
    }

    #[test]
    fn serialize_pre_invocation_output_with_steps() {
        let output = PreInvocationOutput::with_ephemeral_message("warning text");
        let response = HookResponse::PreInvocation(output);
        let serialized = serialize_hook_response(&response).expect("serialization succeeds");
        assert_eq!(
            serialized,
            r#"{"injectSteps":[{"ephemeralMessage":"warning text"}]}"#
        );
    }

    #[test]
    fn serialize_pre_invocation_output_empty() {
        let output = PreInvocationOutput::empty();
        let response = HookResponse::PreInvocation(output);
        let serialized = serialize_hook_response(&response).expect("serialization succeeds");
        assert_eq!(serialized, "{}");
    }
}
