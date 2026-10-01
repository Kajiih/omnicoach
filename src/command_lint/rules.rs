//! The command rule registry.
architecture_component!(CommandLintRules);

use crate::command_lint::rule::CommandRule;

pub mod jj;

/// Every registered command rule.
pub const COMMAND_RULES: &[CommandRule] = &[jj::RULE];
