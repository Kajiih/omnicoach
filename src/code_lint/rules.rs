//! Submodules containing implementations of code validation rules.
architecture_component!(CodeLintRules);

use crate::code_lint::rule::CodeRule;
use crate::rule_taxonomy::ClassifiedRule;

pub mod banned_abbreviations;
pub mod enforce_frozen_slots_dataclass;
pub mod flat_scope_enforced;
pub mod max_test_assertions;
pub mod no_assertion_packing;
pub mod no_dynamic_attribute_access;
pub mod no_env_in_functions;
pub mod no_hungarian_notation;
pub mod no_identical_positional_types;
pub mod no_logging_error_in_except;
pub mod no_mock_assertions;
pub mod no_mocks_in_tests;
pub mod no_sleep_in_tests;
pub mod no_typing_cast;
pub mod no_uncommented_suppress;
pub mod no_unstructured_task_creation;
pub mod prefer_dedent_for_multiline_strings;
pub mod prefer_timedelta_over_seconds;
pub mod prefer_tuple_unpacking;
pub mod single_letter_variable_name;

/// Static list of all code linter rules, each registered with its classification.
pub const CODE_RULES: &[ClassifiedRule<dyn CodeRule>] = &[
    ClassifiedRule {
        rule: &no_unstructured_task_creation::NoUnstructuredTaskCreation,
        classification: no_unstructured_task_creation::NoUnstructuredTaskCreation::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &no_sleep_in_tests::NoSleepInTests,
        classification: no_sleep_in_tests::NoSleepInTests::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &no_sleep_in_tests::NoZeroSleepInTests,
        classification: no_sleep_in_tests::NoZeroSleepInTests::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &max_test_assertions::MaxTestAssertions,
        classification: max_test_assertions::MaxTestAssertions::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &no_assertion_packing::NoAssertionPacking,
        classification: no_assertion_packing::NoAssertionPacking::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &no_mocks_in_tests::NoMocksInTests,
        classification: no_mocks_in_tests::NoMocksInTests::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &no_mock_assertions::NoMockAssertions,
        classification: no_mock_assertions::NoMockAssertions::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &no_logging_error_in_except::NoLoggingErrorInExcept,
        classification: no_logging_error_in_except::NoLoggingErrorInExcept::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &no_uncommented_suppress::NoUncommentedSuppress,
        classification: no_uncommented_suppress::NoUncommentedSuppress::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &no_typing_cast::NoTypingCast,
        classification: no_typing_cast::NoTypingCast::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &no_dynamic_attribute_access::NoDynamicAttributeAccess,
        classification: no_dynamic_attribute_access::NoDynamicAttributeAccess::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &flat_scope_enforced::FlatScopeEnforced,
        classification: flat_scope_enforced::FlatScopeEnforced::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &single_letter_variable_name::SingleLetterVariableName,
        classification: single_letter_variable_name::SingleLetterVariableName::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &banned_abbreviations::BannedAbbreviations,
        classification: banned_abbreviations::BannedAbbreviations::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &no_hungarian_notation::NoHungarianNotation,
        classification: no_hungarian_notation::NoHungarianNotation::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &prefer_timedelta_over_seconds::PreferTimedeltaOverSeconds,
        classification: prefer_timedelta_over_seconds::PreferTimedeltaOverSeconds::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &no_identical_positional_types::NoIdenticalPositionalTypes,
        classification: no_identical_positional_types::NoIdenticalPositionalTypes::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &no_env_in_functions::NoEnvInFunctions,
        classification: no_env_in_functions::NoEnvInFunctions::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &enforce_frozen_slots_dataclass::EnforceFrozenSlotsDataclass,
        classification: enforce_frozen_slots_dataclass::EnforceFrozenSlotsDataclass::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &prefer_dedent_for_multiline_strings::PreferDedentForMultilineStrings,
        classification:
            prefer_dedent_for_multiline_strings::PreferDedentForMultilineStrings::CLASSIFICATION,
    },
    ClassifiedRule {
        rule: &prefer_tuple_unpacking::PreferTupleUnpacking,
        classification: prefer_tuple_unpacking::PreferTupleUnpacking::CLASSIFICATION,
    },
];
