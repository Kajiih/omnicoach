//! AST helper predicates and structural extractors for Rust (`ra_ap_syntax`).

mod bindings;
mod calls;
mod declaration_order;
mod file_summary;
mod functions;
mod literals;
mod positional_reads;
mod strings;
mod test_analysis;

pub(super) use self::bindings::collect_bindings;
pub(super) use self::calls::collect_call_candidates;
pub use self::declaration_order::{
    RustAssociatedItemAfterMethod, collect_associated_items_after_methods,
};
pub(super) use self::declaration_order::{collect_declaration_scopes, collect_type_method_scopes};
pub use self::file_summary::{
    ExternalModDeclaration, RustFileSummary, RustPathReference, VisibleUseDeclaration,
    summarize_rust_file,
};
pub(super) use self::functions::collect_function_returns;
pub use self::functions::{
    NullableReturnPayload, RustFunction, collect_functions, nullable_return_payload,
};
pub(super) use self::literals::collect_literal_occurrences;
pub(super) use self::positional_reads::collect_positional_reads;
pub(super) use self::strings::find_unwrapped_multiline_strings;
pub use self::test_analysis::collect_inline_test_ranges;
use self::test_analysis::extract_macro_arguments;
pub(super) use self::test_analysis::{
    collect_assertion_checks, collect_multi_phase_test_functions,
    collect_test_function_assertion_counts, collect_test_logic_constructs,
    collect_test_setup_scopes, collect_test_to_test_calls,
};

use crate::code_lint::ast::{
    AstNode, QualifiedName, ResolvedName, is_assertion_function_name, resolve_name,
};
use ra_ap_syntax::ast;
use ra_ap_syntax::{AstNode as _, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};

/// Keyword identifier `"self"` in Rust `use` trees and expressions.
const SELF_KEYWORD: &str = "self";
/// Self-type keyword `"Self"` in Rust paths and `impl` blocks.
const SELF_TYPE_KEYWORD: &str = "Self";
/// Keyword identifier `"super"` in Rust module paths.
const SUPER_KEYWORD: &str = "super";
/// Identifier `"test"` used in `#[test]`, `#[cfg(test)]`, and `test_*` functions.
const TEST_IDENTIFIER: &str = "test";

/// Returns true if `expr` contains a function call, method call, or macro invocation.
pub(super) fn expr_contains_call(expr: &ast::Expr) -> bool {
    expr.syntax().descendants().any(|node| {
        matches!(
            node.kind(),
            SyntaxKind::CALL_EXPR | SyntaxKind::METHOD_CALL_EXPR | SyntaxKind::MACRO_EXPR
        )
    })
}

/// Casts the syntax node at `node.span()` (or an exact-range wrapper ancestor) to `T`.
fn cast_at_span<T: ra_ap_syntax::AstNode>(node: &AstNode<'_>) -> Option<T> {
    let inner = syntax_node(node)?;
    let target_range = inner.text_range();
    inner
        .ancestors()
        .take_while(|ancestor| ancestor.text_range() == target_range)
        .find_map(T::cast)
}

/// Resolves the deepest [`SyntaxNode`] in `node`'s file covering `node.span()`.
fn syntax_node(node: &AstNode<'_>) -> Option<SyntaxNode> {
    let parsed = node.file.rs_parsed()?;
    let span = node.span();
    let start_offset = u32::try_from(span.start).ok()?;
    let end_offset = u32::try_from(span.end).ok()?;
    let range = ra_ap_syntax::TextRange::new(start_offset.into(), end_offset.into());
    match parsed.tree().syntax().covering_element(range) {
        SyntaxElement::Node(found) => Some(found),
        SyntaxElement::Token(token) => token.parent(),
    }
}

/// Extracts the terminal identifier of an attribute's path (e.g. `"test"` for `#[tokio::test]`).
fn attribute_terminal_name(attribute: &ast::Attr) -> Option<ast::NameRef> {
    let path = attribute
        .path()
        .or_else(|| attribute.syntax().descendants().find_map(ast::Path::cast))?;
    path.segment()?.name_ref()
}

/// Extracts the terminal macro identifier from a [`ast::MacroCall`] node.
fn macro_call_terminal_name(macro_call: &ast::MacroCall) -> String {
    macro_call
        .path()
        .and_then(|path| path.segment())
        .and_then(|segment| segment.name_ref())
        .map_or_else(String::new, |name_ref| name_ref.text().trim().to_string())
}

/// Resolves the macro path if `token_tree` is immediately preceded by `!` and a macro path
/// (such as `indoc! { ... }` or `indoc::indoc! { ... }` inside an outer `token_tree`).
fn resolve_preceding_macro_path(token_tree: &SyntaxNode) -> Option<QualifiedName> {
    let first_token = token_tree.first_token()?;
    let bang = previous_non_trivia_token(&first_token)?;
    if bang.kind() != SyntaxKind::BANG {
        return None;
    }
    let macro_ident = previous_non_trivia_token(&bang)?;
    if !is_path_segment_token_kind(macro_ident.kind()) {
        return None;
    }
    let terminal = macro_ident.text().trim().to_string();
    let mut full_path = terminal.clone();
    let mut current = macro_ident;
    while let Some(preceding) = previous_non_trivia_token(&current) {
        let before_colons = if preceding.kind() == SyntaxKind::COLON2 {
            previous_non_trivia_token(&preceding)
        } else if preceding.kind() == SyntaxKind::COLON
            && let Some(first_colon) = previous_non_trivia_token(&preceding)
            && first_colon.kind() == SyntaxKind::COLON
        {
            previous_non_trivia_token(&first_colon)
        } else {
            break;
        };
        if let Some(prev_ident) = before_colons
            && is_path_segment_token_kind(prev_ident.kind())
        {
            full_path = format!("{prefix}::{full_path}", prefix = prev_ident.text().trim());
            current = prev_ident;
        } else {
            break;
        }
    }
    Some(QualifiedName {
        path: full_path,
        terminal,
    })
}

/// Returns the previous non-trivia token before `token`.
fn previous_non_trivia_token(token: &SyntaxToken) -> Option<SyntaxToken> {
    std::iter::successors(token.prev_token(), SyntaxToken::prev_token)
        .find(|candidate| !candidate.kind().is_trivia())
}

/// Returns true if `kind` can be a segment of a Rust path (`ident`, `crate`, `self`, `super`).
const fn is_path_segment_token_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IDENT | SyntaxKind::CRATE_KW | SyntaxKind::SELF_KW | SyntaxKind::SUPER_KW
    )
}

fn compact_path_text(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}
