//! Orchestration runner for agent lifecycle hook execution.

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use super::protocol::{
    HookEvent, HookEventKind, HookProtocolError, HookResponse, PreInvocationEvent,
    PreInvocationOutput, PreToolDecision, PreToolUseEvent, ToolCallPayload, parse_hook_input,
    serialize_hook_response,
};
use super::virtual_edit::apply_virtual_edit;
use crate::code_lint::runner::lint_file;
use crate::command_lint::runner::run_command_lint_with_client;
use crate::command_lint::vcs::{JjCliClient, JjClient};
use crate::config::Config;
use crate::diagnostic::Diagnostic;
use crate::rule_selection::{ConfigOverrides, discover_project_root, load_config_for_dir};

const WARNINGS_QUEUE_SUBDIR: &str = "scratch";
const WARNINGS_QUEUE_FILENAME: &str = "omnicoach_lint_warnings.jsonl";

type JjClientFactory<'a> = &'a dyn Fn(&Path) -> Box<dyn JjClient>;

/// Executes a hook event received as JSON on stdin, returning the JSON response string.
///
/// # Errors
///
/// Returns [`HookProtocolError`] if parsing input JSON or serializing response JSON fails.
pub fn execute_hook_json(
    kind: HookEventKind,
    json_input: &str,
) -> Result<String, HookProtocolError> {
    let event = parse_hook_input(kind, json_input)?;
    let response = run_hook_event(&event, &|dir| Box::new(JjCliClient::in_dir(dir)));
    serialize_hook_response(&response)
}

fn run_hook_event(event: &HookEvent, jj_client_for: JjClientFactory<'_>) -> HookResponse {
    match event {
        HookEvent::PreToolUse(tool_event) => handle_pre_tool_use(tool_event, jj_client_for),
        HookEvent::PreInvocation(invocation_event) => handle_pre_invocation(invocation_event),
    }
}

fn handle_pre_tool_use(
    event: &PreToolUseEvent,
    jj_client_for: JjClientFactory<'_>,
) -> HookResponse {
    match &event.payload {
        ToolCallPayload::RunCommand { command_line, cwd } => handle_run_command_tool(
            command_line,
            cwd.as_deref(),
            &event.workspace_paths,
            jj_client_for,
        ),
        ToolCallPayload::WriteFile { .. } | ToolCallPayload::EditFile { .. } => {
            handle_file_editing_tool(event)
        }
        ToolCallPayload::Other { .. } => HookResponse::PreToolUse(PreToolDecision::Allow),
    }
}

fn handle_run_command_tool(
    command_line: &str,
    cwd: Option<&Path>,
    workspace_paths: &[PathBuf],
    jj_client_for: JjClientFactory<'_>,
) -> HookResponse {
    let working_directory = resolve_command_working_directory(cwd, workspace_paths);
    let config = load_hook_config(&working_directory);
    let client = jj_client_for(&working_directory);

    let diagnostics = run_command_lint_with_client(command_line, &config, client.as_ref());
    if diagnostics.is_empty() {
        HookResponse::PreToolUse(PreToolDecision::Allow)
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
        HookResponse::PreToolUse(PreToolDecision::Deny {
            reason: reasons.join("\n\n"),
        })
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

fn handle_file_editing_tool(event: &PreToolUseEvent) -> HookResponse {
    let Some(raw_target_path) = event.payload.target_file_path() else {
        return HookResponse::PreToolUse(PreToolDecision::Allow);
    };

    let target_file_path = resolve_target_file_path(raw_target_path, &event.workspace_paths);

    let is_artifact_file = event
        .artifact_directory_path
        .as_ref()
        .is_some_and(|artifact_dir| {
            !artifact_dir.as_os_str().is_empty() && target_file_path.starts_with(artifact_dir)
        });
    if is_artifact_file {
        return HookResponse::PreToolUse(PreToolDecision::Allow);
    }

    check_virtual_file_code_lint(event, &target_file_path);

    HookResponse::PreToolUse(PreToolDecision::Allow)
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

fn check_virtual_file_code_lint(event: &PreToolUseEvent, target_file_path: &Path) {
    let original_content = std::fs::read_to_string(target_file_path).ok();
    let Ok(virtual_file) = apply_virtual_edit(original_content.as_deref(), &event.payload) else {
        return;
    };

    let project_root = resolve_lint_root(target_file_path, &event.workspace_paths);
    let relative_path = target_file_path
        .strip_prefix(&project_root)
        .unwrap_or(target_file_path);

    let parent_directory = target_file_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let config = load_hook_config(parent_directory);

    let diagnostics = lint_file(relative_path, &virtual_file.content, &config);

    let relevant_diagnostics: Vec<&Diagnostic> = diagnostics
        .iter()
        .filter(|diagnostic| {
            virtual_file
                .modified_lines
                .contains(&diagnostic.location.line)
        })
        .collect();

    if relevant_diagnostics.is_empty() {
        return;
    }

    let Some(artifact_dir) = &event.artifact_directory_path else {
        return;
    };

    let formatted_warnings: Vec<String> = relevant_diagnostics
        .iter()
        .map(|diagnostic| format_code_warning(target_file_path, diagnostic))
        .collect();

    record_lint_warnings(artifact_dir, &formatted_warnings);
}

fn resolve_lint_root(target_file_path: &Path, workspace_paths: &[PathBuf]) -> PathBuf {
    if let Some(matching_workspace) = workspace_paths
        .iter()
        .find(|workspace| target_file_path.starts_with(workspace))
    {
        return matching_workspace.clone();
    }

    let parent_dir = target_file_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    discover_project_root(parent_dir).unwrap_or_else(|| parent_dir.to_path_buf())
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

fn record_lint_warnings(artifact_dir: &Path, warnings: &[String]) {
    let queue_path = warning_queue_path(artifact_dir);
    if let Some(parent) = queue_path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }

    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&queue_path)
    else {
        return;
    };
    let _ = file.lock();

    for warning in warnings {
        let trimmed = warning.trim();
        if !trimmed.is_empty()
            && let Ok(json_line) = serde_json::to_string(trimmed)
        {
            let _ = writeln!(file, "{json_line}");
        }
    }
}

fn handle_pre_invocation(event: &PreInvocationEvent) -> HookResponse {
    let Some(artifact_dir) = &event.artifact_directory_path else {
        return HookResponse::PreInvocation(PreInvocationOutput::empty());
    };

    let queue_path = warning_queue_path(artifact_dir);
    let Some(warnings_content) = drain_unique_warnings(&queue_path) else {
        return HookResponse::PreInvocation(PreInvocationOutput::empty());
    };

    let warning_message = indoc::formatdoc! {"
        [omnicoach] The previous file modification introduced code lint warnings on modified lines. Please review and fix them if appropriate:

        {warnings_content}"};

    HookResponse::PreInvocation(PreInvocationOutput::with_ephemeral_message(warning_message))
}

fn drain_unique_warnings(queue_path: &Path) -> Option<String> {
    if !queue_path.is_file() {
        return None;
    }

    let draining_path = queue_path.with_extension(format!("draining-{}", std::process::id()));
    if std::fs::rename(queue_path, &draining_path).is_err() {
        return None;
    }

    let file = std::fs::File::open(&draining_path).ok()?;
    let _ = file.lock_shared();
    let content = std::io::read_to_string(&file).ok()?;
    let _ = std::fs::remove_file(&draining_path);

    let mut seen = HashSet::new();
    let mut unique_warnings = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let warning_text: String =
            serde_json::from_str(trimmed).unwrap_or_else(|_| trimmed.to_owned());
        if seen.insert(warning_text.clone()) {
            unique_warnings.push(warning_text);
        }
    }

    if unique_warnings.is_empty() {
        None
    } else {
        Some(unique_warnings.join("\n\n"))
    }
}

fn warning_queue_path(artifact_dir: &Path) -> PathBuf {
    artifact_dir
        .join(WARNINGS_QUEUE_SUBDIR)
        .join(WARNINGS_QUEUE_FILENAME)
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
    fn run_hook_event_pre_tool_allows_safe_command() {
        let event = HookEvent::PreToolUse(PreToolUseEvent {
            workspace_paths: Vec::new(),
            artifact_directory_path: None,
            payload: ToolCallPayload::RunCommand {
                command_line: "jj status".to_owned(),
                cwd: None,
            },
        });
        let response = run_hook_event(&event, &|_| {
            Box::new(FakeVcsClient {
                description: String::new(),
                conflict: false,
            })
        });
        assert_eq!(response, HookResponse::PreToolUse(PreToolDecision::Allow));
    }

    #[test]
    fn run_hook_event_pre_tool_denies_jj_edit_on_described_commit() {
        let event = HookEvent::PreToolUse(PreToolUseEvent {
            workspace_paths: Vec::new(),
            artifact_directory_path: None,
            payload: ToolCallPayload::RunCommand {
                command_line: "jj edit main".to_owned(),
                cwd: None,
            },
        });
        let response = run_hook_event(&event, &|_| {
            Box::new(FakeVcsClient {
                description: "feat: described commit".to_owned(),
                conflict: false,
            })
        });
        assert!(matches!(
            response,
            HookResponse::PreToolUse(PreToolDecision::Deny { .. })
        ));
    }

    #[test]
    fn run_hook_event_pre_tool_allows_file_edit_even_when_working_copy_described() {
        let event = HookEvent::PreToolUse(PreToolUseEvent {
            workspace_paths: Vec::new(),
            artifact_directory_path: None,
            payload: ToolCallPayload::WriteFile {
                path: PathBuf::from("src/test.rs"),
                content: "fn test() {}\n".to_owned(),
                append: false,
            },
        });
        let response = run_hook_event(&event, &|_| {
            Box::new(FakeVcsClient {
                description: "feat: described working copy".to_owned(),
                conflict: false,
            })
        });
        assert_eq!(response, HookResponse::PreToolUse(PreToolDecision::Allow));
    }

    #[test]
    fn test_record_and_atomic_drain_warnings() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let artifact_dir = temp_dir.path();
        let queue_path = warning_queue_path(artifact_dir);

        let warnings = vec![
            "warning one\nwith details".to_owned(),
            "warning two".to_owned(),
            "warning one\nwith details".to_owned(),
        ];
        record_lint_warnings(artifact_dir, &warnings);
        let drained = drain_unique_warnings(&queue_path).expect("drain should succeed");

        assert_eq!(
            drained,
            indoc::indoc! {r"
                warning one
                with details

                warning two"}
        );
        assert!(!queue_path.is_file());
    }
}
