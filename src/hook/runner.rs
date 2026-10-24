//! Agent-neutral evaluation of an intercepted tool call: command lint for shell commands and
//! diff-scoped code lint for file edits.

use std::path::{Path, PathBuf};

use super::event::{FileEdit, PreToolDecision, PreToolUseEvent, ToolCallPayload};
use super::virtual_edit::apply_virtual_edit;
use crate::code_lint::runner::lint_file;
use crate::command_lint::runner::run_command_lint_with_client;
use crate::command_lint::vcs::{JjCliClient, JjClient};
use crate::config::Config;
use crate::diagnostic::Diagnostic;
use crate::rule_selection::{ConfigOverrides, discover_project_root, load_config_for_dir};

type JjClientFactory<'a> = &'a dyn Fn(&Path) -> Box<dyn JjClient>;

/// Outcome of evaluating a tool call before it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ToolCallOutcome {
    /// Whether the tool call may run.
    pub(super) decision: PreToolDecision,
    /// Code lint warnings on the lines a file edit adds or modifies, one formatted warning per
    /// diagnostic. How and when they reach the model depends on what the agent's hooks support,
    /// so delivering them is the adapter's job.
    pub(super) warnings: Vec<String>,
}

/// Evaluates a tool call against the repository on disk.
pub(super) fn evaluate_pre_tool_use(event: &PreToolUseEvent) -> ToolCallOutcome {
    evaluate_pre_tool_use_with(event, &|directory| Box::new(JjCliClient::in_dir(directory)))
}

fn evaluate_pre_tool_use_with(
    event: &PreToolUseEvent,
    jj_client_for: JjClientFactory<'_>,
) -> ToolCallOutcome {
    match &event.payload {
        ToolCallPayload::RunCommand { command_line, cwd } => ToolCallOutcome {
            decision: check_command(
                command_line,
                cwd.as_deref(),
                &event.workspace_paths,
                jj_client_for,
            ),
            warnings: Vec::new(),
        },
        ToolCallPayload::FileEdit { path, edit } => ToolCallOutcome {
            decision: PreToolDecision::NoObjection,
            warnings: lint_proposed_edit(event, path, edit),
        },
        ToolCallPayload::Other => ToolCallOutcome {
            decision: PreToolDecision::NoObjection,
            warnings: Vec::new(),
        },
    }
}

fn check_command(
    command_line: &str,
    cwd: Option<&Path>,
    workspace_paths: &[PathBuf],
    jj_client_for: JjClientFactory<'_>,
) -> PreToolDecision {
    let working_directory = resolve_command_working_directory(cwd, workspace_paths);
    let config = load_hook_config(&working_directory);
    let client = jj_client_for(&working_directory);

    let diagnostics = run_command_lint_with_client(command_line, &config, client.as_ref());
    if diagnostics.is_empty() {
        PreToolDecision::NoObjection
    } else {
        let reasons: Vec<String> = diagnostics
            .iter()
            .map(|diagnostic| {
                format!(
                    "[{}] {}\nsuggestion: {}",
                    diagnostic.rule_name, diagnostic.message.summary, diagnostic.message.suggestion
                )
            })
            .collect();
        PreToolDecision::Deny {
            reason: reasons.join("\n\n"),
        }
    }
}

fn resolve_command_working_directory(cwd: Option<&Path>, workspace_paths: &[PathBuf]) -> PathBuf {
    cwd.map_or_else(
        || {
            workspace_paths
                .first()
                .cloned()
                .unwrap_or_else(|| PathBuf::from("."))
        },
        |explicit_cwd| {
            if explicit_cwd.is_relative()
                && let Some(workspace_root) = workspace_paths.first()
            {
                workspace_root.join(explicit_cwd)
            } else {
                explicit_cwd.to_path_buf()
            }
        },
    )
}

fn lint_proposed_edit(
    event: &PreToolUseEvent,
    raw_target_path: &Path,
    edit: &FileEdit,
) -> Vec<String> {
    let target_file_path = resolve_target_file_path(raw_target_path, &event.workspace_paths);

    let is_agent_owned_file = event
        .agent_owned_dir
        .as_ref()
        .is_some_and(|agent_owned_dir| {
            !agent_owned_dir.as_os_str().is_empty() && target_file_path.starts_with(agent_owned_dir)
        });
    if is_agent_owned_file {
        return Vec::new();
    }

    let original_content = std::fs::read_to_string(&target_file_path).ok();
    let Ok(virtual_file) = apply_virtual_edit(original_content.as_deref(), edit) else {
        return Vec::new();
    };

    let project_root = resolve_lint_root(&target_file_path, &event.workspace_paths);
    let relative_path = target_file_path
        .strip_prefix(&project_root)
        .unwrap_or(&target_file_path);

    let parent_directory = target_file_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let config = load_hook_config(parent_directory);

    lint_file(relative_path, &virtual_file.content, &config)
        .iter()
        .filter(|diagnostic| {
            virtual_file
                .modified_lines
                .contains(&diagnostic.location.line)
        })
        .map(|diagnostic| format_code_warning(&target_file_path, diagnostic))
        .collect()
}

fn resolve_target_file_path(raw_path: &Path, workspace_paths: &[PathBuf]) -> PathBuf {
    if raw_path.is_relative()
        && let Some(workspace_root) = workspace_paths.first()
    {
        workspace_root.join(raw_path)
    } else {
        raw_path.to_path_buf()
    }
}

fn resolve_lint_root(target_file_path: &Path, workspace_paths: &[PathBuf]) -> PathBuf {
    if let Some(matching_workspace) = workspace_paths
        .iter()
        .find(|workspace| target_file_path.starts_with(workspace))
    {
        return matching_workspace.clone();
    }

    let parent_directory = target_file_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    discover_project_root(parent_directory).unwrap_or_else(|| parent_directory.to_path_buf())
}

fn load_hook_config(directory: &Path) -> Config {
    load_config_for_dir(directory, &ConfigOverrides::default()).unwrap_or_else(|error| {
        eprintln!("[omnicoach hook] ignoring invalid config: {error}");
        Config::default()
    })
}

fn format_code_warning(target_path: &Path, diagnostic: &Diagnostic) -> String {
    format!(
        "{}:{}:{}: [{}] {}\nsuggestion: {}",
        target_path.display(),
        diagnostic.location.line,
        diagnostic.location.column,
        diagnostic.rule_name,
        diagnostic.message.summary,
        diagnostic.message.suggestion
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_lint::vcs::CommitInfo;

    struct FakeVcsClient {
        description: String,
        conflict: bool,
    }

    impl JjClient for FakeVcsClient {
        fn get_commit_info(&self, _revision: &str) -> Result<CommitInfo, String> {
            Ok(CommitInfo {
                description: self.description.clone(),
                conflict: self.conflict,
            })
        }
    }

    #[test]
    fn evaluate_pre_tool_use_allows_safe_command() {
        let event = PreToolUseEvent {
            workspace_paths: Vec::new(),
            agent_owned_dir: None,
            payload: ToolCallPayload::RunCommand {
                command_line: "jj status".to_owned(),
                cwd: None,
            },
        };
        let outcome = evaluate_pre_tool_use_with(&event, &|_| {
            Box::new(FakeVcsClient {
                description: String::new(),
                conflict: false,
            })
        });
        assert_eq!(outcome.decision, PreToolDecision::NoObjection);
    }

    #[test]
    fn evaluate_pre_tool_use_denies_jj_edit_on_described_commit() {
        let event = PreToolUseEvent {
            workspace_paths: Vec::new(),
            agent_owned_dir: None,
            payload: ToolCallPayload::RunCommand {
                command_line: "jj edit main".to_owned(),
                cwd: None,
            },
        };
        let outcome = evaluate_pre_tool_use_with(&event, &|_| {
            Box::new(FakeVcsClient {
                description: "feat: described commit".to_owned(),
                conflict: false,
            })
        });
        assert!(matches!(outcome.decision, PreToolDecision::Deny { .. }));
    }

    #[test]
    fn evaluate_pre_tool_use_allows_file_edit_even_when_working_copy_described() {
        let event = PreToolUseEvent {
            workspace_paths: Vec::new(),
            agent_owned_dir: None,
            payload: ToolCallPayload::FileEdit {
                path: PathBuf::from("src/test.rs"),
                edit: FileEdit::Overwrite("fn test() {}\n".to_owned()),
            },
        };
        let outcome = evaluate_pre_tool_use_with(&event, &|_| {
            Box::new(FakeVcsClient {
                description: "feat: described working copy".to_owned(),
                conflict: false,
            })
        });
        assert_eq!(outcome.decision, PreToolDecision::NoObjection);
    }
}
