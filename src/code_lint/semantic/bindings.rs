//! Shared identifier binding extraction, classification, and suffix-matching engine.
//!
//! Used to inspect locally authored bindings across supported languages while exempting external imports and trait/override contracts.

use crate::code_lint::ast::{self, AstNode, BindingKind, ParsedFile};
use std::collections::HashSet;

/// Collects all binding nodes in `file` whose identifier names are locally authored and
/// eligible for renaming (excluding imports and trait/override contract members).
#[must_use]
pub fn collect_renameable_bindings(file: &ParsedFile) -> Vec<AstNode<'_>> {
    ast::collect_bindings(file)
        .into_iter()
        .filter(|binding| {
            !matches!(
                binding.kind,
                BindingKind::Import | BindingKind::ContractMember
            )
        })
        .map(|binding| binding.node)
        .collect()
}

/// A variable, constant, parameter, or attribute/field binding whose identifier ends with a
/// matched suffix.
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

/// Finds all value bindings (variables, constants, parameters, declared Python attributes, and
/// Rust struct fields) in `file` whose name ends (case-insensitively) with any suffix in
/// `banned_suffixes`.
///
/// Only [`BindingKind::Value`] bindings are considered: imports, structural definitions
/// (functions, classes, structs, enums, traits, type aliases), and trait/override contract names
/// are skipped. Suffixes are evaluated longest-first for deterministic matching.
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
    let mut matches = Vec::new();

    for binding in bindings {
        if binding.kind != BindingKind::Value {
            continue;
        }
        let node = binding.node;
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
