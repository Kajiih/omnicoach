//! Orchestration runner for command lint evaluation.

architecture_component!(CommandLintRunner);

use crate::command_lint::command::ParsedCommand;
use crate::command_lint::contract::CommandEnv;
use crate::command_lint::rules::COMMAND_RULES;
use crate::command_lint::vcs::{JjCliClient, JjClient};
use crate::config::Config;
use crate::diagnostic::Diagnostic;

/// Evaluates all enabled command rules against the provided raw command string.
#[must_use]
pub fn run_command_lint(raw_cmd: &str, config: &Config) -> Vec<Diagnostic> {
    let jj_client = JjCliClient::new();
    run_command_lint_with_client(raw_cmd, config, &jj_client)
}

/// Evaluates all enabled command rules against the provided raw command string using a custom VCS client.
#[must_use]
pub(crate) fn run_command_lint_with_client(
    raw_cmd: &str,
    config: &Config,
    jj_client: &dyn JjClient,
) -> Vec<Diagnostic> {
    let parsed = ParsedCommand::new(raw_cmd);
    let env = CommandEnv::new(jj_client);
    let mut all_diagnostics = Vec::new();

    for rule in COMMAND_RULES {
        if config.is_rule_enabled(rule.declaration.name) {
            all_diagnostics.extend(rule.check_command(&parsed, &env));
        }
    }

    all_diagnostics
}
