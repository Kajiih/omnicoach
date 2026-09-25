//! Submodules containing implementations of code validation rules.
architecture_component!(CodeLintRules);

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
pub mod single_letter_variable_name;

/// Static list of all code linter rules.
pub const CODE_RULES: &[&dyn crate::code_lint::rule::CodeRule] = &[
    &no_unstructured_task_creation::NoUnstructuredTaskCreation,
    &no_sleep_in_tests::NoSleepInTests,
    &no_sleep_in_tests::NoZeroSleepInTests,
    &max_test_assertions::MaxTestAssertions,
    &no_assertion_packing::NoAssertionPacking,
    &no_mocks_in_tests::NoMocksInTests,
    &no_mock_assertions::NoMockAssertions,
    &no_logging_error_in_except::NoLoggingErrorInExcept,
    &no_uncommented_suppress::NoUncommentedSuppress,
    &no_typing_cast::NoTypingCast,
    &no_dynamic_attribute_access::NoDynamicAttributeAccess,
    &flat_scope_enforced::FlatScopeEnforced,
    &single_letter_variable_name::SingleLetterVariableName,
    &banned_abbreviations::BannedAbbreviations,
    &no_hungarian_notation::NoHungarianNotation,
    &prefer_timedelta_over_seconds::PreferTimedeltaOverSeconds,
    &no_identical_positional_types::NoIdenticalPositionalTypes,
    &no_env_in_functions::NoEnvInFunctions,
    &enforce_frozen_slots_dataclass::EnforceFrozenSlotsDataclass,
    &prefer_dedent_for_multiline_strings::PreferDedentForMultilineStrings,
    &crate::code_lint::suppression::MissingSuppressionReason,
    &crate::code_lint::suppression::UnusedSuppression,
    &crate::code_lint::suppression::UnknownSuppressionRule,
    &crate::code_lint::suppression::BlanketSuppression,
];
