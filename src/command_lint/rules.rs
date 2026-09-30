//! Submodules containing implementations of command validation rules.
architecture_component!(CommandLintRules);

use crate::command_lint::rule::CommandRule;
use crate::rule_taxonomy::ClassifiedRule;

pub mod jj;

/// Static list of all command linter rules, each registered with its classification.
pub const COMMAND_RULES: &[ClassifiedRule<dyn CommandRule>] = &[ClassifiedRule {
    rule: &jj::NoJJEditOnDescribedCommits,
    classification: jj::NoJJEditOnDescribedCommits::CLASSIFICATION,
}];
