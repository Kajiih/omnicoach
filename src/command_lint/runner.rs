//! Orchestration runner for command lint evaluation.

architecture_component!(CommandLintRunner);

use crate::command_lint::rule::InterceptedCommand;
use crate::command_lint::rules::COMMAND_RULES;
use crate::command_lint::vcs::JjCliClient;
use crate::core::Config;
use crate::diagnostic::Diagnostic;

/// Evaluates all enabled command rules against the provided raw command string.
#[must_use]
pub fn run_command_lint(raw_cmd: &str, config: &Config) -> Vec<Diagnostic> {
    let commands = InterceptedCommand::parse_all(raw_cmd);
    let jj_client = JjCliClient;
    let mut all_diagnostics = Vec::new();

    for cmd in &commands {
        for registered in COMMAND_RULES {
            let rule = registered.detector;
            if config.is_rule_enabled(rule.name()) {
                all_diagnostics.extend(rule.check_command(cmd, &jj_client));
            }
        }
    }

    all_diagnostics
}
