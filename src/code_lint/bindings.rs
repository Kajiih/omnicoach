//! Shared identifier binding extraction, classification, and suffix-matching engine.
//!
//! Used to inspect locally authored bindings across supported languages while exempting external imports and trait/override contracts.

architecture_component!(CodeSemanticEngines);

use crate::code_lint::ast::{
    self, AstNode, ParsedFile, is_import_binding, is_structural_definition, is_trait_impl_member,
    is_unaliased_import_binding,
};
use std::collections::HashSet;

/// Collects all binding nodes in `file` whose identifier names are locally authored and
/// eligible for renaming (excluding unaliased imports and trait/override contract members).
#[must_use]
pub fn collect_renameable_bindings(file: &ParsedFile) -> Vec<AstNode<'_>> {
    let lang = file.lang();
    ast::collect_bindings(file)
        .into_iter()
        .filter(|node| {
            !is_unaliased_import_binding(node, lang) && !is_trait_impl_member(node, lang)
        })
        .collect()
}

/// A variable, constant, or parameter binding whose identifier ends with a matched suffix.
pub struct SuffixedBindingMatch<'a> {
    /// The matched identifier AST node.
    pub node: AstNode<'a>,
    /// Full identifier text (e.g. `timeout_seconds` or `user_list`).
    pub name: String,
    /// Matched suffix slice preserving the identifier's original case (e.g. `_seconds` or `_INT`).
    pub actual_suffix: String,
    /// Identifier stem preceding the matched suffix (e.g. `timeout` or `MY`).
    pub base_name: String,
}

/// Finds all variable, constant, and parameter bindings in `file` whose name ends
/// (case-insensitively) with any suffix in `banned_suffixes`.
///
/// Automatically skips imports ([`is_import_binding`]), structural definitions
/// ([`is_structural_definition`]: functions, classes, structs, enums, traits), and
/// trait/override contract names ([`is_trait_impl_member`]).
/// Suffixes are evaluated longest-first for deterministic matching.
#[must_use]
pub fn find_suffixed_bindings<'a, S: std::hash::BuildHasher>(
    file: &'a ParsedFile,
    banned_suffixes: &HashSet<String, S>,
) -> Vec<SuffixedBindingMatch<'a>> {
    if banned_suffixes.is_empty() {
        return Vec::new();
    }

    let mut sorted_suffixes: Vec<String> = banned_suffixes
        .iter()
        .map(|suffix| suffix.to_lowercase())
        .collect();
    // Sort descending by length, then ascending lexicographically for deterministic tie-breaking
    sorted_suffixes
        .sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));

    let bindings = ast::collect_bindings(file);
    let lang = file.lang();
    let mut matches = Vec::new();

    for node in bindings {
        if is_import_binding(&node, lang)
            || is_structural_definition(&node, lang)
            || is_trait_impl_member(&node, lang)
        {
            continue;
        }

        let name = node.text().into_owned();
        let name_lower = name.to_lowercase();

        for suffix_lower in &sorted_suffixes {
            if name.len() > suffix_lower.len() && name_lower.ends_with(suffix_lower.as_str()) {
                let split_idx = name.len() - suffix_lower.len();
                let Some((base_name, actual_suffix)) = name.split_at_checked(split_idx) else {
                    continue;
                };

                matches.push(SuffixedBindingMatch {
                    actual_suffix: actual_suffix.to_string(),
                    base_name: base_name.to_string(),
                    name: name.clone(),
                    node,
                });
                break;
            }
        }
    }

    matches
}
