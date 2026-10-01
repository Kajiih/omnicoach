//! Rule documentation: the user-facing doc each rule declares next to its classification.
//!
//! Pure, constant data with no rendering. Fields hold Markdown without `#` headings: the
//! renderer owns the section headings.

architecture_component!(RuleDocumentation);

/// A link to a document backing or related to a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reference {
    /// The link text.
    pub title: &'static str,
    /// The target URL.
    pub url: &'static str,
}

/// A configuration shape a rule reads under `[rules.<name>]`. Its keys are derived from the
/// matching config struct, never listed by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigShape {
    /// Numeric `min` / `max` thresholds.
    Threshold,
    /// A deny list: replace, extend or exempt banned items.
    DenyList,
    /// An allow list: replace, extend or revoke allowed items.
    AllowList,
    /// An enforcement `mode`.
    Enforcement,
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
    /// The configuration shapes the rule reads; empty if it takes no configuration.
    pub configuration: &'static [ConfigShape],
    /// Links to backing or related documents.
    pub references: &'static [Reference],
}

impl RuleDoc {
    /// Placeholder for rules whose doc is not written yet (content pass, D12).
    pub const TODO: Self = Self {
        summary: "Documentation pending.",
        what_it_does: "Documentation pending.",
        why_is_this_bad: "Documentation pending.",
        configuration: &[],
        references: &[],
    };

    /// Whether this is the [`Self::TODO`] placeholder.
    #[must_use]
    pub fn is_placeholder(&self) -> bool {
        *self == Self::TODO
    }
}
