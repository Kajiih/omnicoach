//! Command safety linter rules.

architecture_component!(CommandLintRules);

use crate::command_lint::contract::CommandRule;

pub mod divergent_jj_push;
pub mod edit_of_described_commit;
pub mod global_flag_before_subcommand;
pub mod interactive_jj_command;
pub mod shared_workspace_rewrite;
pub mod unbounded_jj_log;

/// Every registered command rule.
pub const COMMAND_RULES: &[CommandRule] = &[
    divergent_jj_push::RULE,
    edit_of_described_commit::RULE,
    global_flag_before_subcommand::RULE,
    interactive_jj_command::RULE,
    shared_workspace_rewrite::RULE,
    unbounded_jj_log::RULE,
];
