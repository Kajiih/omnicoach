//! Rule catalog: renders the rule list and one rule's documentation as plain Markdown.

architecture_component!(RuleCatalog);

use strum::IntoEnumIterator as _;

use crate::rule_documentation::ConfigShape;
use crate::rule_selection::{
    ConfigError, Facet, RegisteredRule, UnknownLabel, find_rule, load_rule_status,
    registered_rules, rules_tagged,
};

/// The configuration keys recognized by each shape.
#[must_use]
pub const fn shape_keys(shape: ConfigShape) -> &'static [&'static str] {
    match shape {
        ConfigShape::Threshold => crate::core::ThresholdConfig::KEYS,
        ConfigShape::DenyList => crate::core::DenyListConfig::KEYS,
        ConfigShape::AllowList => crate::core::AllowListConfig::KEYS,
        ConfigShape::Enforcement => crate::core::EnforcementConfig::KEYS,
    }
}

/// Why a discovery request failed. The message is what the user sees.
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    /// A `--tag` or `--explain` label that was not recognized.
    #[error(transparent)]
    Label(#[from] UnknownLabel),
    /// A configuration error when reading status.
    #[error(transparent)]
    Config(#[from] ConfigError),
}

/// Command-line flags for rule discovery: `--list-rules`, `--tag` and `--explain`.
#[derive(Debug, clap::Args)]
pub struct DiscoveryArgs {
    /// List every rule with its languages and summary, as Markdown
    #[arg(long)]
    pub list_rules: bool,

    /// With --list-rules, keep only the rules a `select` label selects (tag, synonym or rule)
    #[arg(long, value_name = "LABEL", requires = "list_rules")]
    pub tag: Option<String>,

    /// Print one rule's documentation as Markdown
    #[arg(long, value_name = "RULE", conflicts_with = "list_rules")]
    pub explain: Option<String>,
}

impl DiscoveryArgs {
    /// Renders the requested discovery output, or `None` when no discovery flag is given.
    #[must_use]
    pub fn render(&self) -> Option<Result<String, CatalogError>> {
        if let Some(name) = &self.explain {
            return Some(explain(name));
        }
        self.list_rules.then(|| list_rules(self.tag.as_deref()))
    }
}

/// The line printed after plain diagnostics, pointing at `--explain`.
#[must_use]
pub fn explain_footer(binary: &str) -> String {
    format!("For details on a rule, run: {binary} --explain <rule>")
}

/// One Markdown bullet per rule, sorted by name: name, languages (or input) and summary.
///
/// # Errors
///
/// Returns [`CatalogError::Label`] if `tag` is not a label `select` accepts.
pub fn list_rules(tag: Option<&str>) -> Result<String, CatalogError> {
    let mut rules = match tag {
        Some(label) => rules_tagged(label)?,
        None => registered_rules().iter().collect(),
    };
    rules.sort_by_key(|rule| rule.name);
    let lines: Vec<String> = rules
        .iter()
        .map(|rule| {
            let languages = Some(facet_tags(rule, Facet::Languages))
                .filter(|languages| !languages.is_empty())
                .unwrap_or_else(|| facet_tags(rule, Facet::Input));
            format!("- `{}` ({languages}): {}", rule.name, rule.doc.summary)
        })
        .collect();
    Ok(lines.join("\n") + "\n")
}

/// The rule's full documentation as Markdown, including its active status.
///
/// # Errors
///
/// Returns [`CatalogError::Label`] if no rule is named `name`.
pub fn explain(name: &str) -> Result<String, CatalogError> {
    let rule = find_rule(name)?;
    Ok(render_rule(rule))
}

fn render_rule(rule: &RegisteredRule) -> String {
    let doc = rule.doc;
    let template = rule.template;
    let status_line = match load_rule_status(rule) {
        Ok(status) => format!("Status: {status}"),
        Err(error) => format!("Status: <config error: {error}>"),
    };
    let mut lines = vec![
        format!("# {}", rule.name),
        String::new(),
        status_line,
        String::new(),
        "## What it does".to_owned(),
        String::new(),
        doc.what_it_does.to_owned(),
        String::new(),
        "## Why is this bad?".to_owned(),
        String::new(),
        doc.why_is_this_bad.trim_end().to_owned(),
        String::new(),
        "## Message".to_owned(),
        String::new(),
    ];
    for (label, text) in [
        ("Summary", template.summary),
        ("Rationale", template.rationale),
        ("Suggestion", template.suggestion),
    ] {
        lines.push(format!("- {label}: {}", text.base));
        lines.extend(
            text.overrides
                .iter()
                .map(|(language, override_text)| format!("  - {language}: {override_text}")),
        );
    }
    let mut keys: Vec<&'static str> = doc
        .configuration
        .iter()
        .flat_map(|&shape| shape_keys(shape))
        .copied()
        .collect();
    keys.sort_unstable();
    keys.dedup();
    if !keys.is_empty() {
        lines.extend([
            String::new(),
            "## Configuration".to_owned(),
            String::new(),
            format!(
                "Keys under `[rules.{0}]`, or `[rules.{0}.<language>]` for one language:",
                rule.name
            ),
            String::new(),
        ]);
        lines.extend(keys.iter().map(|key| format!("- `{key}`")));
    }
    if !doc.references.is_empty() {
        lines.extend([String::new(), "## References".to_owned(), String::new()]);
        lines.extend(
            doc.references
                .iter()
                .map(|reference| format!("- [{}]({})", reference.title, reference.url)),
        );
    }
    lines.extend([String::new(), "## Tags".to_owned(), String::new()]);
    lines.extend(
        Facet::iter()
            .map(|facet| (facet, facet_tags(rule, facet)))
            .filter(|(_, tags)| !tags.is_empty())
            .map(|(facet, tags)| format!("- {}: {tags}", facet.label())),
    );
    lines.join("\n") + "\n"
}

/// The rule's tags in `facet`, comma-separated; an ancestor topic names the topic it comes
/// from, e.g. `testing [via test-timing]`.
fn facet_tags(rule: &RegisteredRule, facet: Facet) -> String {
    let mut labels = Vec::new();
    for branch in rule.branches() {
        let Some((leaf, ancestors)) = branch.split_last() else {
            continue;
        };
        if leaf.facet() != facet {
            continue;
        }
        labels.push(leaf.label().to_owned());
        labels.extend(
            ancestors
                .iter()
                .rev()
                .map(|ancestor| format!("{} [via {}]", ancestor.label(), leaf.label())),
        );
    }
    labels.join(", ")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::rule_documentation::{ConfigShape, Reference, RuleDoc};

    /// The style problems of one doc.
    fn doc_problems(doc: &RuleDoc) -> Vec<String> {
        let mut problems = Vec::new();
        let summary = doc.summary;
        if summary.contains('\n')
            || !summary.ends_with('.')
            || summary.trim_end_matches('.').contains(". ")
        {
            problems.push("`summary` must be one sentence on one line, ending with `.`".into());
        }
        let texts = [
            ("summary", summary),
            ("what_it_does", doc.what_it_does),
            ("why_is_this_bad", doc.why_is_this_bad),
        ]
        .into_iter()
        .chain(doc.references.iter().flat_map(|reference| {
            [
                ("reference title", reference.title),
                ("reference url", reference.url),
            ]
        }));
        for (field, text) in texts {
            if text.trim().is_empty() {
                problems.push(format!("`{field}` is empty"));
            }
            if text.lines().any(|line| line.trim_start().starts_with('#')) {
                problems.push(format!(
                    "`{field}` has a `#` heading; the renderer owns headings"
                ));
            }
        }
        problems
    }

    #[test]
    fn documented_rules_follow_the_doc_style() {
        let problems: Vec<String> = registered_rules()
            .iter()
            .filter(|rule| !rule.doc.is_placeholder())
            .flat_map(|rule| {
                doc_problems(&rule.doc)
                    .into_iter()
                    .map(|problem| format!("{}: {problem}", rule.name))
            })
            .collect();
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    const GOOD: RuleDoc = RuleDoc {
        summary: "Flags things.",
        what_it_does: "Flags things when they happen.",
        why_is_this_bad: "Things are bad.",
        configuration: &[ConfigShape::Threshold],
        references: &[Reference {
            title: "Title",
            url: "https://example.com",
        }],
    };

    #[rstest]
    #[case::good(GOOD, &[])]
    #[case::two_sentences(
        RuleDoc { summary: "Flags things. Also others.", ..GOOD },
        &["`summary` must be one sentence on one line, ending with `.`"]
    )]
    #[case::no_period(
        RuleDoc { summary: "Flags things", ..GOOD },
        &["`summary` must be one sentence on one line, ending with `.`"]
    )]
    #[case::empty_why(
        RuleDoc { why_is_this_bad: " ", ..GOOD },
        &["`why_is_this_bad` is empty"]
    )]
    #[case::heading(
        RuleDoc { why_is_this_bad: "Bad.\n\n## Example\n", ..GOOD },
        &["`why_is_this_bad` has a `#` heading; the renderer owns headings"]
    )]
    fn doc_problems_name_each_problem(#[case] doc: RuleDoc, #[case] expected: &[&str]) {
        assert_eq!(doc_problems(&doc), expected);
    }

    #[test]
    fn explain_rejects_unknown_rules() {
        assert_eq!(
            explain("testing").unwrap_err().to_string(),
            "unknown rule `testing`"
        );
    }
}
