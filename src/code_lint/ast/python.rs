//! AST helper predicates and structural extractors for Python.

mod annotations;
mod bindings;
mod calls;
mod classes;
mod complexity;
mod declaration_order;
mod decorators;
mod docstrings;
mod effects;
mod format_strings;
mod functions;
mod imports;
mod literals;
mod logging;
mod multiline_strings;
mod names;
mod parameter_usage;
mod positional_reads;
mod returns;
mod statement_spans;
mod test_analysis;
mod tree;
mod trivia;

pub(in crate::code_lint) use self::annotations::{
    AnnotationTraversalDepth, CollectionKind, CollectionShape, PythonCollectionType,
    PythonReturnTypeBranch, collect_collection_types, collection_display, collection_type,
    generator_yield_type, includes_none_bool_or_callable, return_type_union,
};
pub(super) use self::bindings::collect_bindings;
pub(in crate::code_lint) use self::bindings::{PythonModuleAssignment, collect_module_assignments};
pub(super) use self::calls::collect_call_candidates;
pub(in crate::code_lint) use self::calls::{call_callee, collect_environ_subscripts};
pub use self::classes::{
    PythonAnnotatedAttribute, PythonBaseClass, PythonClassInfo, extract_classes,
};
pub(in crate::code_lint) use self::classes::{
    PythonAttributeDefinition, PythonInheritedDocstring, enclosing_class,
    inherited_method_docstring, is_documented_by_enclosing_class, is_interface_method,
    local_ancestors,
};
pub(super) use self::declaration_order::collect_declaration_scopes;
pub(in crate::code_lint) use self::declaration_order::{
    collect_fields_after_methods, collect_statements_after_main_guard,
};
pub(in crate::code_lint) use self::docstrings::{
    DocstringEntry, DocstringLine, DocstringSection, DocstringSectionKind, PythonDocstring,
    is_clause_end, is_clause_start, leading_verb, none_words, normalize_docstring_text,
    nullability_markers, without_byte_ranges,
};
pub(in crate::code_lint) use self::format_strings::{
    PythonFormatPlaceholder, PythonFormatString, PythonFormatStyle, collect_format_strings,
    extract_valid_field_root, named_format_field_roots,
};
pub use self::functions::{
    PythonFunctionSignature, PythonParameterInfo, extract_function_signatures,
};
pub(in crate::code_lint) use self::functions::{
    PythonParameterKind, find_nested_classes, find_nested_functions,
};
pub(super) use self::imports::collect_import_map;
pub(super) use self::literals::collect_literal_occurrences;
pub(in crate::code_lint) use self::logging::collect_logger_calls;
pub(super) use self::multiline_strings::collect_multiline_strings;
pub(in crate::code_lint) use self::names::collect_path_references;
pub(in crate::code_lint) use self::parameter_usage::ParameterCollectionCapability;
pub use self::parameter_usage::{ParameterUsage, summarize_parameter_usages};
pub(super) use self::positional_reads::collect_positional_reads;
pub(super) use self::returns::collect_function_returns;
pub use self::returns::collect_locally_mutated_return_functions;
pub(super) use self::statement_spans::{
    enclosing_statement_header_span, record_decorator_and_attribute_spans,
};
pub(super) use self::test_analysis::{
    collect_assertion_checks, collect_multi_phase_test_functions,
    collect_test_function_assertion_counts, collect_test_logic_constructs,
    collect_test_setup_scopes, collect_test_to_test_calls,
};
pub(super) use self::trivia::collect_comment_spans;
