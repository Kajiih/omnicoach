//! Code rule contract ([`CodeDetector`], [`RuleTarget`]) and shared diagnostic helpers.

architecture_component!(CodeRuleContracts);

use crate::code_lint::ast::{AstNode, ParsedFile};
use crate::code_lint::semantic::{bindings, calls};
use crate::core::{Detector, ResolvedOptions};
use crate::diagnostic::Diagnostic;
use ast_grep_language::SupportLang;
use std::collections::HashSet;
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

/// A detector that analyzes source files.
pub trait CodeDetector: Detector {
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

    /// Emits a diagnostic with `("callee", &matched.callee)` for every call in `file` to one of
    /// the `banned` callees.
    #[must_use]
    fn check_banned_calls(
        &self,
        path: &Path,
        file: &ParsedFile,
        banned: &HashSet<String>,
    ) -> Vec<Diagnostic> {
        calls::find_banned_calls(file, banned)
            .into_iter()
            .map(|matched| {
                self.diagnostic_at_node(path, &matched.node, &[("callee", &matched.callee)])
            })
            .collect()
    }

    /// Emits a diagnostic with `("name", ...), ("actual_suffix", ...), ("base_name", ...)` for
    /// every variable, constant, or parameter binding ending in one of the `banned` suffixes.
    #[must_use]
    fn check_banned_suffixes(
        &self,
        path: &Path,
        file: &ParsedFile,
        banned: &HashSet<String>,
    ) -> Vec<Diagnostic> {
        bindings::find_suffixed_bindings(file, banned)
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
    fn check_file(
        &self,
        path: &Path,
        file: &ParsedFile,
        options: &ResolvedOptions<'_>,
    ) -> Vec<Diagnostic>;
}
