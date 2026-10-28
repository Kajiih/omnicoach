//! AST helper predicates and structural extractors for Rust (`ra_ap_syntax`).

mod annotations;
mod attributes;
mod bindings;
mod calls;
mod declaration_order;
mod effects;
mod file_summary;
mod imports;
mod literals;
mod multiline_strings;
mod names;
mod positional_reads;
mod returns;
mod statement_spans;
mod test_analysis;
mod tree;
mod trivia;

pub(super) use self::annotations::collect_parameter_annotations;
pub(super) use self::bindings::collect_bindings;
pub(super) use self::calls::collect_call_candidates;
pub(in crate::code_lint) use self::declaration_order::collect_associated_items_after_methods;
pub(super) use self::declaration_order::collect_declaration_scopes;
pub use self::file_summary::{
    ExternalModDeclaration, RustFileSummary, RustPathReference, VisibleUseDeclaration,
    summarize_rust_file,
};
pub(super) use self::imports::collect_import_map;
pub(super) use self::literals::collect_literal_occurrences;
pub(super) use self::multiline_strings::collect_multiline_strings;
pub(super) use self::positional_reads::collect_positional_reads;
pub(super) use self::returns::collect_function_returns;
pub(in crate::code_lint) use self::returns::{NullableReturnPayload, nullable_return_payload};
pub(super) use self::statement_spans::{
    enclosing_statement_header_span, record_decorator_and_attribute_spans,
};
pub use self::test_analysis::collect_inline_test_ranges;
pub(super) use self::test_analysis::{
    collect_assertion_checks, collect_multi_phase_test_functions,
    collect_test_function_assertion_counts, collect_test_logic_constructs,
    collect_test_setup_scopes, collect_test_to_test_calls,
};
pub(super) use self::trivia::collect_comment_spans;
pub(in crate::code_lint) use self::trivia::is_rust_doc_comment;
