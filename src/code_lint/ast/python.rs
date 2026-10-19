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

pub use self::annotations::{
    AnnotationTraversalDepth, CollectionKind, CollectionShape, PythonCollectionType,
    PythonReturnTypeBranch, PythonReturnTypeUnion, collect_collection_types, collection_display,
    collection_type, generator_yield_type, includes_none_bool_or_callable, return_type_union,
};
pub(super) use self::bindings::collect_bindings;
pub(super) use self::calls::collect_call_candidates;
pub use self::calls::{PythonEnvironSubscript, call_callee, collect_environ_subscripts};
pub use self::classes::{
    PythonAnnotatedAttribute, PythonAttributeDefinition, PythonBaseClass, PythonClassInfo,
    PythonInheritedDocstring, PythonInstanceAttributeAnnotation, enclosing_class, extract_classes,
    inherited_method_docstring, is_interface_method, local_ancestors, local_base_classes,
};
pub use self::complexity::PythonComplexityMetric;
pub use self::declaration_order::{
    PythonStatementAfterMainGuard, collect_statements_after_main_guard,
};
pub(super) use self::declaration_order::{
    collect_declaration_scopes, collect_members_after_methods,
};
pub use self::docstrings::{
    DocstringEntry, DocstringLine, DocstringSection, DocstringSectionKind, DocstringTypedLine,
    PythonDocstring, is_clause_end, is_clause_start, leading_verb, none_words,
    normalize_docstring_text, nullability_markers, parse_google_sections, without_byte_ranges,
};
pub use self::format_strings::{
    PythonFormatPlaceholder, PythonFormatString, PythonFormatStyle, collect_format_strings,
    extract_valid_field_root, named_format_field_roots,
};
pub use self::functions::{
    PythonFunctionSignature, PythonParameterInfo, PythonParameterKind, extract_function_signatures,
    find_nested_classes, find_nested_functions,
};
pub(super) use self::literals::collect_literal_occurrences;
pub use self::literals::{PythonModuleAssignment, collect_module_assignments};
pub use self::logging::{PythonLoggerCall, collect_logger_calls};
pub(super) use self::multiline_strings::find_unwrapped_multiline_strings;
pub use self::parameter_usage::{
    ParameterCollectionCapability, ParameterUsage, summarize_parameter_usages,
};
pub(super) use self::positional_reads::collect_positional_reads;
pub(super) use self::returns::collect_function_returns;
pub use self::returns::{PythonBodyResults, collect_locally_mutated_return_functions};
pub(super) use self::statement_spans::{
    enclosing_statement_header_span, record_decorator_and_attribute_spans,
};
pub(super) use self::test_analysis::{
    collect_assertion_checks, collect_multi_phase_test_functions,
    collect_test_function_assertion_counts, collect_test_logic_constructs,
    collect_test_setup_scopes, collect_test_to_test_calls,
};
pub(super) use self::trivia::collect_comment_spans;
