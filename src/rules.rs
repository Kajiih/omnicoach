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
    /// Checks targeting Python source code ASTs
    Python,
    /// Checks targeting Rust source code ASTs
    Rust,

    /// Identifier conventions, abbreviations, suffixes
    Naming,
    /// Asynchronous execution and structured concurrency
    Async,
    /// Test files, assertions, and mock hygiene
    Testing,
    /// Type annotations, dataclasses, and protocols
    Typing,
    /// Scope nesting, function length, and complexity
    Complexity,
    /// Checks related to logging configurations and invocations
    Logging,
    /// Checks targeting exception handling structures
    Exceptions,
    /// Inline and file-level suppression comment hygiene
    Suppression,
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

    /// Rule relies on heuristics and may trigger edge-case false positives
    Heuristic,
    /// Enforces team or architectural opinions beyond baseline bugs
    Opinionated,
    /// Detects likely bugs, resource leaks, or semantic anti-patterns
    Correctness,
}

impl Tag {
    /// Returns the tag name as a static string slice.
    // omni:ignore [NAME-002] -- idiomatic Rust conversion method name matching std::str
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        (*self).into()
    }

    /// Returns a human-readable description of the tag's purpose.
    #[must_use]
    pub fn description(&self) -> &'static str {
        self.get_documentation().unwrap_or_default().trim()
    }

    /// Returns the corresponding ast-grep `SupportLang` if this tag represents a language.
    #[must_use]
    pub const fn to_support_lang(&self) -> Option<ast_grep_language::SupportLang> {
        match self {
            Self::Python => Some(ast_grep_language::SupportLang::Python),
            Self::Rust => Some(ast_grep_language::SupportLang::Rust),
            _ => None,
        }
    }
}

/// Static list of all code linter rules.
pub const CODE_RULES: &[&dyn crate::code_lint::CodeRule] = &[
    &crate::code_lint::rules::async001_no_unstructured_task_creation::NoUnstructuredTaskCreation,
    &crate::code_lint::rules::test001_no_sleep_in_tests::NoSleepInTests,
    &crate::code_lint::rules::test002_max_test_assertions::MaxTestAssertions,
    &crate::code_lint::rules::py001_no_logging_in_except::NoLoggingInExcept,
    &crate::code_lint::rules::py002_flat_scope_enforced::FlatScopeEnforced,
    &crate::code_lint::rules::gen001_single_letter_variable_name::SingleLetterVariableName,
    &crate::code_lint::rules::gen002_banned_abbreviations::BannedAbbreviations,
    &crate::code_lint::rules::gen003_no_hungarian_notation::NoHungarianNotation,
    &crate::code_lint::suppression::MissingSuppressionReason,
    &crate::code_lint::suppression::UnusedSuppression,
    &crate::code_lint::suppression::UnknownSuppressionCode,
    &crate::code_lint::suppression::BlanketSuppression,
];

/// Static list of all command linter rules.
pub const COMMAND_RULES: &[&dyn crate::command_lint::CommandRule] =
    &[&crate::command_lint::rules::jj::NoJJEditOnDescribedCommits];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use strum::IntoEnumIterator;

    fn validate_rule(
        rule: &(impl crate::core::Rule + ?Sized),
        codes: &mut HashSet<&'static str>,
        names: &mut HashSet<&'static str>,
    ) {
        let code = rule.code().0;
        let name = rule.name().0;

        assert!(codes.insert(code), "Duplicate rule code found in registry: {code}");
        assert!(names.insert(name), "Duplicate rule name found in registry: {name}");
        let parts: Vec<&str> = code.split('-').collect();
        assert!(
            parts.len() == 2
                && !parts[0].is_empty()
                && parts[0].chars().all(|c| c.is_ascii_uppercase())
                && parts[1].len() == 3
                && parts[1].chars().all(|c| c.is_ascii_digit()),
            "Rule code '{code}' does not match standard pattern ^[A-Z]+-[0-9]{{3}}$"
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
            assert!(codes.insert(code), "Global rule code collision across registries: {code}");
            assert!(names.insert(name), "Global rule name collision across registries: {name}");
        }
    }

    #[test]
    fn test_code_rules_declare_supported_languages() {
        for rule in CODE_RULES {
            assert!(
                !rule.supported_languages().is_empty(),
                "Code rule {} must declare at least one supported language",
                rule.code().0
            );
        }
    }

    #[rstest]
    #[case::code_rules(CODE_RULES)]
    #[case::command_rules(COMMAND_RULES)]
    fn test_language_tags_are_derived_not_declared<R>(#[case] rules: &[&R])
    where
        R: crate::core::Rule + ?Sized,
    {
        for rule in rules {
            for tag in rule.tags() {
                assert!(
                    tag.to_support_lang().is_none(),
                    "Rule {} declares language tag {tag:?}; language tags are derived from supported_languages()",
                    rule.code().0
                );
            }
            for lang in rule.supported_languages() {
                let lang_tag = Tag::iter()
                    .find(|tag| tag.to_support_lang() == Some(*lang))
                    .expect("supported language must have a matching Tag variant");
                assert!(
                    rule.has_tag(lang_tag),
                    "Rule {} does not resolve derived language tag {lang_tag:?}",
                    rule.code().0
                );
            }
        }
    }
}
