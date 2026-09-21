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
    /// Hidden global state, ambient dependencies, and other impurity-inducing side effects
    #[serde(rename = "side-effects")]
    #[strum(serialize = "side-effects")]
    SideEffects,
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

    /// Returns the corresponding ast-grep `SupportLang` if this tag represents a language.
    #[must_use]
    pub const fn to_support_lang(self) -> Option<ast_grep_language::SupportLang> {
        match self {
            Self::Python => Some(ast_grep_language::SupportLang::Python),
            Self::Rust => Some(ast_grep_language::SupportLang::Rust),
            _ => None,
        }
    }
}

// TODO: Why do we have both rules listed here and in code_lint/rules.rs?
/// Static list of all code linter rules.
pub const CODE_RULES: &[&dyn crate::code_lint::CodeRule] = &[
    &crate::code_lint::rules::no_unstructured_task_creation::NoUnstructuredTaskCreation,
    &crate::code_lint::rules::no_sleep_in_tests::NoSleepInTests,
    &crate::code_lint::rules::no_sleep_in_tests::NoZeroSleepInTests,
    &crate::code_lint::rules::max_test_assertions::MaxTestAssertions,
    &crate::code_lint::rules::no_assertion_packing::NoAssertionPacking,
    &crate::code_lint::rules::no_mocks_in_tests::NoMocksInTests,
    &crate::code_lint::rules::no_mock_assertions::NoMockAssertions,
    &crate::code_lint::rules::no_logging_error_in_except::NoLoggingErrorInExcept,
    &crate::code_lint::rules::no_uncommented_suppress::NoUncommentedSuppress,
    &crate::code_lint::rules::no_typing_cast::NoTypingCast,
    &crate::code_lint::rules::no_dynamic_attribute_access::NoDynamicAttributeAccess,
    &crate::code_lint::rules::flat_scope_enforced::FlatScopeEnforced,
    &crate::code_lint::rules::single_letter_variable_name::SingleLetterVariableName,
    &crate::code_lint::rules::banned_abbreviations::BannedAbbreviations,
    &crate::code_lint::rules::no_hungarian_notation::NoHungarianNotation,
    &crate::code_lint::rules::prefer_timedelta_over_seconds::PreferTimedeltaOverSeconds,
    &crate::code_lint::rules::no_identical_positional_types::NoIdenticalPositionalTypes,
    &crate::code_lint::rules::no_env_in_functions::NoEnvInFunctions,
    &crate::code_lint::rules::enforce_frozen_slots_dataclass::EnforceFrozenSlotsDataclass,
    &crate::code_lint::rules::prefer_dedent_for_multiline_strings::PreferDedentForMultilineStrings,
    &crate::code_lint::suppression::MissingSuppressionReason,
    &crate::code_lint::suppression::UnusedSuppression,
    &crate::code_lint::suppression::UnknownSuppressionRule,
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

    fn is_kebab_case(name: &str) -> bool {
        !name.is_empty()
            && !name.starts_with('-')
            && !name.ends_with('-')
            && name.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
            })
    }

    fn validate_rule(rule: &(impl crate::core::Rule + ?Sized), names: &mut HashSet<&'static str>) {
        let name = rule.name().0;

        assert!(
            names.insert(name),
            "Duplicate rule name found in registry: {name}"
        );
        assert!(
            is_kebab_case(name),
            "Rule name '{name}' does not match standard kebab-case pattern"
        );
        assert!(
            !rule.tags().is_empty(),
            "Rule {name} must declare at least one domain tag"
        );

        let template = rule.violation_template();
        for field in [template.summary, template.rationale, template.suggestion] {
            for text in std::iter::once(field.base).chain(
                field
                    .overrides
                    .iter()
                    .map(|(_, override_text)| *override_text),
            ) {
                let trimmed = text.trim();
                assert!(
                    !trimmed.is_empty(),
                    "Rule {name} has an empty template field"
                );
                let is_bare_placeholder = trimmed.starts_with('{')
                    && trimmed.ends_with('}')
                    && !trimmed[1..trimmed.len() - 1].contains(['{', '}']);
                assert!(
                    !is_bare_placeholder,
                    "Rule {name} template field '{trimmed}' must not be a bare placeholder pass-through"
                );
            }
            let mut seen_langs = HashSet::new();
            for (lang, _) in field.overrides {
                assert!(
                    seen_langs.insert(lang),
                    "Rule {name} has duplicate template override for {lang:?}"
                );
                assert!(
                    rule.supported_languages().contains(lang),
                    "Rule {name} declares template override for {lang:?}, which is not in supported_languages()"
                );
            }
        }
    }

    use rstest::rstest;

    #[rstest]
    #[case::code_rules(CODE_RULES)]
    #[case::command_rules(COMMAND_RULES)]
    fn test_registry_integrity<R>(#[case] rules: &[&R])
    where
        R: crate::core::Rule + ?Sized,
    {
        let mut names = HashSet::new();
        for rule in rules {
            validate_rule(*rule, &mut names);
        }
    }

    #[test]
    fn test_global_registry_uniqueness() {
        let mut names = HashSet::new();

        for rule in CODE_RULES {
            names.insert(rule.name().0);
        }

        for rule in COMMAND_RULES {
            let name = rule.name().0;
            assert!(
                names.insert(name),
                "Global rule name collision across registries: {name}"
            );
        }
    }

    #[test]
    fn test_code_rules_declare_supported_languages() {
        for rule in CODE_RULES {
            assert!(
                !rule.supported_languages().is_empty(),
                "Code rule {} must declare at least one supported language",
                rule.name().0
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
                    rule.name().0
                );
            }
            for lang in rule.supported_languages() {
                let lang_tag = Tag::iter()
                    .find(|tag| tag.to_support_lang() == Some(*lang))
                    .expect("supported language must have a matching Tag variant");
                assert!(
                    rule.has_tag(lang_tag),
                    "Rule {} does not resolve derived language tag {lang_tag:?}",
                    rule.name().0
                );
            }
        }
    }

    /// Diagnostic ordering is owned by the reporting layer (see [`crate::code_lint::CodeRule::check_file`]),
    /// so a rule sorting its own output is dead work.
    #[test]
    fn test_rule_sources_do_not_sort_diagnostics() {
        let rules_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/code_lint/rules");
        let suppression = concat!(env!("CARGO_MANIFEST_DIR"), "/src/code_lint/suppression.rs");

        let rule_sources = std::fs::read_dir(rules_dir)
            .expect("rule directory must be readable")
            .map(|entry| entry.expect("rule directory entry must be readable").path())
            .chain(std::iter::once(std::path::PathBuf::from(suppression)));

        for path in rule_sources {
            let source = std::fs::read_to_string(&path).expect("rule source must be readable");
            assert!(
                !source.contains(".sort"),
                "{} sorts its output; diagnostic ordering belongs to the reporting layer",
                path.display()
            );
        }
    }
}
