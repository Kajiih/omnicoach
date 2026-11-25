//! Orchestration runner for command lint evaluation.

architecture_component!(CommandLintRunner);

use crate::command_lint::command::ParsedCommand;
use crate::command_lint::contract::{CommandContext, CommandEnv};
use crate::command_lint::rules::COMMAND_RULES;
use crate::command_lint::vcs::{JjCliClient, JjClient};
use crate::config::Config;
use crate::diagnostic::Diagnostic;

/// Evaluates all enabled command rules against `raw_cmd` in `context`.
#[must_use]
pub fn run_command_lint(
    raw_cmd: &str,
    config: &Config,
    context: &CommandContext,
) -> Vec<Diagnostic> {
    run_with_client(raw_cmd, config, context, &JjCliClient)
}

/// Evaluates all enabled command rules against `raw_cmd` in `context` using an injected
/// [`JjClient`] fake.
#[cfg(test)]
#[must_use]
pub(crate) fn run_command_lint_with(
    raw_cmd: &str,
    config: &Config,
    context: &CommandContext,
    jj_client: &dyn JjClient,
) -> Vec<Diagnostic> {
    run_with_client(raw_cmd, config, context, jj_client)
}

fn run_with_client(
    raw_cmd: &str,
    config: &Config,
    context: &CommandContext,
    jj_client: &dyn JjClient,
) -> Vec<Diagnostic> {
    let parsed = ParsedCommand::new(raw_cmd);
    let env = CommandEnv::new(jj_client, context);
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
    use std::cell::{Cell, RefCell};
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::command_lint::vcs::CommitSummary;

    struct CountingJjClient {
        calls: Cell<usize>,
        queried_dirs: RefCell<Vec<PathBuf>>,
    }

    impl JjClient for CountingJjClient {
        fn query_revisions(
            &self,
            repo_dir: &Path,
            _revset: &str,
        ) -> Result<Vec<CommitSummary>, String> {
            self.calls.set(self.calls.get() + 1);
            self.queried_dirs.borrow_mut().push(repo_dir.to_path_buf());
            Ok(vec![CommitSummary {
                change_id: "kpqxywon".to_owned(),
                description: "feat: described".to_owned(),
                ..CommitSummary::default()
            }])
        }
    }

    #[test]
    fn test_run_command_lint_evaluates_syntactic_command_rules() {
        let context = CommandContext::new(".");
        let diagnostics = run_command_lint("jj --no-pager status", &Config::default(), &context);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule_name.0, "global-flag-before-subcommand");
    }

    #[test]
    fn test_run_command_lint_shares_cached_jj_session_and_forwards_context_dir() {
        let client = CountingJjClient {
            calls: Cell::new(0),
            queried_dirs: RefCell::new(Vec::new()),
        };
        let context = CommandContext::new("/ws");
        let diagnostics = run_command_lint_with(
            "jj edit kpqxywon && jj edit kpqxywon",
            &Config::default(),
            &context,
            &client,
        );
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(client.calls.get(), 1);
        assert_eq!(client.queried_dirs.into_inner(), vec![PathBuf::from("/ws")]);
    }
}
