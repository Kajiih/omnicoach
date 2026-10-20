//! Dedicated AST and CST parser encapsulation.
//!
//! Encapsulates `ruff_python_parser` / `ruff_python_ast` (Python) and `ra_ap_syntax` (Rust) behind
//! [`ParsedFile`] and an opaque [`AstNode`], and provides language-dispatched and language-specific
//! syntax extractors. No module outside `crate::code_lint::ast` imports parser crates directly.

architecture_component!(CodeLintAst);

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

pub use self::bindings::{Binding, BindingKind, collect_bindings};
pub use self::call_clusters::{
    CallClusterRule, CallOrderFinding, DeclarationScopeKind, OrderingEdge, OrderingReason,
    PlacementAnchor, RelationFact, RelationKind, ScopeItemKind, Visibility,
    collect_call_cluster_findings, levels, strongly_connected_components,
};
pub use self::calls::{
    AstCallCandidate, CallSiteContext, EnclosingFunction, collect_call_candidates,
};
pub use self::imports::{ResolvedName, resolve_name};
pub use self::literals::{
    LiteralOccurrence, LiteralRole, LiteralValue, collect_literal_occurrences,
};
pub use self::multiline_strings::find_unwrapped_multiline_strings;
pub use self::positional_reads::{PositionalRead, ScopePositionalReads, collect_positional_reads};
pub use self::returns::{FunctionReturn, ReturnShape, collect_function_returns, is_predicate_name};
pub use self::scope_facts::{FindingFact, ItemFacts, ScopeFacts, collect_scope_facts};
pub use self::statement_spans::{
    decorator_and_attribute_end_lines, enclosing_statement_header_range,
};
pub use self::test_analysis::{
    AssertionCheck, MultiPhaseTest, TestAssertionCount, TestFunctionSetup, TestLogicConstruct,
    TestToTestCall, collect_assertion_checks, collect_multi_phase_test_functions,
    collect_test_function_assertion_counts, collect_test_logic_constructs,
    collect_test_setup_scopes, collect_test_to_test_calls,
};
pub use self::tree::{AstNode, ParsedFile};
pub use self::trivia::collect_comment_nodes;
