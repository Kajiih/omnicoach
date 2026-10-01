//! Code rule contract: [`CodeRule`], its type-erased [`AnyCodeRule`] view, [`RuleTarget`], and
//! the diagnostic helpers its check functions share.

architecture_component!(CodeRuleContracts);

use crate::code_lint::ast::{AstNode, ParsedFile};
use crate::code_lint::semantic::{bindings, calls};
use crate::diagnostic::{Diagnostic, RuleName};
use crate::rule_declaration::{
    Declaration, DeclaredRule, EnforcementMode, OptionsDeclaration, RuleOverrides,
};
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

/// A rule that analyzes source files: its declaration, the files it runs on, and the function
/// that finds its violations.
///
/// `check` receives the rule's options already resolved for the file, typed by the
/// declaration: `()` without options, `usize` for a [`crate::rule_declaration::CountOption`],
/// `&HashSet<String>` for a [`crate::rule_declaration::ListOption`], a tuple for a pair. A check function
/// whose last parameter does not match the declared options does not compile.
#[derive(Clone, Copy)]
pub struct CodeRule<Options: OptionsDeclaration = ()> {
    /// Name, template, languages, options, classification and doc.
    pub declaration: Declaration<Options>,
    /// The files the rule runs on.
    pub target: RuleTarget,
    /// Finds the rule's violations in one file. Returned diagnostics may be in any order:
    /// ordering is owned by the reporting layer ([`crate::diagnostic`]).
    pub check: for<'a> fn(&Self, &Path, &ParsedFile, Options::Param<'a>) -> Vec<Diagnostic>,
}

impl<Options: OptionsDeclaration> CodeRule<Options> {
    /// Finds the rule's violations in `file`, with its options resolved for the file's
    /// language from `overrides` (the rule's `[rules.<name>]` configuration, if any).
    #[must_use]
    pub fn check_file(
        &self,
        path: &Path,
        file: &ParsedFile,
        overrides: Option<&RuleOverrides>,
    ) -> Vec<Diagnostic> {
        let options = self
            .declaration
            .options
            .options
            .resolve(file.lang(), overrides);
        (self.check)(self, path, file, Options::as_param(&options))
    }

    /// The enforcement mode for a file in `language`.
    #[must_use]
    pub fn enforcement_mode(
        &self,
        language: SupportLang,
        overrides: Option<&RuleOverrides>,
    ) -> EnforcementMode {
        self.declaration
            .options
            .enforcement_mode(language, overrides)
    }

    /// Renders this rule's violation template at the given AST node using the node's language.
    #[must_use]
    pub fn diagnostic_at_node(
        &self,
        path: &Path,
        node: &AstNode<'_>,
        params: &[(&str, &str)],
    ) -> Diagnostic {
        self.declaration.render_diagnostic_for_lang(
            node.lang(),
            params,
            node.to_source_location(path),
        )
    }

    /// Emits a diagnostic with `("callee", &matched.callee)` for every call in `file` to one of
    /// the `banned` callees.
    #[must_use]
    pub fn check_banned_calls(
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
    pub fn check_banned_suffixes(
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
}

/// A [`CodeRule`] with its options type erased, so rules with different option types fit in one
/// registry slice. Implemented once for every `CodeRule<Options>`.
pub trait AnyCodeRule: Send + Sync {
    /// The rule's declaration with its options listed rather than typed.
    fn declaration(&self) -> DeclaredRule;
    /// The rule's name.
    fn name(&self) -> RuleName;
    /// The languages the rule analyzes.
    fn languages(&self) -> &'static [SupportLang];
    /// Whether the rule analyzes `language`.
    fn supports_language(&self, language: SupportLang) -> bool {
        self.languages().contains(&language)
    }
    /// The files the rule runs on.
    fn target(&self) -> RuleTarget;
    /// The enforcement mode for a file in `language`, given the rule's configuration.
    fn enforcement_mode(
        &self,
        language: SupportLang,
        overrides: Option<&RuleOverrides>,
    ) -> EnforcementMode;
    /// Finds the rule's violations in `file`, given the rule's configuration.
    fn check_file(
        &self,
        path: &Path,
        file: &ParsedFile,
        overrides: Option<&RuleOverrides>,
    ) -> Vec<Diagnostic>;
}

impl<Options: OptionsDeclaration + Send + Sync> AnyCodeRule for CodeRule<Options> {
    fn declaration(&self) -> DeclaredRule {
        self.declaration.declared()
    }

    fn name(&self) -> RuleName {
        self.declaration.name
    }

    fn languages(&self) -> &'static [SupportLang] {
        self.declaration.languages
    }

    fn target(&self) -> RuleTarget {
        self.target
    }

    fn enforcement_mode(
        &self,
        language: SupportLang,
        overrides: Option<&RuleOverrides>,
    ) -> EnforcementMode {
        Self::enforcement_mode(self, language, overrides)
    }

    fn check_file(
        &self,
        path: &Path,
        file: &ParsedFile,
        overrides: Option<&RuleOverrides>,
    ) -> Vec<Diagnostic> {
        Self::check_file(self, path, file, overrides)
    }
}
