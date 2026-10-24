//! Agent-neutral model of a tool call intercepted before it runs.
//!
//! Each supported agent has an adapter module that translates its hook payloads into these types
//! and the decision back, so `runner` never depends on an agent's wire format or vocabulary.

use std::path::{Path, PathBuf};

/// The agent is about to run a tool call, which the hook may deny.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PreToolUseEvent {
    /// Workspace roots open in the agent; the first one resolves relative paths.
    pub(super) workspace_paths: Vec<PathBuf>,
    /// Directory holding the agent's own notes for the session. Files inside it are not project
    /// code, so edits to them are not linted.
    pub(super) agent_owned_dir: Option<PathBuf>,
    pub(super) payload: ToolCallPayload,
}

/// Tool call intercepted before it runs.
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

/// Permission decision for a tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PreToolDecision {
    Allow,
    Deny { reason: String },
}
