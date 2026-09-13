//! Centralized rule and tag registry.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumMessage, EnumString, IntoStaticStr};

/// Metadata tags used to categorize rules.
#[derive(
    Copy,
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    EnumIter,
    IntoStaticStr,
    EnumMessage,
)]
#[strum(ascii_case_insensitive)]
pub enum Tag {
    /// Checks related to logging configurations and invocations
    Logging,
    /// Checks targeting exception handling structures
    Exceptions,
    /// Checks targeting Python source code ASTs
    Python,
    /// Checks targeting Rust source code ASTs
    Rust,
    /// Code style and formatting conventions
    Style,
    /// Safety guidelines and command restrictions
    Safety,
    /// Command-line syntax checks
    Cli,
    /// Workflow execution rules
    Workflow,
    /// Version control systems integrations
    Vcs,
    /// JJ version control system
    JJ,
}

impl Tag {
    /// Returns the tag name as a static string slice.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        (*self).into()
    }

    /// Returns a human-readable description of the tag's purpose.
    #[must_use]
    pub fn description(&self) -> &'static str {
        self.get_documentation().unwrap_or_default().trim()
    }
}

/// Static list of all code linter rules.
pub const CODE_RULES: &[&dyn crate::code_lint::CodeRule] = &[
    &crate::code_lint::rules::py001_no_logging_in_except::NoLoggingInExcept,
    &crate::code_lint::rules::py002_flat_scope_enforced::FlatScopeEnforced,
    &crate::code_lint::rules::gen001_single_letter_variable_name::SingleLetterVariableName,
    &crate::code_lint::rules::gen002_banned_abbreviations::BannedAbbreviations,
    &crate::code_lint::rules::gen003_no_hungarian_notation::NoHungarianNotation,
];

/// Static list of all command linter rules.
pub const COMMAND_RULES: &[&dyn crate::command_lint::CommandRule] =
    &[&crate::command_lint::rules::jj::NoJJEditOnDescribedCommits];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn validate_rule(
        rule: &(impl crate::core::Rule + ?Sized),
        codes: &mut HashSet<&'static str>,
        names: &mut HashSet<&'static str>,
    ) {
        let code = rule.code().0;
        let name = rule.name().0;

        assert!(
            codes.insert(code),
            "Duplicate rule code found in registry: {code}"
        );
        assert!(
            names.insert(name),
            "Duplicate rule name found in registry: {name}"
        );
        assert!(
            code.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                && code
                    .chars()
                    .skip(1)
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()),
            "Rule code '{code}' does not match standard pattern"
        );
    }

    use rstest::rstest;

    #[rstest]
    #[case::code_rules(CODE_RULES)]
    #[case::command_rules(COMMAND_RULES)]
    fn test_registry_integrity<R>(#[case] rules: &[&R])
    where
        R: crate::core::Rule + ?Sized,
    {
        let mut codes = HashSet::new();
        let mut names = HashSet::new();
        for rule in rules {
            validate_rule(*rule, &mut codes, &mut names);
        }
    }

    #[test]
    fn test_global_registry_uniqueness() {
        let mut codes = HashSet::new();
        let mut names = HashSet::new();

        for rule in CODE_RULES {
            codes.insert(rule.code().0);
            names.insert(rule.name().0);
        }

        for rule in COMMAND_RULES {
            let code = rule.code().0;
            let name = rule.name().0;
            assert!(
                codes.insert(code),
                "Global rule code collision across registries: {code}"
            );
            assert!(
                names.insert(name),
                "Global rule name collision across registries: {name}"
            );
        }
    }
}
