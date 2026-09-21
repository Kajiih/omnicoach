//! Submodules containing implementations of code validation rules.
pub mod banned_abbreviations;
pub mod flat_scope_enforced;
pub mod max_test_assertions;
pub mod no_assertion_packing;
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
pub mod prefer_timedelta_over_seconds;
pub mod single_letter_variable_name;
