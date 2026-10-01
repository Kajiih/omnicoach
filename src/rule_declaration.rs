//! Rule declaration: what every rule states about itself, once, as constant data.
//!
//! [`Declaration`] bundles a rule's name, message template, languages, options,
//! classification and doc. Suppression audits are declarations; code and command rules embed
//! one next to their check function. [`DeclaredRule`] is the same data with the options
//! listed rather than typed, so rules with different option types can be listed together.

architecture_component!(RuleDeclaration);

use ast_grep_language::SupportLang;

use crate::core::{DeclaredOptions, OptionsDeclaration, RuleOptions};
use crate::diagnostic::{Diagnostic, RuleName, SourceLocation, ViolationTemplate};
use crate::rule_documentation::RuleDoc;
use crate::rule_taxonomy::Classification;

/// What a rule states about itself: name, message template, languages, options,
/// classification and doc. A rule cannot be registered without all six.
///
/// `Options` is the type of the declared options (`()` when there are none); it fixes the
/// type of the values a code rule's check function receives.
#[derive(Debug, Clone, Copy)]
pub struct Declaration<Options: OptionsDeclaration = ()> {
    /// The rule's name, e.g. `RuleName("max-test-assertions")`.
    pub name: RuleName,
    /// The single message template of the rule's findings.
    pub template: &'static ViolationTemplate,
    /// The languages the rule analyzes; empty for command rules.
    pub languages: &'static [SupportLang],
    /// Everything the rule accepts under `[rules.<name>]`.
    pub options: RuleOptions<Options>,
    /// The rule's classification.
    pub classification: Classification,
    /// The rule's user-facing doc.
    pub doc: RuleDoc,
}

impl<Options: OptionsDeclaration> Declaration<Options> {
    /// The declaration with its options listed rather than typed.
    #[must_use]
    pub fn declared(&self) -> DeclaredRule {
        DeclaredRule {
            name: self.name,
            template: self.template,
            languages: self.languages,
            options: self.options.declared(),
            classification: self.classification,
            doc: self.doc,
        }
    }

    /// A finding at `location`, with the template's base text filled with `params`.
    #[must_use]
    pub fn render_diagnostic(
        &self,
        params: &[(&str, &str)],
        location: SourceLocation,
    ) -> Diagnostic {
        Diagnostic::new(self.name, self.template.render(params), location)
    }

    /// A finding at `location`, with the template's text for `language` filled with `params`.
    #[must_use]
    pub fn render_diagnostic_for_lang(
        &self,
        language: SupportLang,
        params: &[(&str, &str)],
        location: SourceLocation,
    ) -> Diagnostic {
        Diagnostic::new(
            self.name,
            self.template.render_for_lang(language, params),
            location,
        )
    }
}

/// A rule's declaration with its options listed rather than typed: what rule selection,
/// configuration validation and the rule catalog read.
#[derive(Debug, Clone)]
pub struct DeclaredRule {
    /// The rule's name.
    pub name: RuleName,
    /// The single message template of the rule's findings.
    pub template: &'static ViolationTemplate,
    /// The languages the rule analyzes; empty for command rules.
    pub languages: &'static [SupportLang],
    /// Everything the rule accepts under `[rules.<name>]`.
    pub options: DeclaredOptions,
    /// The rule's classification.
    pub classification: Classification,
    /// The rule's user-facing doc.
    pub doc: RuleDoc,
}
