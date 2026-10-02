//! The command rule registry.
architecture_component!(CommandLintRules);

use crate::command_lint::rule::CommandRule;

pub mod edit_of_described_commit;

/// Every registered command rule.
pub const COMMAND_RULES: &[CommandRule] = &[edit_of_described_commit::RULE];
