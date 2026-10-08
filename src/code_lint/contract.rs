//! Code rule contract: [`CodeRule`], its type-erased [`AnyCodeRule`] view, [`RuleTarget`], and
//! the diagnostic helpers its check functions share.

architecture_component!(CodeLintContract);

use crate::code_lint::ast::{AstNode, ParsedFile};
use crate::code_lint::semantic::{bindings, calls, comments::CommentIndex};
use crate::diagnostic::{Diagnostic, Language, RuleName};
use crate::rule_declaration::{
    Declaration, DeclaredRule, EnforcementMode, OptionsDeclaration, RuleOverrides,
};
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
/// A rule file exposes one `pub const` `CodeRule` per rule, listed in `CODE_RULES`
/// (`src/code_lint/rules.rs`), and ends with a `rule_test!` suite.
///
/// `check` receives the rule's options already resolved for the file, typed by the
/// declaration: `()` without options, `usize` for a [`crate::rule_declaration::CountOption`],
/// `&HashSet<String>` for a [`crate::rule_declaration::ListOption`], a tuple for a pair. A check function
/// whose last parameter does not match the declared options does not compile.
///
/// Rules express policy, not plumbing. `check` queries the [`ParsedFile`] through named
/// `code_lint::ast` and `code_lint::semantic` helpers (or [`Self::check_banned_calls`]) and
/// anchors findings on an [`AstNode`] with [`Self::diagnostic_at_node`]. `AstNode` exposes
/// text and location but no tree navigation, so a rule needing a new structural fact adds a
/// named helper to `code_lint::ast` rather than walking the tree itself.
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
    ///
    /// In [`EnforcementMode::RequireExplanation`], violations explained by an adjacent comment
    /// are dropped, and the others carry [`EnforcementMode::EXPLANATION_HINT`].
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
        let mut diagnostics = (self.check)(self, path, file, Options::as_param(&options));
        let mode = self
            .declaration
            .options
            .enforcement_mode(file.lang(), overrides);
        if mode == EnforcementMode::RequireExplanation && !diagnostics.is_empty() {
            let index = CommentIndex::from_file(file);
            diagnostics.retain(|diagnostic| {
                !index.has_explanation_for_span(
                    file,
                    diagnostic.location.span,
                    diagnostic.location.line,
                )
            });
            for diagnostic in &mut diagnostics {
                diagnostic.message.explanation_hint =
                    Some(EnforcementMode::EXPLANATION_HINT.to_owned());
            }
        }
        diagnostics
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
        self.check_banned_calls_where(path, file, banned, |_| true)
    }

    /// Emits a diagnostic with `("callee", &matched.callee)` for every call in `file` to one of
    /// the `banned` callees that satisfies `predicate`.
    #[must_use]
    pub fn check_banned_calls_where(
        &self,
        path: &Path,
        file: &ParsedFile,
        banned: &HashSet<String>,
        predicate: impl FnMut(&calls::CallMatch<'_>) -> bool,
    ) -> Vec<Diagnostic> {
        calls::find_banned_calls(file, banned)
            .into_iter()
            .filter(predicate)
            .map(|matched| {
                self.diagnostic_at_node(path, &matched.node, &[("callee", &matched.callee)])
            })
            .collect()
    }

    /// Emits a diagnostic with `("name", ...), ("suffix", ...), ("stem", ...)` for every
    /// variable, constant, or parameter binding ending in one of the `banned` suffixes.
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
                        ("suffix", &matched.actual_suffix),
                        ("stem", &matched.base_name),
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
    fn languages(&self) -> &'static [Language];
    /// Whether the rule analyzes `language`.
    fn supports_language(&self, language: Language) -> bool {
        self.languages().contains(&language)
    }
    /// The files the rule runs on.
    fn target(&self) -> RuleTarget;
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

    fn languages(&self) -> &'static [Language] {
        self.declaration.languages
    }

    fn target(&self) -> RuleTarget {
        self.target
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::{ViolationTemplate, violation_template};
    use crate::rule_declaration::{
        Classification, Consensus, ImpactedQuality, LanguageDefaults, Precision, RuleDoc,
        RuleOptions, Topic,
    };
    use rstest::rstest;

    const TEMPLATE: ViolationTemplate = violation_template! {
        summary: "Function `f` is called.",
        rationale: "Calling `f` is a test fixture.",
        suggestion: "Remove the call.",
    };

    /// Flags every call to `f`.
    fn check_calls_to_f(
        rule: &CodeRule,
        path: &Path,
        file: &ParsedFile,
        (): (),
    ) -> Vec<Diagnostic> {
        rule.check_banned_calls(path, file, &HashSet::from(["f".to_owned()]))
    }

    /// A rule flagging calls to `f`, declaring `mode` as its default enforcement mode.
    fn rule_with_mode(mode: Option<EnforcementMode>) -> CodeRule {
        CodeRule {
            declaration: Declaration {
                name: RuleName("call-to-f"),
                template: &TEMPLATE,
                languages: &[Language::Python],
                options: RuleOptions {
                    enforcement_mode: mode.map(|mode| LanguageDefaults::new(mode, &[])),
                    options: (),
                },
                classification: Classification {
                    topics: &[Topic::STATIC_TYPING],
                    precision: Precision::Exact,
                    consensus: Consensus::Opinionated,
                    impacted_quality: ImpactedQuality::Maintainability,
                },
                doc: RuleDoc::TODO,
            },
            target: RuleTarget::All,
            check: check_calls_to_f,
        }
    }

    #[rstest]
    #[case::require_explanation_hints_kept_findings_and_drops_explained_ones(
        Some(EnforcementMode::RequireExplanation),
        &[(2, Some(EnforcementMode::EXPLANATION_HINT))]
    )]
    #[case::ban_adds_no_hint(Some(EnforcementMode::Ban), &[(1, None), (2, None)])]
    #[case::rule_without_mode_adds_no_hint(None, &[(1, None), (2, None)])]
    fn test_check_file_sets_explanation_hint_by_mode(
        #[case] mode: Option<EnforcementMode>,
        #[case] expected: &[(usize, Option<&str>)],
    ) {
        let source = indoc::indoc! {"
            f()  # The first call is explained by this comment.
            f()
        "};
        let mut diagnostics = rule_with_mode(mode).check_file(
            Path::new("module.py"),
            &ParsedFile::new(source, Language::Python),
            None,
        );
        diagnostics.sort_unstable();
        let actual: Vec<(usize, Option<&str>)> = diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.location.line,
                    diagnostic.message.explanation_hint.as_deref(),
                )
            })
            .collect();
        assert_eq!(actual, expected);
    }
}
