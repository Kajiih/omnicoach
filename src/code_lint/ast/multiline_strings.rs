//! Unwrapped multiline string literal detection across languages.

use super::tree::{AstNode, ParsedFile, dispatch_lang};

/// Finds all multiline string literals in `file` that are not docstrings/doc-attributes/snapshots
/// and are not wrapped in a call or macro matching `is_wrapper`.
#[must_use]
pub fn find_unwrapped_multiline_strings(
    file: &ParsedFile,
    is_wrapper: impl Fn(&str) -> bool,
) -> Vec<AstNode<'_>> {
    dispatch_lang!(
        file.lang(),
        find_unwrapped_multiline_strings(file, is_wrapper)
    )
}
