//! Rule catalog: renders the rule list and one rule's documentation as plain Markdown.

architecture_component!(RuleCatalog);

use strum::IntoEnumIterator as _;

use crate::diagnostic::Diagnostic;
use crate::rule_declaration::{
    Consensus, DeclaredExamples, DeclaredOptions, EnforcementMode, FilterListDefaults,
    LanguageDefaults, OptionSpec, Precision,
};
use crate::rule_selection::{
    ConfigError, ConfigOverrides, Facet, RegisteredRule, RuleStatus, UnknownLabel, find_rule,
    load_rule_status, registered_rules, registered_topics, rule_status, rules_tagged,
};

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

/// Command-line flags for rule discovery: `--list-rules`, `--tag`, `--list-tags` and `--explain`.
#[derive(Debug, clap::Args)]
pub struct DiscoveryArgs {
    /// List every rule with its languages and summary, as Markdown
    #[arg(long, global = true)]
    pub list_rules: bool,

    /// With --list-rules, keep only the rules a `select` label selects (tag, synonym or rule)
    #[arg(long, global = true, value_name = "LABEL", requires = "list_rules")]
    pub tag: Option<String>,

    /// List the topic tree with each topic's parent, synonyms, description and scope note
    #[arg(long, global = true, conflicts_with_all = ["list_rules", "explain"])]
    pub list_tags: bool,

    /// Print one rule's documentation as Markdown
    #[arg(long, global = true, value_name = "RULE", conflicts_with_all = ["list_rules", "list_tags"])]
    pub explain: Option<String>,
}

impl DiscoveryArgs {
    /// Renders the requested discovery output, or `None` when no discovery flag is given.
    #[must_use]
    pub fn render(&self, overrides: &ConfigOverrides) -> Option<Result<String, CatalogError>> {
        if let Some(name) = &self.explain {
            return Some(explain_with_overrides(name, overrides));
        }
        if self.list_tags {
            return Some(Ok(list_tags()));
        }
        self.list_rules.then(|| list_rules(self.tag.as_deref()))
    }
}

/// The footer note on how to fix findings, printed after every plain `code` report.
const FIX_NOTE: &str = "Note: Fix each finding by applying its suggestion. Do not reshape the \
                        code or change its values only to fall under a rule's limit or outside \
                        its pattern.";

/// The footer note on suppressing findings, printed when a reported rule is `heuristic` or
/// `opinionated`.
const SUPPRESSION_NOTE: &str = "Note: Some rules use heuristics. When a finding conflicts with \
                                the clearest, most robust design, suppress it with \
                                `omni:ignore [<rule>] -- <reason>` rather than working around \
                                the linter and degrading the design.";

/// The footer printed after plain `code` diagnostics: [`FIX_NOTE`], [`SUPPRESSION_NOTE`] when
/// any reported rule is `heuristic` or `opinionated`, and the `--explain` pointer.
#[must_use]
pub fn code_report_footer(binary: &str, diagnostics: &[Diagnostic]) -> String {
    let has_heuristic_or_opinionated = diagnostics.iter().any(|diagnostic| {
        find_rule(diagnostic.rule_name.0).is_ok_and(|rule| {
            rule.classification.precision == Precision::Heuristic
                || rule.classification.consensus == Consensus::Opinionated
        })
    });
    let explain = explain_footer(binary);
    let mut lines = vec![FIX_NOTE];
    if has_heuristic_or_opinionated {
        lines.push(SUPPRESSION_NOTE);
    }
    lines.push(&explain);
    lines.join("\n")
}

/// The line printed after plain diagnostics, pointing at `--explain`.
#[must_use]
pub fn explain_footer(binary: &str) -> String {
    format!("For details on a rule (including known limitations), run: {binary} --explain <rule>")
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

/// The topic tree as a Markdown table: each topic's label, parent, synonyms, description and
/// scope note, with children listed under their parent.
#[must_use]
pub fn list_tags() -> String {
    let mut lines = vec![
        "| Topic | Parent | Synonyms | Description | Scope note |".to_owned(),
        "|---|---|---|---|---|".to_owned(),
    ];
    for topic in registered_topics() {
        let parent = topic.parent.map_or("—", |parent| parent.label);
        let synonyms: Vec<String> = topic
            .synonyms
            .iter()
            .map(|synonym| format!("`{synonym}`"))
            .collect();
        lines.push(format!(
            "| `{}` | {parent} | {} | {} | {} |",
            topic.label,
            synonyms.join(", "),
            topic.description,
            topic.scope_note,
        ));
    }
    lines.join("\n") + "\n"
}

/// The rule's full documentation as Markdown, including its active status in the working directory.
///
/// # Errors
///
/// Returns [`CatalogError::Label`] if no rule is named `name`.
pub fn explain(name: &str) -> Result<String, CatalogError> {
    explain_with_overrides(name, &ConfigOverrides::default())
}

/// The rule's full documentation as Markdown, evaluating its active status in the working
/// directory (or `overrides.config_path`) with `overrides` applied.
///
/// # Errors
///
/// Returns [`CatalogError::Label`] if no rule is named `name`.
pub fn explain_with_overrides(
    name: &str,
    overrides: &ConfigOverrides,
) -> Result<String, CatalogError> {
    let rule = find_rule(name)?;
    Ok(render_rule(rule, &load_rule_status(rule, overrides)))
}

/// The rule's full documentation as Markdown, evaluating its active status against `config_toml`.
///
/// # Errors
///
/// Returns [`CatalogError::Label`] if no rule is named `name`.
pub fn explain_with_config(name: &str, config_toml: &str) -> Result<String, CatalogError> {
    let rule = find_rule(name)?;
    Ok(render_rule(rule, &rule_status(config_toml, rule)))
}

fn render_rule(rule: &RegisteredRule, status_result: &Result<RuleStatus, ConfigError>) -> String {
    let doc = rule.doc;
    let template = rule.template;
    let status_line = match status_result {
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
    ];
    if let Some(known_problems) = doc.known_problems {
        lines.extend([
            "## Known problems".to_owned(),
            String::new(),
            known_problems.to_owned(),
            String::new(),
        ]);
    }
    lines.extend(example_lines(doc.examples));
    lines.extend(["## Message".to_owned(), String::new()]);
    for (label, text) in [
        ("Summary", template.summary),
        ("Rationale", template.rationale),
        ("Suggestion", template.suggestion),
    ] {
        lines.push(text.base.map_or_else(
            || format!("- {label}:"),
            |base| format!("- {label}: {base}"),
        ));
        lines.extend(
            text.overrides
                .iter()
                .map(|(language, override_text)| format!("  - {language}: {override_text}")),
        );
    }
    let configuration = configuration_lines(&rule.options);
    if !configuration.is_empty() {
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
        lines.extend(configuration);
        if let Some(toml_block) = rule.options.default_toml(rule.name, rule.languages) {
            lines.extend([
                String::new(),
                "```toml".to_owned(),
                toml_block.trim_end().to_owned(),
                "```".to_owned(),
            ]);
        }
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

fn example_lines(examples: DeclaredExamples) -> Vec<String> {
    if examples.is_empty() {
        return Vec::new();
    }
    let mut lines = vec!["## Example".to_owned(), String::new()];
    let example_blocks: Vec<(&str, &str, &str)> = match examples {
        DeclaredExamples::Code(items) => items
            .iter()
            .map(|example| (example.language.as_str(), example.flagged, example.fixed))
            .collect(),
        DeclaredExamples::Command(items) => items
            .iter()
            .map(|example| ("sh", example.flagged, example.fixed))
            .collect(),
    };
    for (fence_lang, flagged, fixed) in example_blocks {
        let fence = format!("```{fence_lang}");
        lines.extend([
            fence.clone(),
            flagged.trim_end().to_owned(),
            "```".to_owned(),
            String::new(),
            "Use instead:".to_owned(),
            String::new(),
            fence,
            fixed.trim_end().to_owned(),
            "```".to_owned(),
            String::new(),
        ]);
    }
    lines
}

/// One bullet per accepted key: its type, default and meaning, as declared by the rule.
fn configuration_lines(options: &DeclaredOptions) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(default) = options.enforcement_mode {
        let modes: Vec<&str> = EnforcementMode::iter().map(Into::into).collect();
        lines.push(format!(
            "- `{}` (`{}`, default {}): {}",
            EnforcementMode::KEY,
            modes.join("` or `"),
            per_language(default, |mode| format!("`{}`", <&str>::from(mode))),
            EnforcementMode::DOC
        ));
    }
    for option in &options.options {
        match option {
            OptionSpec::Count(count) => lines.push(format!(
                "- `{}` (integer, default {}): {}",
                count.key,
                per_language(count.default, |value| value.to_string()),
                count.doc
            )),
            OptionSpec::List(list) => lines.extend([
                format!(
                    "- `{}` (list of strings, default {}): {}",
                    list.kind.replace_key(),
                    list_default(&list.default),
                    list.doc
                ),
                format!(
                    "- `{}` (list of strings): items added to the default.",
                    list.kind.extend_key()
                ),
                format!(
                    "- `{}` (list of strings): items removed from the default.",
                    list.kind.remove_key()
                ),
            ]),
        }
    }
    lines
}

/// A default and its per-language overrides, e.g. `4, rust: 6`.
fn per_language<T: Copy>(defaults: LanguageDefaults<T>, render: impl Fn(T) -> String) -> String {
    let overrides = defaults
        .overrides
        .iter()
        .map(|&(language, value)| format!(", {}: {}", language.as_str(), render(value)));
    std::iter::once(render(defaults.base))
        .chain(overrides)
        .collect()
}

/// A list default and its per-language changes, e.g. `` `a`, `b`; rust removes `b` ``.
fn list_default(defaults: &FilterListDefaults) -> String {
    let quoted = |items: &[&str]| {
        let items: Vec<String> = items.iter().map(|item| format!("`{item}`")).collect();
        items.join(", ")
    };
    let base = if defaults.base.is_empty() {
        "empty".to_owned()
    } else {
        quoted(defaults.base)
    };
    let added = defaults
        .extend
        .iter()
        .map(|&(language, items)| format!("{} adds {}", language.as_str(), quoted(items)));
    let removed = defaults
        .remove
        .iter()
        .map(|&(language, items)| format!("{} removes {}", language.as_str(), quoted(items)));
    let parts: Vec<String> = std::iter::once(base).chain(added).chain(removed).collect();
    parts.join("; ")
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
    use crate::rule_declaration::{Reference, RuleDoc};

    /// The style problems of one doc.
    fn doc_problems<Examples>(doc: &RuleDoc<Examples>) -> Vec<String> {
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
        .chain(doc.known_problems.map(|text| ("known_problems", text)))
        .chain(doc.references.iter().flat_map(|reference| {
            [
                ("reference title", reference.title),
                ("reference url", reference.url),
            ]
        }));
        for (field, text) in texts {
            if text.trim().is_empty() {
                problems.push(format!("`{field}` is empty"));
            } else if text.trim() != text {
                problems.push(format!(
                    "`{field}` has leading or trailing whitespace; close the `indoc!` string on \
                     its last line"
                ));
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
        known_problems: None,
        references: &[Reference {
            title: "Title",
            url: "https://example.com",
        }],
        examples: &[],
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
        RuleDoc { why_is_this_bad: indoc::indoc! {r"
            Bad.

            ## Example"}, ..GOOD },
        &["`why_is_this_bad` has a `#` heading; the renderer owns headings"]
    )]
    #[case::empty_known_problems(
        RuleDoc { known_problems: Some(" "), ..GOOD },
        &["`known_problems` is empty"]
    )]
    #[case::trailing_newline(
        RuleDoc { what_it_does: "Flags things.\n", ..GOOD },
        &["`what_it_does` has leading or trailing whitespace; close the `indoc!` string on its last line"]
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

    fn configuration_toml_block(rendered: &str) -> Option<&str> {
        let (_, after_heading) = rendered.split_once("\n## Configuration\n")?;
        let section = after_heading.split("\n## ").next().unwrap_or(after_heading);
        let (_, after_fence) = section
            .split_once("\n```toml\n")
            .expect("`## Configuration` section must contain a ```toml block");
        let (block, _) = after_fence
            .split_once("\n```\n")
            .expect("```toml block must be closed");
        Some(block)
    }

    type ResolvedLanguageOptions = (
        crate::diagnostic::Language,
        EnforcementMode,
        Vec<usize>,
        Vec<std::collections::BTreeSet<String>>,
    );

    fn resolved_rule_options(
        rule: &RegisteredRule,
        overrides: Option<&crate::rule_declaration::RuleOverrides>,
    ) -> Option<Vec<ResolvedLanguageOptions>> {
        use crate::rule_declaration::OptionsDeclaration as _;

        if rule.options.keys().is_empty() {
            return None;
        }
        Some(
            rule.languages
                .iter()
                .map(|&language| {
                    let mode = rule.options.enforcement_mode(language, overrides);
                    let mut counts = Vec::new();
                    let mut lists = Vec::new();
                    for option in &rule.options.options {
                        match option {
                            OptionSpec::Count(count) => {
                                counts.push(count.resolve(language, overrides));
                            }
                            OptionSpec::List(list) => {
                                lists.push(list.resolve(language, overrides).into_iter().collect());
                            }
                        }
                    }
                    (language, mode, counts, lists)
                })
                .collect(),
        )
    }

    #[test]
    fn explain_toml_block_round_trips_to_default_options_for_every_rule() {
        use crate::rule_selection::parse_config;

        // omni:ignore[logic-in-tests] -- verifies every registered rule in the catalog
        for rule in registered_rules() {
            let rendered = explain(rule.name.0).unwrap();
            let round_tripped = configuration_toml_block(&rendered).map(|toml_block| {
                let config = parse_config(toml_block).unwrap_or_else(|error| {
                    panic!(
                        indoc::indoc! {"
                            Rule {} rendered invalid TOML block:
                            {}
                            Error: {}"},
                        rule.name, toml_block, error
                    )
                });
                let overrides = config.rule_overrides.get(&rule.name).unwrap_or_else(|| {
                    panic!(
                        "Rule {} TOML block did not populate `rule_overrides`",
                        rule.name
                    )
                });
                resolved_rule_options(rule, Some(overrides)).unwrap()
            });
            assert_eq!(
                round_tripped,
                resolved_rule_options(rule, None),
                "Rule {} `--explain` TOML block did not round-trip to its declared defaults",
                rule.name
            );
        }
    }

    #[rstest]
    #[case::exact_unopinionated("sleep-in-tests", false)]
    #[case::exact_opinionated("logic-in-tests", true)]
    #[case::heuristic_opinionated("multi-phase-test", true)]
    fn code_report_footer_includes_note_only_for_heuristic_or_opinionated_rules(
        #[case] rule_name: &'static str,
        #[case] expects_note: bool,
    ) {
        use crate::diagnostic::{
            LineColumn, RuleName, SourceLocation, SourceSpan, ViolationMessage,
        };

        let diagnostic = Diagnostic::new(
            RuleName(rule_name),
            ViolationMessage::new("Summary.", "Rationale.", "Suggestion."),
            SourceLocation::file_span(
                "test.py",
                SourceSpan::new(0, 1),
                LineColumn { line: 1, column: 1 },
            ),
        );
        let footer = code_report_footer("omnicoach", &[diagnostic]);
        assert!(footer.starts_with(FIX_NOTE), "{footer}");
        assert_eq!(footer.contains(SUPPRESSION_NOTE), expects_note, "{footer}");
        assert!(
            footer.ends_with(
                "For details on a rule (including known limitations), run: omnicoach --explain <rule>"
            ),
            "{footer}"
        );
    }
}
