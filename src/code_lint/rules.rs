//! The code rule registry.
architecture_component!(CodeLintRules);

use crate::code_lint::contract::AnyCodeRule;

pub mod abbreviated_name;
pub mod bare_multiline_string;
pub mod concrete_collection_attribute;
pub mod concrete_collection_parameter;
pub mod concrete_collection_return;
pub mod dynamic_attribute_access;
pub mod environment_variable_in_function;
pub mod error_log_in_except;
pub mod identical_positional_types;
pub mod mock_call_assertion;
pub mod mock_in_tests;
pub mod mutable_collection_attribute;
pub mod mutable_collection_parameter;
pub mod mutable_collection_return;
pub mod mutable_dataclass;
pub mod mutable_module_constant;
pub mod nested_function;
pub mod packed_assertion;
pub mod primitive_duration;
pub mod repeated_index_access;
pub mod repeated_literal;
pub mod single_letter_name;
pub mod sleep_in_tests;
pub mod specific_collection_parameter;
pub mod suppressed_exception;
pub mod too_many_assertions;
pub mod type_cast;
pub mod type_suffixed_name;
pub mod unslotted_dataclass;
pub mod unstructured_task;

/// Every registered code rule.
pub const CODE_RULES: &[&dyn AnyCodeRule] = &[
    &unstructured_task::RULE,
    &sleep_in_tests::SLEEP_IN_TESTS,
    &sleep_in_tests::ZERO_SLEEP_IN_TESTS,
    &too_many_assertions::RULE,
    &packed_assertion::RULE,
    &mock_in_tests::RULE,
    &mock_call_assertion::RULE,
    &error_log_in_except::RULE,
    &suppressed_exception::RULE,
    &type_cast::RULE,
    &dynamic_attribute_access::RULE,
    &nested_function::RULE,
    &single_letter_name::RULE,
    &abbreviated_name::RULE,
    &type_suffixed_name::RULE,
    &primitive_duration::RULE,
    &identical_positional_types::RULE,
    &environment_variable_in_function::RULE,
    &mutable_dataclass::RULE,
    &unslotted_dataclass::RULE,
    &bare_multiline_string::RULE,
    &repeated_index_access::RULE,
    &concrete_collection_parameter::RULE,
    &concrete_collection_return::RULE,
    &concrete_collection_attribute::RULE,
    &mutable_collection_parameter::RULE,
    &mutable_collection_return::RULE,
    &mutable_collection_attribute::RULE,
    &specific_collection_parameter::RULE,
    &repeated_literal::RULE,
    &mutable_module_constant::RULE,
];
