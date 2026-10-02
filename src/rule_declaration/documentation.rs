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
///
/// Examples are documentation written for the reader, rendered by `--explain`. They
/// deliberately overlap the `rule_test!` `pass` / `fail` cases and never replace them, so
/// editing an example for readability cannot drop coverage.
///
/// Write both snippets as short, realistic `indoc::indoc!` raw strings. They are
/// self-contained bodies: include an import or `use` only when the fix introduces or
/// replaces it (`inspect.cleandoc`, `Duration`, `timedelta`), or when the definition
/// syntax requires it (`@dataclass`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Example {
    /// The language both snippets are written in.
    pub language: SupportLang,
    /// Code the rule flags once.
    pub flagged: &'static str,
    /// The part of `flagged` the finding spans; not rendered. It plays the role of a
    /// `rule_test!` case's `=> r#"..."#` span.
    pub flagged_span: &'static str,
    /// The same code rewritten as the rule's template `suggestion` prescribes.
    pub fixed: &'static str,
}

/// A rule's user-facing documentation, rendered by `--explain`. A missing section does not
/// compile (`E0063`).
///
/// Option defaults are rendered from the rule's options declaration, so the prose never
/// repeats them. Wording rules for `summary` are in
/// `docs/dev/naming_and_message_style_guide.md` §2.5, checked by `tests/registry.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleDoc {
    /// One sentence for rule lists, opening with `Flags` (pattern rules) or `Requires`
    /// (rules whose fix adds something) and ending with a period.
    pub summary: &'static str,
    /// What the rule flags, in a few sentences: the covered constructs and the explicit
    /// exemptions.
    pub what_it_does: &'static str,
    /// Why the flagged code is a problem: the authoritative, longer rationale.
    pub why_is_this_bad: &'static str,
    /// Links to backing or related documents.
    pub references: &'static [Reference],
    /// Exactly one [`Example`] per analyzed language (checked by `tests/registry.rs`);
    /// empty for rules without a code harness.
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
