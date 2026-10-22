//! Dedicated AST and CST parser encapsulation.
//!
//! Encapsulates `ruff_python_parser` / `ruff_python_ast` (Python) and `ra_ap_syntax` (Rust) behind
//! [`ParsedFile`] and an opaque [`AstNode`], and provides language-dispatched and language-specific
//! syntax extractors. No module outside `crate::code_lint::ast` imports parser crates directly.

architecture_component!(CodeLintAst);

/// Dispatches `$func(args...)` to `ast::python` or `ast::rust` by `$lang`.
macro_rules! dispatch_lang {
    ($lang:expr, $func:ident ( $($arg:expr),* $(,)? )) => {
        match $lang {
            $crate::diagnostic::Language::Python => {
                $crate::code_lint::ast::python::$func($($arg),*)
            }
            $crate::diagnostic::Language::Rust => {
                $crate::code_lint::ast::rust::$func($($arg),*)
            }
        }
    };
}

mod bindings;
mod call_clusters;
mod calls;
mod imports;
mod literals;
mod multiline_strings;
mod positional_reads;
pub mod python;
mod returns;
pub mod rust;
mod scope_facts;
mod statement_spans;
mod test_analysis;
mod tree;
mod trivia;

pub(in crate::code_lint) use self::bindings::BindingKind;
pub use self::bindings::{Binding, collect_bindings};
pub(in crate::code_lint) use self::call_clusters::{
    CallClusterRule, DeclarationScopeKind, OrderingReason, RelationKind, ScopeItemKind, Visibility,
    collect_call_cluster_findings, levels, strongly_connected_components,
};
#[cfg(test)]
pub(in crate::code_lint) use self::call_clusters::{OrderingEdge, PlacementAnchor, RelationFact};
pub(in crate::code_lint) use self::calls::EnclosingFunction;
pub use self::calls::{AstCallCandidate, CallSiteContext, collect_call_candidates};
pub use self::imports::{ResolvedName, resolve_name};
pub(in crate::code_lint) use self::literals::LiteralRole;
pub(crate) use self::literals::LiteralValue;
pub use self::literals::{LiteralOccurrence, collect_literal_occurrences};
pub(in crate::code_lint) use self::multiline_strings::find_unwrapped_multiline_strings;
pub use self::positional_reads::{ScopePositionalReads, collect_positional_reads};
pub(in crate::code_lint) use self::returns::{
    ReturnShape, collect_function_returns, is_predicate_name,
};
pub(in crate::code_lint) use self::scope_facts::{
    FindingFact, ItemFacts, ScopeFacts, collect_scope_facts,
};
pub(in crate::code_lint) use self::statement_spans::{
    decorator_and_attribute_end_lines, enclosing_statement_header_range,
};
pub(in crate::code_lint) use self::test_analysis::{
    AssertionCheck, TestFunctionSetup, collect_assertion_checks,
    collect_multi_phase_test_functions, collect_test_function_assertion_counts,
    collect_test_logic_constructs, collect_test_setup_scopes, collect_test_to_test_calls,
};
pub use self::tree::{AstNode, ParsedFile};
pub use self::trivia::collect_comment_nodes;
