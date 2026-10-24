//! Orchestration runner for command lint evaluation.

architecture_component!(CommandLintRunner);

use crate::command_lint::command::InterceptedCommand;
use crate::command_lint::rules::COMMAND_RULES;
use crate::command_lint::vcs::{JjCliClient, JjClient};
use crate::config::Config;
use crate::diagnostic::Diagnostic;

/// Evaluates all enabled command rules against the provided raw command string.
#[must_use]
pub fn run_command_lint(raw_command: &str, config: &Config) -> Vec<Diagnostic> {
    let jj_client = JjCliClient::new();
    run_command_lint_with_client(raw_command, config, &jj_client)
}

/// Evaluates all enabled command rules against the provided raw command string using a custom VCS client.
#[must_use]
pub fn run_command_lint_with_client(
    raw_command: &str,
    config: &Config,
    jj_client: &dyn JjClient,
) -> Vec<Diagnostic> {
    let commands = InterceptedCommand::parse_all(raw_command);
    let mut all_diagnostics = Vec::new();

    for command in &commands {
        for rule in COMMAND_RULES {
            if config.is_rule_enabled(rule.declaration.name) {
                all_diagnostics.extend(rule.check_command(command, jj_client));
            }
        }
    }

    all_diagnostics
}
