//! Rule declaration: what every rule states about itself, once, as constant data.
//!
//! [`Declaration`] bundles a rule's name, message template, languages, options,
//! classification and doc. Suppression audits are declarations; code and command rules embed
//! one next to their check function. [`DeclaredRule`] is the same data with the options
//! listed rather than typed, so rules with different option types can be listed together.

architecture_component!(RuleDeclaration);

mod documentation;
mod options;
mod taxonomy;

pub use self::documentation::{Example, Reference, RuleDoc};
pub use self::options::{
    CountOption, DeclaredOptions, EnforcementMode, FilterListDefaults, LanguageDefaults, ListKind,
    ListOption, OptionProblem, OptionSpec, OptionsDeclaration, RuleOptions, RuleOptionsError,
    RuleOverrides,
};
pub use self::taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Topic};

use crate::diagnostic::{Diagnostic, Language, RuleName, SourceLocation, ViolationTemplate};

/// Returns true if `name` is a non-empty `kebab-case` identifier (`[a-z0-9]+(-[a-z0-9]+)*`).
#[must_use]
pub fn is_kebab_case(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

/// The candidate closest to `label`, if it is close enough to be a typo.
pub fn closest_match(
    label: &str,
    candidates: impl Iterator<Item = &'static str>,
) -> Option<&'static str> {
    let normalized = label.to_ascii_lowercase();
    let tolerance = (normalized.chars().count() / 3).max(1);
    candidates
        .map(|known| (edit_distance(&normalized, known), known))
        .filter(|&(distance, _)| distance <= tolerance)
        .min()
        .map(|(_, known)| known)
}

/// Levenshtein distance over characters.
fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for (row, left_char) in left.chars().enumerate() {
        let mut current = vec![row + 1];
        for (column, &right_char) in right.iter().enumerate() {
            let substitution = previous[column] + usize::from(left_char != right_char);
            current.push(
                substitution
                    .min(previous[column + 1] + 1)
                    .min(current[column] + 1),
            );
        }
        previous = current;
    }
    previous[right.len()]
}

/// What a rule states about itself: name, message template, languages, options,
/// classification and doc. A rule cannot be registered without all six.
///
/// `Options` is the type of the declared options (`()` when there are none); it fixes the
/// type of the values a code rule's check function receives.
#[derive(Debug, Clone, Copy)]
pub struct Declaration<Options: OptionsDeclaration = ()> {
    /// The rule's name, e.g. `RuleName("too-many-assertions")`.
    pub name: RuleName,
    /// The single message template of the rule's findings.
    pub template: &'static ViolationTemplate,
    /// The languages the rule analyzes; empty for command rules.
    pub languages: &'static [Language],
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
        language: Language,
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
    pub languages: &'static [Language],
    /// Everything the rule accepts under `[rules.<name>]`.
    pub options: DeclaredOptions,
    /// The rule's classification.
    pub classification: Classification,
    /// The rule's user-facing doc.
    pub doc: RuleDoc,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case::missing_letter("alpa", Some("alpha"))]
    #[case::case_folded("ALPHA", Some("alpha"))]
    #[case::nearest_wins("alphabe", Some("alphabet"))]
    #[case::at_tolerance("alphaxy", Some("alpha"))]
    #[case::beyond_tolerance("alphaxyz", None)]
    fn closest_match_suggests_typos_only(#[case] label: &str, #[case] expected: Option<&str>) {
        let candidates = ["alpha", "alphabet", "omega"];
        assert_eq!(closest_match(label, candidates.into_iter()), expected);
    }

    #[test]
    fn edit_distance_is_levenshtein() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }
}
