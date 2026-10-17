//! Rule selection: resolves `select`, `ignore` and `per-file-ignores` into the tag-free
//! [`RuleName`] sets on [`Config`].
//!
//! Selectors are resolved once, at load, with this precedence: on each of a rule's
//! branches the nearest selector wins, the rule name being the leaf of every branch.
//! When branches disagree, `ignore` wins. With no verdict, the rule is on only if `select` is
//! absent. `per-file-ignores` is a later, subtract-only stage. This is the only module that
//! parses selector strings.

architecture_component!(RuleSelection);

mod taxonomy;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use strum::IntoEnumIterator as _;

pub use self::taxonomy::{Derived, Facet, RegisteredRule, Tag, registered_topics};
use self::taxonomy::{REGISTERED_RULES, Selector};
use crate::config::{CONFIG_FILE_NAME, Config, ContextConfig, compile_glob};
use crate::diagnostic::RuleName;
use crate::rule_declaration::{RuleOptionsError, RuleOverrides};

/// Command-line overrides for configuration loading and rule selection: `--config`, `--select`
/// and `--ignore`.
#[derive(Debug, Default, Clone, clap::Args)]
pub struct ConfigOverrides {
    /// Path to the configuration file (overrides `.omnicoach.toml` discovery)
    #[arg(long = "config", global = true, value_name = "PATH")]
    pub config_path: Option<PathBuf>,

    /// Comma-separated rule or tag selectors to enable (overrides `select` and `ignore` in `.omnicoach.toml`)
    #[arg(long, global = true, value_delimiter = ',', value_name = "SELECTOR")]
    pub select: Option<Vec<String>>,

    /// Comma-separated rule or tag selectors to disable
    #[arg(long, global = true, value_delimiter = ',', value_name = "SELECTOR")]
    pub ignore: Vec<String>,
}

/// Where in the config an entry was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// The `select` list.
    Select,
    /// The `ignore` list.
    Ignore,
    /// A `per-file-ignores` pattern.
    PerFile(String),
}

impl fmt::Display for Location {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Select => formatter.write_str("`select`"),
            Self::Ignore => formatter.write_str("`ignore`"),
            Self::PerFile(pattern) => write!(formatter, "`per-file-ignores.\"{pattern}\"`"),
        }
    }
}

/// Why a config was rejected. The message is what the user sees.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Failed to read the configuration file from disk.
    #[error("failed to read `{path}`: {source}")]
    Io {
        /// Path to the configuration file.
        path: String,
        /// The underlying IO error.
        #[source]
        source: std::io::Error,
    },
    /// Not valid TOML, or a value of the wrong shape.
    #[error(transparent)]
    Toml(#[from] toml::de::Error),
    /// A label that is not a rule, a tag or a synonym.
    #[error(
        "unknown selector `{label}` in {location}{}",
        suggestion.map(|known| format!("; did you mean `{known}`?")).unwrap_or_default()
    )]
    UnknownSelector {
        /// The label as written.
        label: String,
        /// Where it was written.
        location: Location,
        /// The closest registered label.
        suggestion: Option<&'static str>,
    },
    /// A facet label, which is never a selector.
    #[error(
        "`{label}` in {location} is a facet label, not a selector; use one of its values: {values}"
    )]
    FacetLabel {
        /// The label as written.
        label: String,
        /// Where it was written.
        location: Location,
        /// The facet's values, comma-separated.
        values: String,
    },
    /// A canonical selector in both `select` and `ignore`.
    #[error(
        "`{0}` is both selected and ignored (synonyms count as the same selector); \
         remove it from one list"
    )]
    Conflict(&'static str),
    /// A `per-file-ignores` pattern that is not a valid glob.
    #[error("invalid glob `{pattern}` in `per-file-ignores`: {source}")]
    Glob {
        /// The pattern as written.
        pattern: String,
        /// The glob error.
        #[source]
        source: globset::Error,
    },
    /// A `[rules.<name>]` table for a rule that does not exist.
    #[error(
        "`rules.{name}`: unknown rule{}",
        suggestion.map(|known| format!("; did you mean `{known}`?")).unwrap_or_default()
    )]
    UnknownRule {
        /// The rule name as written.
        name: String,
        /// The closest registered rule name.
        suggestion: Option<&'static str>,
    },
    /// A `[rules.<name>]` entry that the rule's declared options reject.
    #[error(transparent)]
    RuleOptions(#[from] RuleOptionsError),
}

/// A label given on the command line that names no rule or tag.
#[derive(Debug, thiserror::Error)]
#[error(
    "unknown {kind} `{label}`{}",
    suggestion.map(|known| format!("; did you mean `{known}`?")).unwrap_or_default()
)]
pub struct UnknownLabel {
    kind: &'static str,
    label: String,
    suggestion: Option<&'static str>,
}

/// Every registered rule, in registry order.
#[must_use]
pub fn registered_rules() -> &'static [RegisteredRule] {
    &REGISTERED_RULES
}

/// The rules that `select = ["<label>"]` would select: a topic includes its subtopics and
/// synonyms resolve.
///
/// # Errors
///
/// Returns [`UnknownLabel`] if `label` is not a rule, a tag or a synonym.
pub fn rules_tagged(label: &str) -> Result<Vec<&'static RegisteredRule>, UnknownLabel> {
    let selector = taxonomy::lookup(label).ok_or_else(|| UnknownLabel {
        kind: "tag",
        label: label.to_owned(),
        suggestion: taxonomy::closest_label(label),
    })?;
    Ok(REGISTERED_RULES
        .iter()
        .filter(|rule| rule.matches(selector))
        .collect())
}

/// The registered rule named `name`.
///
/// # Errors
///
/// Returns [`UnknownLabel`] if no rule has this name.
pub fn find_rule(name: &str) -> Result<&'static RegisteredRule, UnknownLabel> {
    REGISTERED_RULES
        .iter()
        .find(|rule| rule.name.0 == name)
        .ok_or_else(|| UnknownLabel {
            kind: "rule",
            label: name.to_owned(),
            suggestion: taxonomy::closest_rule_name(name),
        })
}

/// Loads `.omnicoach.toml` from the current directory (or `overrides.config_path`), or the
/// default config if `.omnicoach.toml` is absent, and applies `overrides`.
///
/// # Errors
///
/// Returns [`ConfigError`] if the file cannot be read or is rejected by
/// [`parse_config_with_overrides`].
pub fn load_config(overrides: &ConfigOverrides) -> Result<Config, ConfigError> {
    let content = read_config_file(overrides.config_path.as_deref())?;
    parse_config_with_overrides(&content, overrides)
}

/// Whether `rule` is on under `.omnicoach.toml` in the current directory (or
/// `overrides.config_path`) with `overrides` applied, and why.
///
/// # Errors
///
/// Returns [`ConfigError`] if the file cannot be read or is rejected by
/// [`rule_status_with_overrides`].
pub fn load_rule_status(
    rule: &RegisteredRule,
    overrides: &ConfigOverrides,
) -> Result<RuleStatus, ConfigError> {
    let content = read_config_file(overrides.config_path.as_deref())?;
    rule_status_with_overrides(&content, overrides, rule)
}

/// Whether `rule` is on under `config_toml`'s `select` and `ignore`, and why.
///
/// # Errors
///
/// On invalid TOML, unknown selectors, facet labels and conflicts.
pub fn rule_status(config_toml: &str, rule: &RegisteredRule) -> Result<RuleStatus, ConfigError> {
    rule_status_with_overrides(config_toml, &ConfigOverrides::default(), rule)
}

/// Whether `rule` is on under `config_toml` and `overrides`, and why.
///
/// # Errors
///
/// On invalid TOML, unknown selectors, facet labels and conflicts.
pub fn rule_status_with_overrides(
    config_toml: &str,
    overrides: &ConfigOverrides,
    rule: &RegisteredRule,
) -> Result<RuleStatus, ConfigError> {
    let raw: RawConfig = toml::from_str(config_toml)?;
    Ok(parse_plan_with_overrides(raw.select, raw.ignore, overrides)?.decide(rule))
}

/// Parses a TOML config and resolves its selectors into rule names.
///
/// # Errors
///
/// On invalid TOML, unknown selectors, facet labels, conflicts and invalid globs.
pub fn parse_config(config_toml: &str) -> Result<Config, ConfigError> {
    parse_config_with_overrides(config_toml, &ConfigOverrides::default())
}

/// Parses a TOML config, applies `overrides`, and resolves selectors into rule names.
///
/// # Errors
///
/// On invalid TOML, unknown selectors, facet labels, conflicts and invalid globs.
pub fn parse_config_with_overrides(
    config_toml: &str,
    overrides: &ConfigOverrides,
) -> Result<Config, ConfigError> {
    let raw: RawConfig = toml::from_str(config_toml)?;
    let plan = parse_plan_with_overrides(raw.select, raw.ignore, overrides)?;
    let disabled_rules = rule_names(|rule| !plan.decide(rule).enabled());
    let per_file_ignores = raw
        .per_file_ignores
        .into_iter()
        .map(|(pattern, labels)| {
            let selectors = resolve_all(labels, &Location::PerFile(pattern.clone()))?;
            let matcher = compile_glob(&pattern)
                .map_err(|source| ConfigError::Glob { pattern, source })?
                .compile_matcher();
            let rules = rule_names(|rule| selectors.iter().any(|&selector| rule.matches(selector)));
            Ok((matcher, rules))
        })
        .collect::<Result<_, ConfigError>>()?;
    let rule_overrides = raw
        .rules
        .iter()
        .map(|(name, value)| {
            let rule = REGISTERED_RULES
                .iter()
                .find(|rule| rule.name.0 == name)
                .ok_or_else(|| ConfigError::UnknownRule {
                    name: name.clone(),
                    suggestion: taxonomy::closest_rule_name(name),
                })?;
            let overrides = RuleOverrides::parse(name, &rule.options, rule.languages, value)?;
            Ok((rule.name, overrides))
        })
        .collect::<Result<_, ConfigError>>()?;
    Ok(Config {
        disabled_rules,
        rule_overrides,
        context: raw.context,
        per_file_ignores,
    })
}

/// Whether a rule is on under `select` and `ignore`, and the deciding selector (per-file
/// ignores aside). Its `Display` is the one-line explanation shown by `--explain`.
#[derive(Debug)]
pub struct RuleStatus {
    selected_by_default: bool,
    deciding: Option<Decision>,
}

impl RuleStatus {
    /// Whether the rule is on.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.deciding
            .as_ref()
            .map_or(self.selected_by_default, |decision| {
                decision.verdict == Verdict::Select
            })
    }
}

impl fmt::Display for RuleStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = if self.enabled() {
            "enabled"
        } else {
            "disabled"
        };
        let Some(decision) = &self.deciding else {
            let reason = if self.selected_by_default {
                "default"
            } else {
                "not in `select`"
            };
            return write!(formatter, "{state} ({reason})");
        };
        let list = match decision.verdict {
            Verdict::Select => "select",
            Verdict::Ignore => "ignore",
        };
        write!(
            formatter,
            "{state} by `{list} = [\"{}\"]`",
            decision.selector.label()
        )?;
        if matches!(decision.selector, Selector::Tag(_)) && decision.branch.len() > 1 {
            let path: Vec<_> = decision.branch.iter().map(|&tag| tag.label()).collect();
            write!(formatter, " (via {})", path.join(" > "))?;
        }
        Ok(())
    }
}

/// The raw `.omnicoach.toml` table before selector and rule-option validation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RawConfig {
    select: Option<Vec<String>>,
    #[serde(default)]
    ignore: Vec<String>,
    #[serde(default)]
    per_file_ignores: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    rules: toml::Table,
    #[serde(default)]
    context: ContextConfig,
}

fn read_config_file(config_path: Option<&Path>) -> Result<String, ConfigError> {
    if let Some(path) = config_path {
        return std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.display().to_string(),
            source,
        });
    }
    match std::fs::read_to_string(CONFIG_FILE_NAME) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(source) => Err(ConfigError::Io {
            path: CONFIG_FILE_NAME.to_owned(),
            source,
        }),
    }
}

fn rule_names(mut predicate: impl FnMut(&RegisteredRule) -> bool) -> HashSet<RuleName> {
    REGISTERED_RULES
        .iter()
        .filter(|rule| predicate(rule))
        .map(|rule| rule.name)
        .collect()
}

/// Validated `select` and `ignore` sets (`select: None` selects every rule by default).
struct Plan {
    select: Option<BTreeSet<Selector>>,
    ignore: BTreeSet<Selector>,
}

impl Plan {
    /// Model B: nearest selector per branch, then ignore wins across branches.
    fn decide(&self, rule: &RegisteredRule) -> RuleStatus {
        let decisions = rule.branches().into_iter().filter_map(|branch| {
            let (selector, verdict) = std::iter::once(Selector::Rule(rule.name))
                .chain(branch.iter().rev().map(|&tag| Selector::Tag(tag)))
                .find_map(|selector| Some((selector, self.verdict(selector)?)))?;
            Some(Decision {
                verdict,
                selector,
                branch,
            })
        });
        RuleStatus {
            selected_by_default: self.select.is_none(),
            // The first `ignore` decision if any, else the first `select` one.
            deciding: decisions.min_by_key(|decision| decision.verdict == Verdict::Select),
        }
    }

    fn verdict(&self, selector: Selector) -> Option<Verdict> {
        if self.ignore.contains(&selector) {
            Some(Verdict::Ignore)
        } else if self.select.as_ref()?.contains(&selector) {
            Some(Verdict::Select)
        } else {
            None
        }
    }
}

fn parse_plan_with_overrides(
    select: Option<Vec<String>>,
    ignore: Vec<String>,
    overrides: &ConfigOverrides,
) -> Result<Plan, ConfigError> {
    let mut plan = parse_plan(select, ignore)?;
    let cli = parse_plan(overrides.select.clone(), overrides.ignore.clone())?;
    if cli.select.is_some() {
        plan = cli;
    } else if !cli.ignore.is_empty() {
        if let Some(selected) = &mut plan.select {
            for selector in &cli.ignore {
                selected.remove(selector);
            }
        }
        plan.ignore.extend(cli.ignore);
    }
    Ok(plan)
}

fn parse_plan(select: Option<Vec<String>>, ignore: Vec<String>) -> Result<Plan, ConfigError> {
    let select = select
        .map(|labels| resolve_all(labels, &Location::Select))
        .transpose()?;
    let ignore = resolve_all(ignore, &Location::Ignore)?;
    if let Some(&both) = select
        .iter()
        .flatten()
        .find(|&selector| ignore.contains(selector))
    {
        return Err(ConfigError::Conflict(both.label()));
    }
    Ok(Plan { select, ignore })
}

fn resolve_all(
    labels: Vec<String>,
    location: &Location,
) -> Result<BTreeSet<Selector>, ConfigError> {
    labels
        .into_iter()
        .map(|label| resolve(label, location))
        .collect()
}

fn resolve(label: String, location: &Location) -> Result<Selector, ConfigError> {
    if let Some(selector) = taxonomy::lookup(&label) {
        return Ok(selector);
    }
    let location = location.clone();
    match Facet::iter().find(|facet| facet.matches_label(&label)) {
        Some(facet) => Err(ConfigError::FacetLabel {
            label,
            location,
            values: taxonomy::all_tags()
                .filter(|tag| tag.facet() == facet)
                .map(Tag::label)
                .collect::<Vec<_>>()
                .join(", "),
        }),
        None => Err(ConfigError::UnknownSelector {
            suggestion: taxonomy::closest_label(&label),
            label,
            location,
        }),
    }
}

/// The selector that decided a rule's status, on the branch where it was found.
#[derive(Debug)]
struct Decision {
    verdict: Verdict,
    selector: Selector,
    branch: Vec<Tag>,
}

/// What a selector asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Select,
    Ignore,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rstest::rstest;

    use super::*;
    use crate::rule_declaration::{Classification, Consensus, ImpactedQuality, Precision, Topic};

    const PARENT: Topic = Topic {
        label: "parent",
        parent: None,
        description: "",
        scope_note: "",
        synonyms: &[],
    };
    const CHILD: Topic = Topic {
        label: "child",
        parent: Some(&PARENT),
        ..PARENT
    };
    const OTHER: Topic = Topic {
        label: "other",
        ..PARENT
    };

    const RULE: Selector = Selector::Rule(RuleName("rule"));
    const PARENT_TAG: Selector = Selector::Tag(Tag::Topic(PARENT));
    const CHILD_TAG: Selector = Selector::Tag(Tag::Topic(CHILD));
    const OTHER_TAG: Selector = Selector::Tag(Tag::Topic(OTHER));
    const HEURISTIC: Selector = Selector::Tag(Tag::Precision(Precision::Heuristic));

    fn enabled(config: &Config, path: &str) -> BTreeSet<&'static str> {
        REGISTERED_RULES
            .iter()
            .map(|rule| rule.name)
            .filter(|&name| config.is_rule_enabled_for_path(name, Path::new(path)))
            .map(|name| name.0)
            .collect()
    }

    fn all_except(excluded: &[&str]) -> BTreeSet<&'static str> {
        REGISTERED_RULES
            .iter()
            .map(|rule| rule.name.0)
            .filter(|name| !excluded.contains(name))
            .collect()
    }

    /// The status of `rule`, on the branches `parent > child`, `other` and `heuristic`.
    #[rstest]
    #[case::default_on_without_select(None, &[], "enabled (default)")]
    #[case::default_off_with_select(Some(vec![]), &[], "disabled (not in `select`)")]
    #[case::ignored_without_select(None, &[OTHER_TAG], "disabled by `ignore = [\"other\"]`")]
    #[case::ancestor_selected(
        Some(vec![PARENT_TAG]),
        &[],
        "enabled by `select = [\"parent\"]` (via parent > child)"
    )]
    #[case::nearest_select_on_a_branch_wins(
        Some(vec![CHILD_TAG]),
        &[PARENT_TAG],
        "enabled by `select = [\"child\"]` (via parent > child)"
    )]
    #[case::nearest_ignore_on_a_branch_wins(
        Some(vec![PARENT_TAG]),
        &[CHILD_TAG],
        "disabled by `ignore = [\"child\"]` (via parent > child)"
    )]
    #[case::ignore_wins_across_topics(
        Some(vec![CHILD_TAG]),
        &[OTHER_TAG],
        "disabled by `ignore = [\"other\"]`"
    )]
    #[case::ignore_wins_across_facets(
        Some(vec![HEURISTIC]),
        &[PARENT_TAG],
        "disabled by `ignore = [\"parent\"]` (via parent > child)"
    )]
    #[case::selected_rule_name_beats_tags(
        Some(vec![RULE]),
        &[PARENT_TAG, OTHER_TAG, HEURISTIC],
        "enabled by `select = [\"rule\"]`"
    )]
    #[case::ignored_rule_name_beats_tags(
        Some(vec![PARENT_TAG, OTHER_TAG, HEURISTIC]),
        &[RULE],
        "disabled by `ignore = [\"rule\"]`"
    )]
    fn the_nearest_selector_decides_and_ignore_wins_across_branches(
        #[case] select: Option<Vec<Selector>>,
        #[case] ignore: &[Selector],
        #[case] expected: &str,
    ) {
        let rule = RegisteredRule::synthetic(
            "rule",
            Classification {
                topics: &[CHILD, OTHER],
                precision: Precision::Heuristic,
                consensus: Consensus::Opinionated,
                impacted_quality: ImpactedQuality::Maintainability,
            },
        );
        let plan = Plan {
            select: select.map(|selectors| selectors.into_iter().collect()),
            ignore: ignore.iter().copied().collect(),
        };
        assert_eq!(plan.decide(&rule).to_string(), expected);
    }

    #[test]
    fn per_file_ignores_remove_only_on_matching_paths() {
        let heuristic: Vec<_> = rules_tagged("heuristic")
            .unwrap()
            .iter()
            .map(|rule| rule.name.0)
            .collect();
        let config = parse_config("[per-file-ignores]\n\"tests/**\" = [\"heuristic\"]").unwrap();
        assert_eq!(
            enabled(&config, "tests/test_module.py"),
            all_except(&heuristic)
        );
        assert_eq!(enabled(&config, "src/module.py"), all_except(&[]));
    }

    #[test]
    fn per_file_ignores_apply_after_selection_by_rule_name() {
        let rule = rules_tagged("heuristic").unwrap()[0].name.0;
        let config = parse_config(&format!(
            "select = [\"{rule}\"]\n[per-file-ignores]\n\"tests/**\" = [\"heuristic\"]"
        ))
        .unwrap();
        assert!(enabled(&config, "tests/test_module.py").is_empty());
        assert_eq!(enabled(&config, "src/module.py"), BTreeSet::from([rule]));
    }

    #[rstest]
    #[case::typo_topic(
        r#"select = ["tesing"]"#,
        "unknown selector `tesing` in `select`; did you mean `testing`?"
    )]
    #[case::typo_rule(
        r#"select = ["sleep-in-test"]"#,
        "unknown selector `sleep-in-test` in `select`; did you mean `sleep-in-tests`?"
    )]
    #[case::wrong_case(
        r#"ignore = ["Testing"]"#,
        "unknown selector `Testing` in `ignore`; did you mean `testing`?"
    )]
    #[case::conflict(
        "select = [\"jj\"]\nignore = [\"jj\"]",
        "`jj` is both selected and ignored (synonyms count as the same selector); remove it \
         from one list"
    )]
    #[case::synonym_conflict(
        "select = [\"jujutsu\"]\nignore = [\"jj\"]",
        "`jj` is both selected and ignored (synonyms count as the same selector); remove it \
         from one list"
    )]
    #[case::facet_label(
        r#"select = ["precision"]"#,
        "`precision` in `select` is a facet label, not a selector; use one of its values: \
         exact, heuristic"
    )]
    #[case::multi_word_facet_label(
        r#"select = ["Impacted quality"]"#,
        "`Impacted quality` in `select` is a facet label, not a selector; use one of its \
         values: reliability, maintainability"
    )]
    #[case::typo_in_per_file(
        "[per-file-ignores]\n\"tests/**\" = [\"heurstic\"]",
        "unknown selector `heurstic` in `per-file-ignores.\"tests/**\"`; did you mean \
         `heuristic`?"
    )]
    #[case::invalid_glob(
        "[per-file-ignores]\n\"src/[\" = [\"testing\"]",
        "invalid glob `src/[` in `per-file-ignores`: "
    )]
    #[case::unknown_rule(
        "[rules.too-many-assertion]\nmax-assertions = 3",
        "`rules.too-many-assertion`: unknown rule; did you mean `too-many-assertions`?"
    )]
    #[case::unknown_key(
        "[rules.too-many-assertions]\nmax = 3",
        "`rules.too-many-assertions.max`: unknown key; expected one of `enforcement-mode`, \
         `max-assertions`, `python`, `rust`"
    )]
    #[case::wrong_type(
        "[rules.too-many-assertions.rust]\nmax-assertions = \"5\"",
        "`rules.too-many-assertions.rust.max-assertions`: expected a non-negative integer, \
         found \"5\""
    )]
    #[case::unknown_enforcement_mode(
        "[rules.type-cast]\nenforcement-mode = \"warn\"",
        "`rules.type-cast.enforcement-mode`: expected `ban` or `require-explanation`, \
         found \"warn\""
    )]
    #[case::unsupported_language(
        "[rules.type-cast.rust]\nbanned = []",
        "`rules.type-cast.rust`: this rule does not analyze this language; it analyzes \
         python"
    )]
    #[case::enforcement_mode_on_audit(
        "[rules.unused-suppression]\nenforcement-mode = \"ban\"",
        "`rules.unused-suppression.enforcement-mode`: this rule has no enforcement mode; it \
         always reports every finding"
    )]
    #[case::enforcement_mode_on_command_rule(
        "[rules.edit-of-described-commit]\nenforcement-mode = \"ban\"",
        "`rules.edit-of-described-commit.enforcement-mode`: this rule has no enforcement \
         mode; it always reports every finding"
    )]
    #[case::unknown_top_level_key(
        r#"selct = ["testing"]"#,
        "TOML parse error at line 1, column 1\n  |\n1 | selct = [\"testing\"]\n  | ^^^^^\nunknown field `selct`, expected one of `select`, `ignore`, `per-file-ignores`, `rules`, `context`"
    )]
    #[case::unknown_context_key(
        "[context]\ntest-paterns = []",
        "TOML parse error at line 2, column 1\n  |\n2 | test-paterns = []\n  | ^^^^^^^^^^^^\nunknown field `test-paterns`, expected `test-patterns`"
    )]
    fn invalid_configs_are_rejected_loudly(#[case] config_toml: &str, #[case] message: &str) {
        let error = parse_config(config_toml).unwrap_err().to_string();
        assert!(error.starts_with(message), "{error}");
    }

    #[test]
    fn valid_rule_options_are_stored_per_rule() {
        let config = parse_config(
            "[rules.too-many-assertions]\nmax-assertions = 6\n\
             [rules.too-many-assertions.rust]\nmax-assertions = 8\n\
             [rules.abbreviated-name]\nextend-banned = [\"ctx\"]",
        )
        .unwrap();
        let mut configured: Vec<_> = config.rule_overrides.keys().map(|name| name.0).collect();
        configured.sort_unstable();
        assert_eq!(configured, ["abbreviated-name", "too-many-assertions"]);
    }

    #[rstest]
    #[case::cli_select_replaces_toml_select_and_ignore(
        "select = [\"nested-function\"]\nignore = [\"sleep-in-tests\"]",
        Some(vec!["testing".to_owned()]),
        vec![],
        "sleep-in-tests",
        "enabled by `select = [\"testing\"]` (via testing > test-timing)"
    )]
    #[case::cli_select_discards_toml_select_for_unselected_rules(
        "select = [\"nested-function\"]",
        Some(vec!["testing".to_owned()]),
        vec![],
        "nested-function",
        "disabled (not in `select`)"
    )]
    #[case::cli_select_and_cli_ignore_compose(
        "select = [\"nested-function\"]",
        Some(vec!["testing".to_owned()]),
        vec!["sleep-in-tests".to_owned()],
        "sleep-in-tests",
        "disabled by `ignore = [\"sleep-in-tests\"]`"
    )]
    #[case::cli_ignore_without_select_extends_toml_and_removes_from_toml_select(
        "select = [\"sleep-in-tests\", \"nested-function\"]",
        None,
        vec!["sleep-in-tests".to_owned()],
        "sleep-in-tests",
        "disabled by `ignore = [\"sleep-in-tests\"]`"
    )]
    fn cli_overrides_resolve_precedence(
        #[case] config_toml: &str,
        #[case] select: Option<Vec<String>>,
        #[case] ignore: Vec<String>,
        #[case] rule_name: &str,
        #[case] expected: &str,
    ) {
        let rule = find_rule(rule_name).unwrap();
        let overrides = ConfigOverrides {
            config_path: None,
            select,
            ignore,
        };
        let status = rule_status_with_overrides(config_toml, &overrides, rule).unwrap();
        assert_eq!(status.to_string(), expected);
    }

    #[test]
    fn cli_select_and_ignore_conflict_is_rejected() {
        let overrides = ConfigOverrides {
            config_path: None,
            select: Some(vec!["jujutsu".to_owned()]),
            ignore: vec!["jj".to_owned()],
        };
        let error = parse_config_with_overrides("", &overrides)
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "`jj` is both selected and ignored (synonyms count as the same selector); remove it \
             from one list"
        );
    }
}
