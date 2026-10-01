//! Submodules containing implementations of code validation rules.
architecture_component!(CodeLintRules);

use crate::code_lint::rule::CodeDetector;
use crate::rule_declaration::Rule;

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

/// Static list of all code linter rules.
pub const CODE_RULES: &[Rule<dyn CodeDetector>] = &[
    no_unstructured_task_creation::RULE,
    no_sleep_in_tests::NO_SLEEP_IN_TESTS,
    no_sleep_in_tests::NO_ZERO_SLEEP_IN_TESTS,
    max_test_assertions::RULE,
    no_assertion_packing::RULE,
    no_mocks_in_tests::RULE,
    no_mock_assertions::RULE,
    no_logging_error_in_except::RULE,
    no_uncommented_suppress::RULE,
    no_typing_cast::RULE,
    no_dynamic_attribute_access::RULE,
    flat_scope_enforced::RULE,
    single_letter_variable_name::RULE,
    banned_abbreviations::RULE,
    no_hungarian_notation::RULE,
    prefer_timedelta_over_seconds::RULE,
    no_identical_positional_types::RULE,
    no_env_in_functions::RULE,
    enforce_frozen_slots_dataclass::RULE,
    prefer_dedent_for_multiline_strings::RULE,
    prefer_tuple_unpacking::RULE,
];
