//! Submodules containing implementations of command validation rules.
architecture_component!(CommandLintRules);

use crate::command_lint::rule::CommandDetector;
use crate::rule_taxonomy::Rule;

pub mod jj;

/// Static list of all command linter rules, each registered with its classification and doc.
pub const COMMAND_RULES: &[Rule<dyn CommandDetector>] = &[Rule {
    detector: &jj::NoJJEditOnDescribedCommits,
    classification: jj::NoJJEditOnDescribedCommits::CLASSIFICATION,
    doc: jj::NoJJEditOnDescribedCommits::DOC,
}];
