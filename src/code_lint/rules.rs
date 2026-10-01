//! Submodules containing implementations of code validation rules.
architecture_component!(CodeLintRules);

use crate::code_lint::rule::CodeDetector;
use crate::rule_taxonomy::Rule;

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

/// Static list of all code linter rules, each registered with its classification and doc.
pub const CODE_RULES: &[Rule<dyn CodeDetector>] = &[
    Rule {
        detector: &no_unstructured_task_creation::NoUnstructuredTaskCreation,
        classification: no_unstructured_task_creation::NoUnstructuredTaskCreation::CLASSIFICATION,
        doc: no_unstructured_task_creation::NoUnstructuredTaskCreation::DOC,
    },
    Rule {
        detector: &no_sleep_in_tests::NoSleepInTests,
        classification: no_sleep_in_tests::NoSleepInTests::CLASSIFICATION,
        doc: no_sleep_in_tests::NoSleepInTests::DOC,
    },
    Rule {
        detector: &no_sleep_in_tests::NoZeroSleepInTests,
        classification: no_sleep_in_tests::NoZeroSleepInTests::CLASSIFICATION,
        doc: no_sleep_in_tests::NoZeroSleepInTests::DOC,
    },
    Rule {
        detector: &max_test_assertions::MaxTestAssertions,
        classification: max_test_assertions::MaxTestAssertions::CLASSIFICATION,
        doc: max_test_assertions::MaxTestAssertions::DOC,
    },
    Rule {
        detector: &no_assertion_packing::NoAssertionPacking,
        classification: no_assertion_packing::NoAssertionPacking::CLASSIFICATION,
        doc: no_assertion_packing::NoAssertionPacking::DOC,
    },
    Rule {
        detector: &no_mocks_in_tests::NoMocksInTests,
        classification: no_mocks_in_tests::NoMocksInTests::CLASSIFICATION,
        doc: no_mocks_in_tests::NoMocksInTests::DOC,
    },
    Rule {
        detector: &no_mock_assertions::NoMockAssertions,
        classification: no_mock_assertions::NoMockAssertions::CLASSIFICATION,
        doc: no_mock_assertions::NoMockAssertions::DOC,
    },
    Rule {
        detector: &no_logging_error_in_except::NoLoggingErrorInExcept,
        classification: no_logging_error_in_except::NoLoggingErrorInExcept::CLASSIFICATION,
        doc: no_logging_error_in_except::NoLoggingErrorInExcept::DOC,
    },
    Rule {
        detector: &no_uncommented_suppress::NoUncommentedSuppress,
        classification: no_uncommented_suppress::NoUncommentedSuppress::CLASSIFICATION,
        doc: no_uncommented_suppress::NoUncommentedSuppress::DOC,
    },
    Rule {
        detector: &no_typing_cast::NoTypingCast,
        classification: no_typing_cast::NoTypingCast::CLASSIFICATION,
        doc: no_typing_cast::NoTypingCast::DOC,
    },
    Rule {
        detector: &no_dynamic_attribute_access::NoDynamicAttributeAccess,
        classification: no_dynamic_attribute_access::NoDynamicAttributeAccess::CLASSIFICATION,
        doc: no_dynamic_attribute_access::NoDynamicAttributeAccess::DOC,
    },
    Rule {
        detector: &flat_scope_enforced::FlatScopeEnforced,
        classification: flat_scope_enforced::FlatScopeEnforced::CLASSIFICATION,
        doc: flat_scope_enforced::FlatScopeEnforced::DOC,
    },
    Rule {
        detector: &single_letter_variable_name::SingleLetterVariableName,
        classification: single_letter_variable_name::SingleLetterVariableName::CLASSIFICATION,
        doc: single_letter_variable_name::SingleLetterVariableName::DOC,
    },
    Rule {
        detector: &banned_abbreviations::BannedAbbreviations,
        classification: banned_abbreviations::BannedAbbreviations::CLASSIFICATION,
        doc: banned_abbreviations::BannedAbbreviations::DOC,
    },
    Rule {
        detector: &no_hungarian_notation::NoHungarianNotation,
        classification: no_hungarian_notation::NoHungarianNotation::CLASSIFICATION,
        doc: no_hungarian_notation::NoHungarianNotation::DOC,
    },
    Rule {
        detector: &prefer_timedelta_over_seconds::PreferTimedeltaOverSeconds,
        classification: prefer_timedelta_over_seconds::PreferTimedeltaOverSeconds::CLASSIFICATION,
        doc: prefer_timedelta_over_seconds::PreferTimedeltaOverSeconds::DOC,
    },
    Rule {
        detector: &no_identical_positional_types::NoIdenticalPositionalTypes,
        classification: no_identical_positional_types::NoIdenticalPositionalTypes::CLASSIFICATION,
        doc: no_identical_positional_types::NoIdenticalPositionalTypes::DOC,
    },
    Rule {
        detector: &no_env_in_functions::NoEnvInFunctions,
        classification: no_env_in_functions::NoEnvInFunctions::CLASSIFICATION,
        doc: no_env_in_functions::NoEnvInFunctions::DOC,
    },
    Rule {
        detector: &enforce_frozen_slots_dataclass::EnforceFrozenSlotsDataclass,
        classification: enforce_frozen_slots_dataclass::EnforceFrozenSlotsDataclass::CLASSIFICATION,
        doc: enforce_frozen_slots_dataclass::EnforceFrozenSlotsDataclass::DOC,
    },
    Rule {
        detector: &prefer_dedent_for_multiline_strings::PreferDedentForMultilineStrings,
        classification:
            prefer_dedent_for_multiline_strings::PreferDedentForMultilineStrings::CLASSIFICATION,
        doc: prefer_dedent_for_multiline_strings::PreferDedentForMultilineStrings::DOC,
    },
    Rule {
        detector: &prefer_tuple_unpacking::PreferTupleUnpacking,
        classification: prefer_tuple_unpacking::PreferTupleUnpacking::CLASSIFICATION,
        doc: prefer_tuple_unpacking::PreferTupleUnpacking::DOC,
    },
];
