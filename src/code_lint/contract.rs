//! Code rule contract: [`CodeRule`], its type-erased [`AnyCodeRule`] view, [`RuleTarget`], and
//! the diagnostic helpers its check functions share.

architecture_component!(CodeLintContract);

use crate::code_lint::ast::{AstNode, ParsedFile};
use crate::code_lint::project::ProjectContext;
use crate::code_lint::semantic::{bindings, calls, comments::CommentIndex};
use crate::diagnostic::{Diagnostic, Language, RuleName, SourceSpan};
use crate::rule_declaration::{
    Declaration, DeclaredExamples, DeclaredRule, EnforcementMode, OptionsDeclaration,
    ProjectExample, RuleOverrides,
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
/// `&HashSet<String>` for a [`crate::rule_declaration::ListOption`], or a named struct from
/// `rule_options!`. A check function whose last parameter does not match the declared options
/// does not compile.
///
/// Rules express policy, not plumbing. `check` queries the [`ParsedFile`] through named
/// `code_lint::ast` and `code_lint::semantic` helpers (or `Self::check_banned_calls`) and
/// anchors findings on an [`AstNode`] with `Self::diagnostic_at_node`. `AstNode` exposes
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
    pub(crate) check: for<'a> fn(&Self, &Path, &ParsedFile, Options::Param<'a>) -> Vec<Diagnostic>,
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
        let diagnostics = (self.check)(self, path, file, Options::as_param(&options));
        apply_enforcement_mode(&self.declaration, file, overrides, diagnostics)
    }

    /// Renders this rule's violation template at the given AST node using the node's language.
    #[must_use]
    pub(super) fn diagnostic_at_node(
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
    pub(super) fn check_banned_calls(
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
    pub(super) fn check_banned_calls_where(
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
    /// value binding (variable, constant, parameter, declared attribute, or struct field) ending
    /// in one of the `banned` suffixes.
    #[must_use]
    pub(super) fn check_banned_suffixes(
        &self,
        path: &Path,
        file: &ParsedFile,
        banned: &HashSet<String>,
    ) -> Vec<Diagnostic> {
        self.check_banned_suffixes_where(path, file, banned, |_| true)
    }

    /// Emits a diagnostic with `("name", ...), ("suffix", ...), ("stem", ...)` for every
    /// value binding ending in one of the `banned` suffixes that satisfies `predicate`.
    #[must_use]
    pub(super) fn check_banned_suffixes_where(
        &self,
        path: &Path,
        file: &ParsedFile,
        banned: &HashSet<String>,
        predicate: impl FnMut(&bindings::SuffixedBindingMatch<'_>) -> bool,
    ) -> Vec<Diagnostic> {
        bindings::find_suffixed_bindings(file, banned)
            .into_iter()
            .filter(predicate)
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

/// A code rule that queries cross-file [`ProjectContext`] in addition to the target
/// [`ParsedFile`], emitting per-file diagnostics through the standard target, explanation,
/// suppression, and `--diff` pipeline.
#[derive(Clone, Copy)]
pub struct ProjectCodeRule<Options: OptionsDeclaration = (), Examples = &'static [ProjectExample]> {
    /// Name, template, languages, options, classification and doc.
    pub declaration: Declaration<Options, Examples>,
    /// The files the rule runs on.
    pub target: RuleTarget,
    /// Finds the rule's violations in `file` within `project`.
    pub(crate) check: for<'a> fn(
        &Self,
        &Path,
        &ParsedFile,
        &ProjectContext<'_>,
        Options::Param<'a>,
    ) -> Vec<Diagnostic>,
}

impl<Options: OptionsDeclaration, Examples> ProjectCodeRule<Options, Examples> {
    /// Finds the rule's violations in `file` within `project`, with options and enforcement mode
    /// resolved for the file's language from `overrides`.
    #[must_use]
    pub fn check_file(
        &self,
        path: &Path,
        file: &ParsedFile,
        project: &ProjectContext<'_>,
        overrides: Option<&RuleOverrides>,
    ) -> Vec<Diagnostic> {
        let options = self
            .declaration
            .options
            .options
            .resolve(file.lang(), overrides);
        let diagnostics = (self.check)(self, path, file, project, Options::as_param(&options));
        apply_enforcement_mode(&self.declaration, file, overrides, diagnostics)
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

    /// Renders this rule's violation template at `span` in `file` using the file's language.
    #[must_use]
    pub fn diagnostic_at_span(
        &self,
        path: &Path,
        file: &ParsedFile,
        span: SourceSpan,
        params: &[(&str, &str)],
    ) -> Diagnostic {
        self.declaration.render_diagnostic_for_lang(
            file.lang(),
            params,
            file.to_source_location(path, span),
        )
    }
}

/// A [`CodeRule`] or [`ProjectCodeRule`] with its options and examples type erased, so all code
/// rules fit in one registry slice (`CODE_RULES`).
pub trait AnyCodeRule: Send + Sync {
    /// The rule's declaration with its options and examples listed rather than typed.
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
    /// Whether evaluating this rule requires a [`ProjectContext`].
    fn requires_project_context(&self) -> bool;
    /// Evaluates the rule on `file`, returning `None` when the rule requires a [`ProjectContext`]
    /// and `project` is `None` so callers cannot treat an unevaluated cross-file rule as clean.
    fn check_file_in_project(
        &self,
        path: &Path,
        file: &ParsedFile,
        project: Option<&ProjectContext<'_>>,
        overrides: Option<&RuleOverrides>,
    ) -> Option<Vec<Diagnostic>>;
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

    fn requires_project_context(&self) -> bool {
        false
    }

    fn check_file_in_project(
        &self,
        path: &Path,
        file: &ParsedFile,
        _project: Option<&ProjectContext<'_>>,
        overrides: Option<&RuleOverrides>,
    ) -> Option<Vec<Diagnostic>> {
        Some(Self::check_file(self, path, file, overrides))
    }
}

impl<Options, Examples> AnyCodeRule for ProjectCodeRule<Options, Examples>
where
    Options: OptionsDeclaration + Send + Sync,
    Examples: Copy + Send + Sync + Into<DeclaredExamples>,
{
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

    fn requires_project_context(&self) -> bool {
        true
    }

    fn check_file_in_project(
        &self,
        path: &Path,
        file: &ParsedFile,
        project: Option<&ProjectContext<'_>>,
        overrides: Option<&RuleOverrides>,
    ) -> Option<Vec<Diagnostic>> {
        project.map(|context| Self::check_file(self, path, file, context, overrides))
    }
}

fn apply_enforcement_mode<Options: OptionsDeclaration, Examples>(
    declaration: &Declaration<Options, Examples>,
    file: &ParsedFile,
    overrides: Option<&RuleOverrides>,
    mut diagnostics: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    let mode = declaration.options.enforcement_mode(file.lang(), overrides);
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
