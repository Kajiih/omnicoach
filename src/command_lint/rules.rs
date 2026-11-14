//! The command rule registry.
architecture_component!(CommandLintRules);

use crate::command_lint::contract::CommandRule;

pub mod edit_of_described_commit;
pub mod global_flag_before_subcommand;

/// Every registered command rule.
pub const COMMAND_RULES: &[CommandRule] = &[
    edit_of_described_commit::RULE,
    global_flag_before_subcommand::RULE,
];
