//! Positional element read extraction (`receiver[0]`, `receiver.0`) grouped by scope.

use super::tree::{AstNode, ParsedFile};
use std::collections::HashSet;

/// A read of one positional element through an integer literal: `receiver[1]` or
/// `receiver[-1]` in Python, `receiver.1` in Rust.
pub(in crate::code_lint) struct PositionalRead<'a> {
    /// The whole indexing expression (`point[0]`, `span.0`).
    pub(in crate::code_lint) node: AstNode<'a>,
    /// Source text of the indexed value (`point`, `self.pair`, `rows[i]`).
    pub(in crate::code_lint) receiver: String,
    /// The literal position; negative for Python end-relative indices.
    pub(in crate::code_lint) position: i64,
}

/// Positional reads of one scope (a function body, or the Python module top level), in source
/// order.
#[derive(Default)]
pub struct ScopePositionalReads<'a> {
    /// Literal-position reads whose receiver contains no call.
    pub(in crate::code_lint) reads: Vec<PositionalRead<'a>>,
    /// Receivers for which unpacking would not be equivalent. Python: receivers the scope writes
    /// to or uses as a collection (iterated, sized, indexed by a non-literal, sliced, mutated).
    /// Rust: receivers with a field assigned or mutably borrowed.
    pub(in crate::code_lint) non_unpackable_receivers: HashSet<String>,
}

/// Collects positional reads in `file` grouped by scope.
///
/// There is one group per function body, plus the Python module top level. Python lambdas and
/// class bodies, and Rust items outside functions, are not collected; Rust closures belong to
/// their enclosing function.
#[must_use]
pub fn collect_positional_reads(file: &ParsedFile) -> Vec<ScopePositionalReads<'_>> {
    dispatch_lang!(file.lang(), collect_positional_reads(file))
}
