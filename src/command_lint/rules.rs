//! Submodules containing implementations of command validation rules.
architecture_component!(CommandLintRules);

use crate::command_lint::rule::CommandRule;
use crate::rule_taxonomy::RuleEntry;

pub mod jj;

/// Static list of all command linter rules, each registered with its classification and doc.
pub const COMMAND_RULES: &[RuleEntry<dyn CommandRule>] = &[RuleEntry {
    rule: &jj::NoJJEditOnDescribedCommits,
    classification: jj::NoJJEditOnDescribedCommits::CLASSIFICATION,
    doc: jj::NoJJEditOnDescribedCommits::DOC,
}];
