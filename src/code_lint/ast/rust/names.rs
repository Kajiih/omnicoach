//! What a Rust path or macro invocation names.

use super::trivia::previous_non_trivia_token;
use crate::code_lint::ast::imports::QualifiedName;
use ra_ap_syntax::ast;
use ra_ap_syntax::{SyntaxKind, SyntaxNode};

/// Keyword identifier `"self"` in Rust `use` trees and expressions.
pub(super) const SELF_KEYWORD: &str = "self";
/// Self-type keyword `"Self"` in Rust paths and `impl` blocks.
pub(super) const SELF_TYPE_KEYWORD: &str = "Self";
/// Keyword identifier `"super"` in Rust module paths.
pub(super) const SUPER_KEYWORD: &str = "super";

/// Extracts the terminal macro identifier from a [`ast::MacroCall`] node.
pub(super) fn macro_call_terminal_name(macro_call: &ast::MacroCall) -> String {
    macro_call
        .path()
        .and_then(|path| path.segment())
        .and_then(|segment| segment.name_ref())
        .map_or_else(String::new, |name_ref| name_ref.text().trim().to_string())
}

/// Resolves the macro path if `token_tree` is immediately preceded by `!` and a macro path
/// (such as `indoc! { ... }` or `indoc::indoc! { ... }` inside an outer `token_tree`).
pub(super) fn resolve_preceding_macro_path(token_tree: &SyntaxNode) -> Option<QualifiedName> {
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

/// Returns true if `kind` can be a segment of a Rust path (`ident`, `crate`, `self`, `super`).
pub(super) const fn is_path_segment_token_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IDENT | SyntaxKind::CRATE_KW | SyntaxKind::SELF_KW | SyntaxKind::SUPER_KW
    )
}

pub(super) fn compact_path_text(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}
