//! Rule documentation: the user-facing doc each rule declares next to its classification.
//!
//! Pure, constant data with no rendering. Fields hold Markdown without `#` headings: the
//! renderer owns the section headings.

use ast_grep_language::SupportLang;

/// A link to a document backing or related to a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reference {
    /// The link text.
    pub title: &'static str,
    /// The target URL.
    pub url: &'static str,
}

/// A flagged snippet and its fix in one language.
///
/// The rule's tests check both as strictly as `rule_test!` cases: the flagged snippet yields
/// exactly one finding spanning `flagged_span`, also when repeated, and the fixed one none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Example {
    /// The language both snippets are written in.
    pub language: SupportLang,
    /// Code the rule flags once.
    pub flagged: &'static str,
    /// The part of `flagged` the finding spans; not rendered.
    pub flagged_span: &'static str,
    /// The same code following the rule's suggestion.
    pub fixed: &'static str,
}

/// A rule's user-facing documentation. A missing section does not compile (`E0063`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleDoc {
    /// One sentence for rule lists, ending with a period.
    pub summary: &'static str,
    /// What the rule flags, in a few sentences.
    pub what_it_does: &'static str,
    /// Why the flagged code is a problem: the authoritative, longer rationale.
    pub why_is_this_bad: &'static str,
    /// Links to backing or related documents.
    pub references: &'static [Reference],
    /// One example per analyzed language; empty for rules without a code harness.
    pub examples: &'static [Example],
}

impl RuleDoc {
    /// Placeholder for rules whose doc is not written yet.
    pub const TODO: Self = Self {
        summary: "Documentation pending.",
        what_it_does: "Documentation pending.",
        why_is_this_bad: "Documentation pending.",
        references: &[],
        examples: &[],
    };

    /// Whether this is the [`Self::TODO`] placeholder.
    #[must_use]
    pub fn is_placeholder(&self) -> bool {
        *self == Self::TODO
    }
}
