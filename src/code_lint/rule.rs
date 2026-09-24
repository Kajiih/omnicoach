//! Code rule contract ([`CodeRule`], [`RuleTarget`]) and shared diagnostic helpers.

use crate::code_lint::ast::{AstNode, ParsedFile};
use crate::code_lint::{bindings, calls};
use crate::core::{Config, FilterListDefaults, Rule};
use crate::diagnostic::Diagnostic;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Target execution scope for a code rule (source files vs test files).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuleTarget {
    /// Rule runs on all matching files.
    #[default]
    All,
    /// Rule runs exclusively on test files.
    TestsOnly,
    /// Rule runs exclusively on production source files (skipped on test files).
    SourceOnly,
}

/// Common trait for static file code validation rules.
pub trait CodeRule: Rule {
    /// Returns the target execution scope of this code rule (defaults to `RuleTarget::All`).
    #[must_use]
    fn target(&self) -> RuleTarget {
        RuleTarget::All
    }

    /// Returns true if this rule supports the given language.
    #[must_use]
    fn supports_language(&self, lang: SupportLang) -> bool {
        self.supported_languages().contains(&lang)
    }

    /// Renders this rule's violation template at the given AST node using the node's language.
    #[must_use]
    fn diagnostic_at_node(
        &self,
        path: &Path,
        node: &AstNode<'_>,
        params: &[(&str, &str)],
    ) -> Diagnostic {
        self.render_diagnostic_for_lang(node.lang(), params, node.to_source_location(path))
    }

    /// Resolves this rule's banned call patterns against `defaults` for the file's language
    /// and returns all matching call expressions in `file`.
    #[must_use]
    fn find_configured_banned_calls<'a>(
        &self,
        file: &'a ParsedFile,
        config: &Config,
        defaults: &FilterListDefaults,
    ) -> Vec<calls::CallMatch<'a>> {
        let effective_banned = self.effective_banned_set(file.lang(), config, defaults);
        calls::find_banned_calls(file, &effective_banned)
    }

    /// Evaluates `find_configured_banned_calls` and emits a diagnostic with `("callee", &matched.callee)`
    /// for every matched call expression.
    #[must_use]
    fn check_banned_calls(
        &self,
        path: &Path,
        file: &ParsedFile,
        config: &Config,
        defaults: &FilterListDefaults,
    ) -> Vec<Diagnostic> {
        self.find_configured_banned_calls(file, config, defaults)
            .into_iter()
            .map(|matched| {
                self.diagnostic_at_node(path, &matched.node, &[("callee", &matched.callee)])
            })
            .collect()
    }

    /// Resolves this rule's banned identifier suffixes against `defaults` for the file's language
    /// and emits a diagnostic with `("name", ...), ("actual_suffix", ...), ("base_name", ...)`
    /// for every matching variable, constant, or parameter binding.
    #[must_use]
    fn check_banned_suffixes(
        &self,
        path: &Path,
        file: &ParsedFile,
        config: &Config,
        defaults: &FilterListDefaults,
    ) -> Vec<Diagnostic> {
        let effective_banned = self.effective_banned_set(file.lang(), config, defaults);
        bindings::find_suffixed_bindings(file, &effective_banned)
            .into_iter()
            .map(|matched| {
                self.diagnostic_at_node(
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

    /// Evaluates the file against this static analysis rule.
    ///
    /// Returned diagnostics may be in any order: ordering is owned by the reporting layer
    /// ([`crate::diagnostic`]), so sorting here is dead work.
    #[must_use]
    fn check_file(&self, path: &Path, file: &ParsedFile, config: &Config) -> Vec<Diagnostic>;
}
