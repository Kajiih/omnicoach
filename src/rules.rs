//! Centralized rule and tag registry.

use serde::{Deserialize, Serialize};

macro_rules! define_tags {
    ($( $(#[doc = $doc:expr])* $variant:ident => $desc:expr ),* $(,)?) => {
        /// Metadata tags used to categorize rules.
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum Tag {
            $(
                $(#[doc = $doc])*
                $variant
            ),*
        }

        impl Tag {
            /// Returns the tag name as a static string slice.
            #[must_use]
            pub const fn as_str(&self) -> &'static str {
                match self {
                    $(Tag::$variant => stringify!($variant)),*
                }
            }

            /// Returns a human-readable description of the tag's purpose.
            #[must_use]
            pub const fn description(&self) -> &'static str {
                match self {
                    $(Tag::$variant => $desc),*
                }
            }
        }

        impl std::str::FromStr for Tag {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s.to_ascii_lowercase().as_str() {
                    $(s_low if s_low == stringify!($variant).to_ascii_lowercase() => Ok(Tag::$variant),)*
                    _ => Err(format!("Unknown tag '{}'", s)),
                }
            }
        }
    };
}

define_tags! {
    /// Checks related to logging configurations and invocations.
    Logging => "Checks related to logging configurations and invocations",
    /// Checks targeting exception handling structures.
    Exceptions => "Checks targeting exception handling structures",
    /// Checks targeting Python source code ASTs.
    Python => "Checks targeting Python source code ASTs",
    /// Checks targeting Rust source code ASTs.
    Rust => "Checks targeting Rust source code ASTs",
    /// Code style and formatting conventions.
    Style => "Code style and formatting conventions",
    /// Safety guidelines and command restrictions.
    Safety => "Safety guidelines and command restrictions",
    /// Command-line syntax checks.
    Cli => "Command-line syntax checks",
    /// Workflow execution rules.
    Workflow => "Workflow execution rules",
    /// Version control systems integrations.
    Vcs => "Version control systems integrations",
    /// JJ version control system.
    JJ => "JJ version control system",
}

/// Static list of all code linter rules.
pub const CODE_RULES: &[&dyn crate::code_lint::CodeRule] = &[
    &crate::code_lint::rules::py001_no_logging_in_except::NoLoggingInExcept,
    &crate::code_lint::rules::py002_flat_scope_enforced::FlatScopeEnforced,
    &crate::code_lint::rules::gen001_single_letter_variable_name::SingleLetterVariableName,
    &crate::code_lint::rules::gen002_banned_abbreviations::BannedAbbreviations,
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
