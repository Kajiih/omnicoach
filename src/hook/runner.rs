//! Agent-neutral evaluation of an intercepted tool call: command lint for shell commands and
//! diff-scoped code lint for file edits.

use std::path::{Component, Path, PathBuf};

use super::event::{FileEdit, PreToolDecision, PreToolUseEvent, ToolCallPayload};
use super::virtual_edit::apply_virtual_edit;
use crate::code_lint::runner::lint_file;
use crate::command_lint::runner::run_command_lint_with_client;
use crate::command_lint::vcs::{JjCliClient, JjClient};
use crate::config::Config;
use crate::diagnostic::{Diagnostic, Language};
use crate::rule_selection::{ConfigOverrides, discover_project_root, load_config_from_root};

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
    let working_directory =
        resolve_in_workspace(cwd.unwrap_or_else(|| Path::new("")), workspace_paths);
    let project_root = resolve_project_root(&working_directory, workspace_paths);
    let Some(config) = load_hook_config(&project_root) else {
        return PreToolDecision::NoObjection;
    };
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

fn lint_proposed_edit(
    event: &PreToolUseEvent,
    raw_target_path: &Path,
    edit: &FileEdit,
) -> Vec<String> {
    let target_file_path = resolve_in_workspace(raw_target_path, &event.workspace_paths);

    let is_agent_owned_file = event
        .agent_owned_dir
        .as_ref()
        .is_some_and(|agent_owned_dir| {
            !agent_owned_dir.as_os_str().is_empty()
                && target_file_path.starts_with(normalize_path(agent_owned_dir))
        });
    if is_agent_owned_file {
        return Vec::new();
    }

    if Language::from_path(&target_file_path).is_none() {
        return Vec::new();
    }

    let original_content = match std::fs::read_to_string(&target_file_path) {
        Ok(content) => Some(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            eprintln!(
                "[omnicoach hook] skipping lint of {}: {error}",
                target_file_path.display()
            );
            return Vec::new();
        }
    };
    let Ok(virtual_file) = apply_virtual_edit(original_content.as_deref(), edit) else {
        return Vec::new();
    };

    let parent_directory = target_file_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let project_root = resolve_project_root(parent_directory, &event.workspace_paths);
    let Some(config) = load_hook_config(&project_root) else {
        return Vec::new();
    };
    let relative_path = target_file_path
        .strip_prefix(&project_root)
        .unwrap_or(&target_file_path);

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

fn resolve_in_workspace(raw_path: &Path, workspace_paths: &[PathBuf]) -> PathBuf {
    let joined = if raw_path.as_os_str().is_empty() {
        workspace_paths
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from("."))
    } else if raw_path.is_relative()
        && let Some(workspace_root) = workspace_paths.first()
    {
        workspace_root.join(raw_path)
    } else {
        raw_path.to_path_buf()
    };
    normalize_path(&joined)
}

fn resolve_project_root(directory: &Path, workspace_paths: &[PathBuf]) -> PathBuf {
    discover_project_root(directory)
        .or_else(|| {
            workspace_paths
                .iter()
                .map(|workspace| normalize_path(workspace))
                .find(|workspace| directory.starts_with(workspace))
        })
        .unwrap_or_else(|| directory.to_path_buf())
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut components: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match components.last() {
                Some(Component::Normal(_)) => {
                    components.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                Some(Component::ParentDir | Component::CurDir) | None => {
                    components.push(component);
                }
            },
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                components.push(component);
            }
        }
    }
    if components.is_empty() {
        PathBuf::from(".")
    } else {
        components.iter().collect()
    }
}

fn load_hook_config(project_root: &Path) -> Option<Config> {
    match load_config_from_root(project_root, &ConfigOverrides::default()) {
        Ok(config) => Some(config),
        Err(error) => {
            eprintln!("[omnicoach hook] ignoring invalid config (failing open): {error}");
            None
        }
    }
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
        let temp_dir = tempfile::tempdir().unwrap();
        let event = PreToolUseEvent {
            workspace_paths: vec![temp_dir.path().to_path_buf()],
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
        let temp_dir = tempfile::tempdir().unwrap();
        let event = PreToolUseEvent {
            workspace_paths: vec![temp_dir.path().to_path_buf()],
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
        let temp_dir = tempfile::tempdir().unwrap();
        let event = PreToolUseEvent {
            workspace_paths: vec![temp_dir.path().to_path_buf()],
            agent_owned_dir: None,
            payload: ToolCallPayload::FileEdit {
                path: PathBuf::from("src/test.rs"),
                edit: FileEdit::Overwrite("pub fn test_fn() {}\n".to_owned()),
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

    #[test]
    fn evaluate_pre_tool_use_fails_open_when_config_is_invalid() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            temp_dir.path().join(".omnicoach.toml"),
            "select = [invalid syntax",
        )
        .unwrap();
        let event = PreToolUseEvent {
            workspace_paths: vec![temp_dir.path().to_path_buf()],
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
        assert_eq!(outcome.decision, PreToolDecision::NoObjection);
    }

    #[test]
    fn evaluate_pre_tool_use_blocks_traversal_into_agent_owned_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace = temp_dir.path().join("workspace");
        let artifacts = temp_dir.path().join("artifacts");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&artifacts).unwrap();

        let bad_code = indoc::indoc! {"
            pub fn compute() -> i32 {
                let err = 1;
                err
            }
        "};
        let target_in_artifacts = workspace.join("../artifacts/notes.rs");

        let event = PreToolUseEvent {
            workspace_paths: vec![workspace],
            agent_owned_dir: Some(artifacts),
            payload: ToolCallPayload::FileEdit {
                path: target_in_artifacts,
                edit: FileEdit::Overwrite(bad_code.to_owned()),
            },
        };
        let outcome = evaluate_pre_tool_use_with(&event, &|_| {
            Box::new(FakeVcsClient {
                description: String::new(),
                conflict: false,
            })
        });
        assert_eq!(outcome.warnings, Vec::<String>::new());
    }
}
