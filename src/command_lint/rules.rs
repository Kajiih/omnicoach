//! Submodules containing implementations of command validation rules.
architecture_component!(CommandLintRules);

pub mod jj;

/// Static list of all command linter rules.
pub const COMMAND_RULES: &[&dyn crate::command_lint::rule::CommandRule] =
    &[&jj::NoJJEditOnDescribedCommits];
