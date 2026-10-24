//! Agent-neutral model of a tool call intercepted before it runs.
//!
//! Each supported agent has an adapter module that translates its hook payloads into these types
//! and the decision back, so `runner` never depends on an agent's wire format or vocabulary.

use std::ops::RangeInclusive;
use std::path::PathBuf;

/// The agent is about to run a tool call, which the hook may deny.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PreToolUseEvent {
    /// Workspace roots open in the agent; the first one resolves relative paths.
    pub(super) workspace_paths: Vec<PathBuf>,
    /// Directory holding the agent's own session files and notes.
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
    FileEdit {
        path: PathBuf,
        edit: FileEdit,
    },
    Other,
}

/// Agent-neutral file modification operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FileEdit {
    /// Creates or overwrites the file with complete content.
    Overwrite(String),
    /// Appends content to the end of the file (or creates it if absent).
    Append(String),
    /// Applies one or more exact substring replacements sequentially.
    Replace(Vec<TextReplacement>),
}

/// A single substring replacement within a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TextReplacement {
    pub(super) target: String,
    pub(super) replacement: String,
    /// Optional 1-based inclusive `start_line..=end_line` range where `target` is searched
    /// first before falling back to a unique whole-file match.
    pub(super) line_range: Option<RangeInclusive<usize>>,
    /// Whether multiple occurrences of `target` may be replaced. When `false`, a target that
    /// matches more than once in the active search scope is rejected.
    pub(super) allow_multiple: bool,
}

/// Permission decision for a tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PreToolDecision {
    /// The hook found no policy violation; adapters should delegate to the agent's normal
    /// permission flow rather than bypassing user approval prompts.
    NoObjection,
    /// The tool call violates a command rule and must be blocked with coaching feedback.
    Deny { reason: String },
}
