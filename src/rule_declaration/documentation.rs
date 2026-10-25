//! Rule documentation: the user-facing doc each rule declares next to its classification.
//!
//! Pure, constant data with no rendering. Fields hold Markdown without `#` headings: the
//! renderer owns the section headings.

use crate::diagnostic::Language;

/// A link to a document backing or related to a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reference {
    /// The link text.
    pub title: &'static str,
    /// The target URL.
    pub url: &'static str,
}

impl Reference {
    /// How imported and locally defined names are matched; referenced by every rule whose
    /// matches depend on it, instead of restating it in `what_it_does`.
    pub const NAME_RESOLUTION: Self = Self {
        title: "How Omni resolves names",
        url: "https://github.com/Kajiih/omnicoach/blob/main/docs/name_resolution.md",
    };
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
    pub language: Language,
    /// Code the rule flags once.
    pub flagged: &'static str,
    /// The part of `flagged` the finding spans; not rendered. It plays the role of a
    /// `rule_test!` case's `=> r#"..."#` span.
    pub flagged_span: &'static str,
    /// The same code rewritten as the rule's template `suggestion` prescribes.
    pub fixed: &'static str,
}

/// A flagged shell command and its replacement for a command rule.
///
/// Executed by `command_rule_test!` and rendered by `--explain` under `## Example` with
/// ```` ```sh ```` code fences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandExample {
    /// Shell command the rule flags once.
    pub flagged: &'static str,
    /// The part of `flagged` the finding spans; not rendered.
    pub flagged_span: &'static str,
    /// Replacement shell command that satisfies the rule with zero findings.
    pub fixed: &'static str,
}

/// Type-erased example list carried by [`crate::rule_declaration::DeclaredRule`] and
/// [`crate::rule_selection::RegisteredRule`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredExamples {
    /// Language-tagged source-code examples declared by a code rule or suppression audit.
    Code(&'static [Example]),
    /// Shell command examples declared by a command rule.
    Command(&'static [CommandExample]),
}

impl DeclaredExamples {
    /// Returns `true` if no examples are declared.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        match self {
            Self::Code(examples) => examples.is_empty(),
            Self::Command(examples) => examples.is_empty(),
        }
    }
}

impl From<&'static [Example]> for DeclaredExamples {
    fn from(examples: &'static [Example]) -> Self {
        Self::Code(examples)
    }
}

impl From<&'static [CommandExample]> for DeclaredExamples {
    fn from(examples: &'static [CommandExample]) -> Self {
        Self::Command(examples)
    }
}

/// A rule's user-facing documentation, rendered by `--explain`. A missing section does not
/// compile (`E0063`).
///
/// Its objective is predictability: a reader can tell whether the rule flags their code without
/// reading the implementation (`docs/dev/rule_design_guide.md` §6).
///
/// The prose never repeats what `--explain` already renders or documents elsewhere: option
/// defaults such as the flagged callees or a threshold (`## Configuration`), the file scope
/// (`## Tags`), and how imported names resolve ([`Reference::NAME_RESOLUTION`], listed in
/// `references` by the rules that resolve names). Edge cases belong in the `rule_test!` cases.
/// Wording rules for `summary` are in `docs/dev/naming_and_message_style_guide.md` §2.5,
/// checked by `tests/registry.rs`.
///
/// Write `what_it_does`, `why_is_this_bad` and `known_problems` as `indoc::indoc!` raw strings
/// wrapped at the source line width, closed on their last line. They are Markdown: a single
/// line break is a space, and a blank line separates paragraphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleDoc<Examples = &'static [Example]> {
    /// One sentence for rule lists, opening with `Flags` (pattern rules) or `Requires`
    /// (rules whose fix adds something) and ending with a period.
    pub summary: &'static str,
    /// What the rule flags, in one to three sentences. Name an exemption only when a reader
    /// would otherwise mispredict whether common code is flagged; never list every covered or
    /// exempt construct.
    pub what_it_does: &'static str,
    /// Why the flagged code is a problem: the authoritative, longer rationale.
    pub why_is_this_bad: &'static str,
    /// Limitations a user may hit: false positives or false negatives the rule accepts by
    /// design or has not solved yet. `None` when there are none worth telling.
    pub known_problems: Option<&'static str>,
    /// Links to backing or related documents.
    pub references: &'static [Reference],
    /// Verified examples (one [`Example`] per analyzed language for code rules; one
    /// [`CommandExample`] for command rules).
    pub examples: Examples,
}

impl RuleDoc {
    /// Placeholder for rules whose doc is not written yet.
    pub const TODO: Self = Self {
        summary: "Documentation pending.",
        what_it_does: "Documentation pending.",
        why_is_this_bad: "Documentation pending.",
        known_problems: None,
        references: &[],
        examples: &[],
    };
}

impl<Examples> RuleDoc<Examples> {
    /// Whether this is the [`RuleDoc::TODO`] placeholder.
    #[must_use]
    pub fn is_placeholder(&self) -> bool {
        self.summary == RuleDoc::TODO.summary
            && self.what_it_does == RuleDoc::TODO.what_it_does
            && self.why_is_this_bad == RuleDoc::TODO.why_is_this_bad
            && self.known_problems.is_none()
            && self.references.is_empty()
    }

    /// The doc with its `examples` erased into [`DeclaredExamples`].
    #[must_use]
    pub fn declared(self) -> RuleDoc<DeclaredExamples>
    where
        Examples: Into<DeclaredExamples>,
    {
        RuleDoc {
            summary: self.summary,
            what_it_does: self.what_it_does,
            why_is_this_bad: self.why_is_this_bad,
            known_problems: self.known_problems,
            references: self.references,
            examples: self.examples.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_resolution_reference_points_at_a_repository_file() {
        let (_, path) = Reference::NAME_RESOLUTION
            .url
            .split_once("/blob/main/")
            .expect("the URL must point into the repository's `main` branch");
        assert!(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(path)
                .is_file(),
            "`{path}` does not exist"
        );
    }
}
