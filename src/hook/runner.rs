//! Agent-neutral evaluation of an intercepted tool call: command lint for shell commands and
//! diff-scoped code lint for file edits.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::event::{FileEdit, PreToolDecision, PreToolUseEvent, ToolCallPayload};
use super::virtual_edit::{VirtualFile, apply_virtual_edit};
use crate::code_lint::runner::{lint_virtual_file, lint_virtual_file_for_rules};
use crate::command_lint::runner::run_command_lint_with_client;
use crate::command_lint::vcs::{JjCliClient, JjClient};
use crate::config::{Config, lexical_normalize};
use crate::diagnostic::{Diagnostic, Language, RuleName};
use crate::rule_selection::{
    ConfigOverrides, discover_project_root, load_command_config_from_root, load_config_from_root,
};

type JjClientFactory<'a> = &'a dyn Fn(&Path) -> Box<dyn JjClient>;

/// Outcome of evaluating a tool call before it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ToolCallOutcome {
    /// Whether the tool call may run.
    pub(super) decision: PreToolDecision,
    /// Code lint warnings introduced by a file edit (on modified lines or net-new on unmodified
    /// lines), one formatted warning per diagnostic. How and when they reach the model depends on
    /// what the agent's hooks support, so delivering them is the adapter's job.
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
    let Some(config) = load_hook_config(&project_root, load_command_config_from_root) else {
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
                indoc::formatdoc! {"
                    [{}] {}
                      Rationale: {}
                      Suggestion: {}",
                    diagnostic.rule_name,
                    diagnostic.message.summary,
                    diagnostic.message.rationale,
                    diagnostic.message.suggestion
                }
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
                && target_file_path.starts_with(lexical_normalize(agent_owned_dir))
        });
    if is_agent_owned_file {
        return Vec::new();
    }

    if !is_within_workspace(&target_file_path, &event.workspace_paths) {
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
    let Some(config) = load_hook_config(&project_root, load_config_from_root) else {
        return Vec::new();
    };

    let post_edit_diagnostics = match lint_virtual_file(
        &project_root,
        &target_file_path,
        &virtual_file.content,
        &config,
    ) {
        Ok(diagnostics) => diagnostics,
        Err(error) => {
            eprintln!(
                "[omnicoach hook] skipping lint of {}: {error}",
                target_file_path.display()
            );
            return Vec::new();
        }
    };
    filter_relevant_diagnostics(
        &project_root,
        &target_file_path,
        original_content.as_deref(),
        &virtual_file,
        &config,
        post_edit_diagnostics,
    )
    .iter()
    .map(|diagnostic| format_code_warning(&target_file_path, diagnostic))
    .collect()
}

fn is_within_workspace(target_file_path: &Path, workspace_paths: &[PathBuf]) -> bool {
    workspace_paths.is_empty()
        || workspace_paths
            .iter()
            .map(|workspace| lexical_normalize(workspace))
            .any(|workspace| target_file_path.starts_with(&workspace))
}

/// Keeps diagnostics on lines touched by the edit, plus net-new spillover onto unmodified
/// lines whose `(rule_name, summary)` multiplicity exceeds the pre-edit baseline.
///
/// The baseline lints the pre-edit source with only the rules that reported spillover, since no
/// other rule's counts are consulted. Modified-line diagnostics bypass `baseline_counts` without
/// decrementing it so adding a violation on a modified line never unmasks a matching
/// pre-existing violation on an unmodified line.
fn filter_relevant_diagnostics(
    project_root: &Path,
    target_file_path: &Path,
    original_content: Option<&str>,
    virtual_file: &VirtualFile,
    config: &Config,
    post_edit_diagnostics: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    let spillover_rules: Vec<RuleName> = post_edit_diagnostics
        .iter()
        .filter(|diagnostic| {
            !virtual_file
                .modified_lines
                .contains(&diagnostic.location.line)
        })
        .map(|diagnostic| diagnostic.rule_name)
        .collect();
    if spillover_rules.is_empty() {
        return post_edit_diagnostics;
    }

    let Some(before_source) = original_content else {
        return post_edit_diagnostics;
    };
    let Ok(baseline_diagnostics) = lint_virtual_file_for_rules(
        project_root,
        target_file_path,
        before_source,
        config,
        &spillover_rules,
    ) else {
        return post_edit_diagnostics;
    };

    let mut baseline_counts: HashMap<(RuleName, String), usize> = HashMap::new();
    for diagnostic in baseline_diagnostics {
        *baseline_counts
            .entry((diagnostic.rule_name, diagnostic.message.summary))
            .or_default() += 1;
    }

    post_edit_diagnostics
        .into_iter()
        .filter(|diagnostic| {
            if virtual_file
                .modified_lines
                .contains(&diagnostic.location.line)
            {
                return true;
            }
            let key = (diagnostic.rule_name, diagnostic.message.summary.clone());
            if let Some(count) = baseline_counts.get_mut(&key)
                && *count > 0
            {
                *count -= 1;
                false
            } else {
                true
            }
        })
        .collect()
}

/// Formats a diagnostic for warning delivery. The first-line `<path>:<line>:<col>: [<rule>] <summary>`
/// layout is parsed by `super::antigravity::extract_rule_name` to resolve rule classifications.
fn format_code_warning(target_path: &Path, diagnostic: &Diagnostic) -> String {
    let mut warning = indoc::formatdoc! {"
        {}:{}:{}: [{}] {}
          Rationale: {}
          Suggestion: {}",
        target_path.display(),
        diagnostic.location.line,
        diagnostic.location.column,
        diagnostic.rule_name,
        diagnostic.message.summary,
        diagnostic.message.rationale,
        diagnostic.message.suggestion
    };
    if let Some(hint) = &diagnostic.message.explanation_hint {
        let _ = write!(warning, "\n  Or: {hint}");
    }
    warning
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
    lexical_normalize(&joined)
}

fn resolve_project_root(directory: &Path, workspace_paths: &[PathBuf]) -> PathBuf {
    discover_project_root(directory)
        .or_else(|| {
            workspace_paths
                .iter()
                .map(|workspace| lexical_normalize(workspace))
                .find(|workspace| directory.starts_with(workspace))
        })
        .unwrap_or_else(|| directory.to_path_buf())
}

fn load_hook_config<E: std::fmt::Display>(
    project_root: &Path,
    load: impl FnOnce(&Path, &ConfigOverrides) -> Result<Config, E>,
) -> Option<Config> {
    match load(project_root, &ConfigOverrides::default()) {
        Ok(config) => Some(config),
        Err(error) => {
            eprintln!("[omnicoach hook] ignoring invalid config (failing open): {error}");
            None
        }
    }
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

    #[test]
    fn evaluate_pre_tool_use_skips_file_outside_workspace_paths() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace = temp_dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let bad_code = indoc::indoc! {"
            pub fn compute() -> i32 {
                let err = 1;
                err
            }
        "};
        let outside_file = temp_dir.path().join("scratch.rs");

        let event = PreToolUseEvent {
            workspace_paths: vec![workspace],
            agent_owned_dir: None,
            payload: ToolCallPayload::FileEdit {
                path: outside_file,
                edit: FileEdit::Overwrite(bad_code.to_owned()),
            },
        };
        let outcome = evaluate_pre_tool_use(&event);
        assert_eq!(outcome.warnings, Vec::<String>::new());
    }

    #[test]
    fn evaluate_pre_tool_use_reports_net_new_spillover_on_unmodified_line_and_suppresses_legacy_debt()
     {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("billing.py");
        let initial_code = indoc::indoc! {r#"
            def legacy():
                x = 1
                return x

            def check_status(status: str) -> bool:
                return status == "settled"
        "#};
        std::fs::write(&file_path, initial_code).unwrap();

        let edit_code = indoc::indoc! {r#"
            STATUS_SETTLED = "settled"

            def is_settled(status: str) -> bool:
                return status == STATUS_SETTLED
        "#};

        let event = PreToolUseEvent {
            workspace_paths: vec![temp_dir.path().to_path_buf()],
            agent_owned_dir: None,
            payload: ToolCallPayload::FileEdit {
                path: file_path,
                edit: FileEdit::Append(edit_code.to_owned()),
            },
        };
        let outcome = evaluate_pre_tool_use(&event);

        assert_eq!(outcome.warnings.len(), 1);
        assert!(outcome.warnings[0].contains("repeated-literal"));
        assert!(outcome.warnings[0].contains("\"settled\""));
        assert!(
            !outcome
                .warnings
                .iter()
                .any(|warning| warning.contains("single-letter-name"))
        );
    }

    #[test]
    fn evaluate_pre_tool_use_does_not_unmask_unmodified_violation_when_same_violation_added_above()
    {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("logic.py");
        let initial_code = indoc::indoc! {r"
            def legacy():
                k = 1
                return k
        "};
        std::fs::write(&file_path, initial_code).unwrap();

        let replacement_code = indoc::indoc! {r"
            def new_worker():
                k = 2
                return k

            def legacy():
                k = 1
                return k
        "};

        let event = PreToolUseEvent {
            workspace_paths: vec![temp_dir.path().to_path_buf()],
            agent_owned_dir: None,
            payload: ToolCallPayload::FileEdit {
                path: file_path,
                edit: FileEdit::Overwrite(replacement_code.to_owned()),
            },
        };
        let outcome = evaluate_pre_tool_use(&event);

        let single_letter_warnings: Vec<&String> = outcome
            .warnings
            .iter()
            .filter(|warning| warning.contains("single-letter-name"))
            .collect();
        assert_eq!(single_letter_warnings.len(), 1);
        assert!(single_letter_warnings[0].contains("logic.py:2:"));
    }

    #[test]
    fn evaluate_pre_tool_use_suppresses_legacy_unused_suppression_on_unmodified_line() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("legacy.py");
        let initial_code = indoc::indoc! {r"
            def legacy():
                value = 1  # omni:ignore [nested-function] -- stale directive
                return value
        "};
        std::fs::write(&file_path, initial_code).unwrap();

        let event = PreToolUseEvent {
            workspace_paths: vec![temp_dir.path().to_path_buf()],
            agent_owned_dir: None,
            payload: ToolCallPayload::FileEdit {
                path: file_path,
                edit: FileEdit::Append(
                    indoc::indoc! {r"


                        def total():
                            return 2
                    "}
                    .to_owned(),
                ),
            },
        };
        let outcome = evaluate_pre_tool_use(&event);

        assert!(
            !outcome
                .warnings
                .iter()
                .any(|warning| warning.contains("unused-suppression")),
            "{:?}",
            outcome.warnings
        );
    }

    #[test]
    fn format_code_warning_includes_rationale_suggestion_and_optional_hint() {
        use crate::diagnostic::{LineColumn, SourceLocation, SourceSpan, ViolationMessage};

        let location = SourceLocation::file_span(
            "sample.rs",
            SourceSpan::new(0, 10),
            LineColumn { line: 1, column: 1 },
        );
        let mut message = ViolationMessage::new(
            "Summary message.",
            "Rationale explanation.",
            "Suggestion fix.",
        );
        message.explanation_hint = Some("Comment hint.".to_owned());

        let diagnostic = Diagnostic::new(RuleName("sample-rule"), message, location);
        let warning = format_code_warning(Path::new("sample.rs"), &diagnostic);

        assert!(warning.contains("sample.rs:1:1: [sample-rule] Summary message."));
        assert!(warning.contains("Rationale: Rationale explanation."));
        assert!(warning.contains("Suggestion: Suggestion fix."));
        assert!(warning.contains("Or: Comment hint."));
    }

    #[test]
    fn evaluate_pre_tool_use_handles_cross_file_spillover_and_suppresses_legacy_cross_file_debt() {
        let temp_dir = tempfile::tempdir().unwrap();
        let pkg_dir = temp_dir.path().join("pkg");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            temp_dir.path().join("pyproject.toml"),
            "[project]\nname = \"demo\"\n",
        )
        .unwrap();
        std::fs::write(pkg_dir.join("__init__.py"), "").unwrap();
        std::fs::write(
            pkg_dir.join("interfaces.py"),
            indoc::indoc! {r"
                from typing import Protocol

                class Store(Protocol):
                    def load(self, key: str) -> str: ...
                    def save(self, key: str, value: str) -> None: ...

                class Codec(Protocol):
                    def encode(self, item: str) -> bytes: ...
                    def decode(self, payload: bytes) -> str: ...
            "},
        )
        .unwrap();

        let service_path = pkg_dir.join("service.py");
        let initial_service = indoc::indoc! {r"
            from .interfaces import Codec

            class LegacyCodec:
                def encode(self, item: str) -> bytes:
                    return item.encode()

                def decode(self, payload: bytes) -> str:
                    return payload.decode()

            class MemoryStore:
                def load(self, key: str) -> str:
                    return key

                def save(self, key: str, value: str) -> None:
                    pass
        "};
        std::fs::write(&service_path, initial_service).unwrap();

        let edited_service = initial_service.replacen(
            "from .interfaces import Codec",
            "from .interfaces import Codec, Store",
            1,
        );
        let event = PreToolUseEvent {
            workspace_paths: vec![temp_dir.path().to_path_buf()],
            agent_owned_dir: None,
            payload: ToolCallPayload::FileEdit {
                path: service_path,
                edit: FileEdit::Overwrite(edited_service),
            },
        };
        let outcome = evaluate_pre_tool_use(&event);

        assert_eq!(
            outcome.warnings.len(),
            1,
            "warnings: {:?}",
            outcome.warnings
        );
        assert!(outcome.warnings[0].contains("[uninherited-protocol]"));
        assert!(outcome.warnings[0].contains("MemoryStore"));
        assert!(!outcome.warnings[0].contains("LegacyCodec"));
    }

    #[test]
    fn evaluate_pre_tool_use_fails_open_when_architecture_declarations_conflict() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
        std::fs::write(
            root.join(".omnicoach.toml"),
            indoc::indoc! {r#"
                [[architecture.components]]
                name = "Core"
                roots = ["core"]
            "#},
        )
        .unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            indoc::indoc! {r"
                mod architecture;
                mod core;
            "},
        )
        .unwrap();
        std::fs::write(
            root.join("src/architecture.rs"),
            indoc::indoc! {r"
                define_architecture! {
                    Core => [],
                }
            "},
        )
        .unwrap();
        let event = PreToolUseEvent {
            workspace_paths: vec![root.to_path_buf()],
            agent_owned_dir: None,
            payload: ToolCallPayload::FileEdit {
                path: root.join("src/core.rs"),
                edit: FileEdit::Overwrite(
                    indoc::indoc! {r"
                        pub fn compute() -> i32 {
                            let err = 1;
                            err
                        }
                    "}
                    .to_owned(),
                ),
            },
        };
        let outcome = evaluate_pre_tool_use(&event);

        assert_eq!(outcome.warnings, Vec::<String>::new());
    }
}
