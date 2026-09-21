//! Shared identifier binding extraction, classification, and suffix-matching engine.
//!
//! Used to inspect locally authored bindings across supported languages while exempting external imports and trait/override contracts.

use crate::code_lint::{AstNode, CodeRule, SourceDoc, ast_python, ast_rust};
use crate::core::{Config, FilterListDefaults};
use crate::diagnostic::Diagnostic;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::collections::HashSet;
use std::path::Path;

/// Collects all binding definition nodes from a parsed AST grep document.
#[must_use]
pub fn collect_bindings(grep: &AstGrep<SourceDoc>) -> Vec<AstNode<'_>> {
    match grep.lang() {
        SupportLang::Rust => ast_rust::collect_bindings(&grep.root()),
        SupportLang::Python => ast_python::collect_bindings(&grep.root()),
        _ => Vec::new(),
    }
}

/// Returns true if the node represents an import binding.
#[must_use]
pub fn is_import_binding(node: &AstNode<'_>, lang: SupportLang) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let parent_kind = parent.kind();
    match lang {
        SupportLang::Rust => ast_rust::is_import_binding_parent(parent_kind.as_ref()),
        SupportLang::Python => ast_python::is_import_binding_parent(parent_kind.as_ref()),
        _ => false,
    }
}

/// Returns true if the node represents an unaliased import binding (an external symbol
/// imported directly without a local `as` alias).
#[must_use]
pub fn is_unaliased_import_binding(node: &AstNode<'_>, lang: SupportLang) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let parent_kind = parent.kind();
    match lang {
        SupportLang::Rust => ast_rust::is_unaliased_import_binding_parent(parent_kind.as_ref()),
        SupportLang::Python => ast_python::is_unaliased_import_binding_parent(parent_kind.as_ref()),
        _ => false,
    }
}

/// Returns true if the node represents a structural type, class, or function definition name.
#[must_use]
pub fn is_structural_definition(node: &AstNode<'_>, lang: SupportLang) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let parent_kind = parent.kind();
    match lang {
        SupportLang::Rust => ast_rust::is_structural_definition_parent(parent_kind.as_ref()),
        SupportLang::Python => ast_python::is_structural_definition_parent(parent_kind.as_ref()),
        _ => false,
    }
}

/// Returns true if the node is the name of a member defined inside a trait implementation
/// (`impl Trait for Type` in Rust or `@override` method in Python), i.e. a name mandated by a contract.
///
/// Such names cannot be changed without breaking the implementation, so naming rules that
/// suggest renaming do not meaningfully apply to them. Only the member name itself qualifies;
/// nested names the author does control, such as locals, do not.
#[must_use]
pub fn is_trait_impl_member(node: &AstNode<'_>, lang: SupportLang) -> bool {
    let Some(item) = node.parent() else {
        return false;
    };
    match lang {
        SupportLang::Rust => ast_rust::is_trait_impl_member(&item),
        SupportLang::Python => ast_python::is_trait_impl_member(&item),
        _ => false,
    }
}

/// Collects all binding nodes in `grep` whose identifier names are locally authored and
/// eligible for renaming (excluding unaliased imports and trait/override contract members).
#[must_use]
pub fn collect_renameable_bindings(grep: &AstGrep<SourceDoc>) -> Vec<AstNode<'_>> {
    let lang = *grep.lang();
    collect_bindings(grep)
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

/// Finds all variable, constant, and parameter bindings in `grep` whose name ends
/// (case-insensitively) with any suffix in `banned_suffixes`.
///
/// Automatically skips imports ([`is_import_binding`]), structural definitions
/// ([`is_structural_definition`]: functions, classes, structs, enums, traits), and
/// trait/override contract names ([`is_trait_impl_member`]).
/// Suffixes are evaluated longest-first for deterministic matching.
#[must_use]
pub fn find_suffixed_bindings<'a, S: std::hash::BuildHasher>(
    grep: &'a AstGrep<SourceDoc>,
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

    let bindings = collect_bindings(grep);
    let lang = *grep.lang();
    let mut matches = Vec::new();

    for node in bindings {
        if is_import_binding(&node, lang)
            || is_structural_definition(&node, lang)
            || is_trait_impl_member(&node, lang)
        {
            continue;
        }

        let name = node.text();
        let name_lower = name.to_lowercase();

        for suffix_lower in &sorted_suffixes {
            if name.len() > suffix_lower.len() && name_lower.ends_with(suffix_lower.as_str()) {
                let split_idx = name.len() - suffix_lower.len();
                let Some((base_name, actual_suffix)) = name.split_at_checked(split_idx) else {
                    continue;
                };

                matches.push(SuffixedBindingMatch {
                    node,
                    actual_suffix: actual_suffix.to_string(),
                    base_name: base_name.to_string(),
                    name: name.into_owned(),
                });
                break;
            }
        }
    }

    matches
}

/// Resolves `rule`'s banned identifier suffixes against `defaults` for the file's language
/// and emits a diagnostic with `("name", ...), ("actual_suffix", ...), ("base_name", ...)`
/// for every matching variable, constant, or parameter binding.
#[must_use]
pub fn check_banned_suffixes(
    rule: &(impl CodeRule + ?Sized),
    path: &Path,
    grep: &AstGrep<SourceDoc>,
    config: &Config,
    defaults: &FilterListDefaults,
) -> Vec<Diagnostic> {
    let effective_banned = rule.effective_banned_set(*grep.lang(), config, defaults);
    find_suffixed_bindings(grep, &effective_banned)
        .into_iter()
        .map(|matched| {
            rule.diagnostic_at_node(
                path,
                &matched.node,
                &[
                    ("name", &matched.name),
                    ("actual_suffix", &matched.actual_suffix),
                    ("base_name", &matched.base_name),
                ],
            )
        })
        .collect()
}
