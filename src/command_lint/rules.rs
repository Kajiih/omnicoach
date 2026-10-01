//! Submodules containing implementations of command validation rules.
architecture_component!(CommandLintRules);

use crate::command_lint::rule::CommandDetector;
use crate::rule_declaration::Rule;

pub mod jj;

/// Static list of all command linter rules.
pub const COMMAND_RULES: &[Rule<dyn CommandDetector>] = &[jj::RULE];
