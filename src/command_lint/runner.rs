//! Orchestration runner for command lint evaluation.

architecture_component!(CommandLintRunner);

use std::path::{Path, PathBuf};

use crate::command_lint::command::ParsedCommand;
use crate::command_lint::contract::CommandEnv;
use crate::command_lint::rules::COMMAND_RULES;
use crate::command_lint::vcs::{JjCliClient, JjClient};
use crate::config::Config;
use crate::diagnostic::Diagnostic;

/// Evaluates all enabled command rules against `raw_cmd` in the process's current working directory.
#[must_use]
pub fn run_command_lint(raw_cmd: &str, config: &Config) -> Vec<Diagnostic> {
    let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    run_command_lint_in_dir(raw_cmd, config, &working_dir)
}

/// Evaluates all enabled command rules against `raw_cmd` rooted at `working_dir`.
#[must_use]
pub(crate) fn run_command_lint_in_dir(
    raw_cmd: &str,
    config: &Config,
    working_dir: &Path,
) -> Vec<Diagnostic> {
    run_with_client(raw_cmd, config, working_dir, &JjCliClient)
}

/// Evaluates all enabled command rules against `raw_cmd` rooted at `working_dir` using an injected
/// [`JjClient`] fake.
#[cfg(test)]
#[must_use]
pub(crate) fn run_command_lint_in_dir_with(
    raw_cmd: &str,
    config: &Config,
    working_dir: &Path,
    jj_client: &dyn JjClient,
) -> Vec<Diagnostic> {
    run_with_client(raw_cmd, config, working_dir, jj_client)
}

fn run_with_client(
    raw_cmd: &str,
    config: &Config,
    working_dir: &Path,
    jj_client: &dyn JjClient,
) -> Vec<Diagnostic> {
    let parsed = ParsedCommand::new(raw_cmd);
    let env = CommandEnv::with_working_dir(jj_client, working_dir);
    let mut all_diagnostics = Vec::new();

    for rule in COMMAND_RULES {
        if config.is_rule_enabled(rule.declaration.name) {
            all_diagnostics.extend(rule.check_command(&parsed, &env));
        }
    }

    all_diagnostics
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::command_lint::vcs::CommitSummary;

    struct CountingJjClient {
        calls: Cell<usize>,
    }

    impl JjClient for CountingJjClient {
        fn query_revisions(
            &self,
            _repo_dir: &Path,
            _revset: &str,
        ) -> Result<Vec<CommitSummary>, String> {
            self.calls.set(self.calls.get() + 1);
            Ok(vec![CommitSummary {
                change_id: "kpqxywon".to_owned(),
                description: "feat: described".to_owned(),
                ..CommitSummary::default()
            }])
        }
    }

    #[test]
    fn test_run_command_lint_evaluates_syntactic_command_rules() {
        let diagnostics = run_command_lint("jj --no-pager status", &Config::default());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule_name.0, "global-flag-before-subcommand");
    }

    #[test]
    fn test_run_command_lint_shares_cached_jj_session_within_run() {
        let client = CountingJjClient {
            calls: Cell::new(0),
        };
        let diagnostics = run_command_lint_in_dir_with(
            "jj edit kpqxywon && jj edit kpqxywon",
            &Config::default(),
            Path::new("/ws"),
            &client,
        );
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(client.calls.get(), 1);
    }
}
