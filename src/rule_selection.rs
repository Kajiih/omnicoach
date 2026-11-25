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
use tagonomy::{Catalog, ItemId, LabelError, Reason, Selection, Selector, Verdict};

use self::taxonomy::CATALOG;
pub use self::taxonomy::{AnalyzedInput, RegisteredRule, registered_topics};
use crate::code_lint::project::{
    ArchitectureConfig, ArchitectureConfigError, CONFIG_FILE_NAME, ContextConfig, ProjectConfig,
    compile_glob,
};
use crate::config::Config;
use crate::diagnostic::RuleName;
use crate::diff::detect_vcs_at;
use crate::paths::{lexical_normalize, resolve_against_cwd};
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
        suggestion.map_or_default(|known| format!("; did you mean `{known}`?"))
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
        suggestion.map_or_default(|known| format!("; did you mean `{known}`?"))
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
    /// An `[architecture]` table with duplicate or out-of-order component dependencies.
    #[error(transparent)]
    Architecture(#[from] ArchitectureConfigError),
}

/// A label given on the command line that names no rule or tag.
#[derive(Debug, thiserror::Error)]
#[error(
    "unknown {kind} `{label}`{}",
    suggestion.map_or_default(|known| format!("; did you mean `{known}`?"))
)]
pub struct UnknownLabel {
    kind: &'static str,
    label: String,
    suggestion: Option<&'static str>,
}

/// Every registered rule, in registry order.
#[must_use]
pub fn registered_rules() -> &'static [RegisteredRule] {
    CATALOG.values()
}

/// The rules that `select = ["<label>"]` would select: a topic includes its subtopics and
/// synonyms resolve.
///
/// # Errors
///
/// Returns [`UnknownLabel`] if `label` is not a rule, a tag or a synonym.
pub fn rules_tagged(label: &str) -> Result<Vec<&'static RegisteredRule>, UnknownLabel> {
    let selector = CATALOG.resolve(label).map_err(|_| UnknownLabel {
        kind: "tag",
        label: label.to_owned(),
        suggestion: taxonomy::closest_label(label),
    })?;
    Ok(CATALOG
        .items()
        .filter(|&item| CATALOG.selects(selector, item))
        .map(|item| &CATALOG[item])
        .collect())
}

/// The registered rule named `name`.
///
/// # Errors
///
/// Returns [`UnknownLabel`] if no rule has this name.
pub fn find_rule(name: &str) -> Result<&'static RegisteredRule, UnknownLabel> {
    CATALOG
        .item(name)
        .map(|item| &CATALOG[item])
        .ok_or_else(|| UnknownLabel {
            kind: "rule",
            label: name.to_owned(),
            suggestion: taxonomy::closest_rule_name(name),
        })
}

/// Loads configuration for the code analysis domain starting from the current directory (or
/// `overrides.config_path`), falling back to global configuration if absent, and applies `overrides`.
///
/// # Errors
///
/// Returns [`ConfigError`] if the discovered configuration file cannot be read or parsed.
pub fn load_config(overrides: &ConfigOverrides) -> Result<Config, ConfigError> {
    let start_directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    load_domain_config(&start_directory, overrides, AnalyzedInput::Code)
}

/// Loads configuration for the code analysis domain for explicit target `paths`.
///
/// When `overrides.config_path` is unset and all existing target `paths` share a single discovered
/// project root (or directory when outside any VCS/manifest root), discovers configuration from
/// that root so `Config` and `ProjectIndex` agree on repository boundaries and never leak the
/// caller's `cwd` `.omnicoach.toml` onto another repository.
///
/// # Errors
///
/// Returns [`ConfigError`] if the discovered configuration file cannot be read or parsed.
pub fn load_config_for_paths(
    paths: &[PathBuf],
    overrides: &ConfigOverrides,
) -> Result<Config, ConfigError> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let start_directory = if overrides.config_path.is_some() {
        cwd
    } else {
        resolve_target_config_start_directory(&cwd, paths)
    };
    load_domain_config(&start_directory, overrides, AnalyzedInput::Code)
}

/// Loads configuration for the command workflow safety domain.
///
/// Starts from the current directory (or `overrides.config_path`), prioritizing
/// user-global configuration over repository configuration, and applies `overrides`.
///
/// # Errors
///
/// Returns [`ConfigError`] if the discovered configuration file cannot be read or parsed.
pub fn load_command_config(overrides: &ConfigOverrides) -> Result<Config, ConfigError> {
    let start_directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    load_domain_config(&start_directory, overrides, AnalyzedInput::Command)
}

/// Loads configuration for the code analysis domain starting from `project_root`.
///
/// Discovers ancestor `.omnicoach.toml` up to the VCS root or falls back to user-global
/// configuration, and applies `overrides`.
///
/// # Errors
///
/// Returns [`ConfigError`] if the discovered config file cannot be read or parsed.
pub(crate) fn load_config_from_root(
    project_root: &Path,
    overrides: &ConfigOverrides,
) -> Result<Config, ConfigError> {
    load_domain_config(project_root, overrides, AnalyzedInput::Code)
}

/// Loads configuration for the command workflow safety domain starting from `project_root`.
///
/// Prioritizes user-global configuration over repository configuration, and applies `overrides`.
///
/// # Errors
///
/// Returns [`ConfigError`] if the discovered config file cannot be read or parsed.
pub(crate) fn load_command_config_from_root(
    project_root: &Path,
    overrides: &ConfigOverrides,
) -> Result<Config, ConfigError> {
    load_domain_config(project_root, overrides, AnalyzedInput::Command)
}

/// Discovers the project root directory starting at `start_directory` and traversing up parent directories.
///
/// Prefers `.omnicoach.toml`, `.jj`, or `.git`, falling back to `pyproject.toml` or `Cargo.toml`
/// when no configuration file or VCS marker is present.
#[must_use]
pub(crate) fn discover_project_root(start_directory: &Path) -> Option<PathBuf> {
    const MANIFEST_MARKERS: &[&str] = &["pyproject.toml", "Cargo.toml"];
    start_directory
        .ancestors()
        .find(|ancestor| {
            !ancestor.as_os_str().is_empty()
                && (ancestor.join(CONFIG_FILE_NAME).is_file() || detect_vcs_at(ancestor).is_some())
        })
        .or_else(|| {
            start_directory.ancestors().find(|ancestor| {
                !ancestor.as_os_str().is_empty()
                    && MANIFEST_MARKERS
                        .iter()
                        .any(|marker| ancestor.join(marker).is_file())
            })
        })
        .map(Path::to_path_buf)
}

/// Whether `rule` is on under resolved configuration in the current directory (or
/// `overrides.config_path`) with `overrides` applied, and why.
///
/// Uses command-domain precedence for command rules and code-domain precedence for code rules.
///
/// # Errors
///
/// Returns [`ConfigError`] if the file cannot be read or is rejected by
/// [`rule_status_with_overrides`].
pub fn load_rule_status(
    rule: &RegisteredRule,
    overrides: &ConfigOverrides,
) -> Result<RuleStatus, ConfigError> {
    let start_directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let resolved = resolve_domain_config(&start_directory, overrides, rule.input)?;
    rule_status_with_overrides(&resolved.content, overrides, rule)
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
    let file = parse_selection(raw.select.as_deref(), &raw.ignore)?;
    let selection = apply_overrides(file.selection, overrides)?;
    Ok(status(&CATALOG, &selection, rule.item_id()))
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
    if let Some(ref architecture) = raw.architecture {
        architecture.validate()?;
    }
    let file = parse_selection(raw.select.as_deref(), &raw.ignore)?;
    let warnings = ineffective_selector_warnings(file.duplicates, &file.selection);
    let selection = apply_overrides(file.selection, overrides)?;
    let disabled_rules = rule_names(|item| !selection.matches(item));
    let per_file_ignores = raw
        .per_file_ignores
        .into_iter()
        .map(|(pattern, labels)| {
            let selectors = resolve_all(&labels, &Location::PerFile(pattern.clone()))?;
            let matcher = compile_glob(&pattern)
                .map_err(|source| ConfigError::Glob { pattern, source })?
                .compile_matcher();
            let rules = rule_names(|item| {
                selectors
                    .iter()
                    .any(|&selector| CATALOG.selects(selector, item))
            });
            Ok((matcher, rules))
        })
        .collect::<Result<_, ConfigError>>()?;
    let rule_overrides = raw
        .rules
        .iter()
        .map(|(name, value)| {
            let rule = CATALOG
                .item(name)
                .map(|item| &CATALOG[item])
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
        project: ProjectConfig {
            context: raw.context,
            root_dir: None,
            architecture: raw.architecture,
        },
        per_file_ignores,
        warnings,
    })
}

/// Whether a rule is on under `select` and `ignore`, and the deciding selector (per-file
/// ignores aside). Its `Display` is the one-line explanation shown by `--explain`.
#[derive(Debug)]
pub struct RuleStatus {
    enabled: bool,
    deciding: Option<Deciding>,
}

impl RuleStatus {
    /// Whether the rule is on.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
}

impl fmt::Display for RuleStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = if self.enabled { "enabled" } else { "disabled" };
        let Some(deciding) = &self.deciding else {
            let reason = if self.enabled {
                "default"
            } else {
                "not in `select`"
            };
            return write!(formatter, "{state} ({reason})");
        };
        let list = match deciding.verdict {
            Verdict::Include => "select",
            Verdict::Exclude => "ignore",
        };
        write!(
            formatter,
            "{state} by `{list} = [\"{}\"]`",
            deciding.selector
        )?;
        if deciding.branch.len() > 1 {
            write!(formatter, " (via {})", deciding.branch.join(" > "))?;
        }
        Ok(())
    }
}

const GLOBAL_CONFIG_RELATIVE_PATH: &str = "omnicoach/config.toml";
const DEFAULT_CONFIG_DIR_NAME: &str = ".config";

fn resolve_target_config_start_directory(cwd: &Path, paths: &[PathBuf]) -> PathBuf {
    let roots: Option<Vec<PathBuf>> = paths
        .iter()
        .map(|path| {
            let directory = existing_target_directory(cwd, path)?;
            Some(discover_project_root(&directory).unwrap_or(directory))
        })
        .collect();
    match roots.as_deref() {
        Some([first, rest @ ..]) if rest.iter().all(|root| root == first) => first.clone(),
        _ => cwd.to_path_buf(),
    }
}

fn existing_target_directory(cwd: &Path, path: &Path) -> Option<PathBuf> {
    let resolved = resolve_against_cwd(path, Some(cwd));
    if resolved.is_dir() {
        Some(resolved)
    } else if resolved.is_file() {
        resolved.parent().map(Path::to_path_buf)
    } else {
        None
    }
}

fn load_domain_config(
    start_directory: &Path,
    overrides: &ConfigOverrides,
    domain: AnalyzedInput,
) -> Result<Config, ConfigError> {
    let resolved = resolve_domain_config(start_directory, overrides, domain)?;
    let mut config = parse_config_with_overrides(&resolved.content, overrides)?;
    config.project.root_dir = Some(resolved.root_dir);
    Ok(config)
}

struct ResolvedDomainConfig {
    content: String,
    root_dir: PathBuf,
}

fn resolve_domain_config(
    start_directory: &Path,
    overrides: &ConfigOverrides,
    domain: AnalyzedInput,
) -> Result<ResolvedDomainConfig, ConfigError> {
    let global_config_path = default_global_config_path();
    resolve_domain_config_with(
        start_directory,
        overrides,
        domain,
        global_config_path.as_deref(),
    )
}

fn default_global_config_path() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    load_env()
}

fn load_env() -> Option<PathBuf> {
    resolve_global_config_path(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

fn resolve_global_config_path(
    xdg_config_home: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    if let Some(xdg) = xdg_config_home.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(xdg).join(GLOBAL_CONFIG_RELATIVE_PATH));
    }
    let home = home.filter(|value| !value.is_empty())?;
    Some(
        PathBuf::from(home)
            .join(DEFAULT_CONFIG_DIR_NAME)
            .join(GLOBAL_CONFIG_RELATIVE_PATH),
    )
}

fn resolve_domain_config_with(
    start_directory: &Path,
    overrides: &ConfigOverrides,
    domain: AnalyzedInput,
    global_config_path: Option<&Path>,
) -> Result<ResolvedDomainConfig, ConfigError> {
    let discovered_root = discover_project_root(start_directory);
    let local_config_path = discovered_root
        .as_deref()
        .map(|root| root.join(CONFIG_FILE_NAME));
    let root_dir = discovered_root.unwrap_or_else(|| start_directory.to_path_buf());

    if let Some(explicit_path) = overrides.config_path.as_deref() {
        let content = read_config_at(explicit_path)?;
        let root_dir = resolve_against_cwd(explicit_path, Some(start_directory))
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map_or_else(|| lexical_normalize(start_directory), Path::to_path_buf);
        return Ok(ResolvedDomainConfig { content, root_dir });
    }

    let candidate_paths: &[Option<&Path>] = match domain {
        AnalyzedInput::Code => &[local_config_path.as_deref(), global_config_path],
        AnalyzedInput::Command => &[global_config_path, local_config_path.as_deref()],
    };

    for candidate in candidate_paths.iter().flatten() {
        if let Some(content) = read_optional_config_at(candidate)? {
            return Ok(ResolvedDomainConfig { content, root_dir });
        }
    }

    Ok(ResolvedDomainConfig {
        content: String::new(),
        root_dir,
    })
}

fn read_config_at(path: &Path) -> Result<String, ConfigError> {
    std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.display().to_string(),
        source,
    })
}

fn read_optional_config_at(path: &Path) -> Result<Option<String>, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ConfigError::Io {
            path: path.display().to_string(),
            source,
        }),
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
    #[serde(default)]
    architecture: Option<ArchitectureConfig>,
}

fn rule_names(mut predicate: impl FnMut(ItemId) -> bool) -> HashSet<RuleName> {
    CATALOG
        .items()
        .filter(|&item| predicate(item))
        .map(|item| RuleName(CATALOG.name(item)))
        .collect()
}

/// Whether `item` is on under `selection`, and why.
fn status<T>(catalog: &Catalog<T>, selection: &Selection<'_, T>, item: ItemId) -> RuleStatus {
    let decision = selection.decide(item);
    let vocabulary = catalog.vocabulary();
    let deciding = match decision.reason {
        Reason::Default => None,
        Reason::Item => Some(Deciding {
            verdict: decision.verdict,
            selector: catalog.name(item),
            branch: Vec::new(),
        }),
        Reason::Tag { tag, source } => Some(Deciding {
            verdict: decision.verdict,
            selector: vocabulary.tag(tag).label,
            branch: vocabulary
                .chain_through(source, tag)
                .into_iter()
                .map(|id| vocabulary.tag(id).label)
                .collect(),
        }),
    };
    RuleStatus {
        enabled: decision.verdict == Verdict::Include,
        deciding,
    }
}

/// The selector that decided a rule's status.
#[derive(Debug)]
struct Deciding {
    /// What the selector asks for.
    verdict: Verdict,
    /// The selector's canonical label.
    selector: &'static str,
    /// The root-to-leaf path of the rule's tag the selector applies through; empty for a
    /// rule name.
    branch: Vec<&'static str>,
}

/// One warning per entry of the config file's `select` and `ignore` whose removal changes
/// no rule's outcome. Command-line overrides are left out: they are not in the file.
fn ineffective_selector_warnings(
    mut duplicates: BTreeMap<Selector, Verdict>,
    selection: &Selection<'_, RegisteredRule>,
) -> Vec<String> {
    duplicates.extend(selection.ineffective());
    duplicates
        .into_iter()
        .map(|(selector, verdict)| {
            let location = match verdict {
                Verdict::Include => Location::Select,
                Verdict::Exclude => Location::Ignore,
            };
            format!(
                "`{}` in {location} changes no rule's outcome; remove it",
                taxonomy::selector_label(&CATALOG, selector)
            )
        })
        .collect()
}

/// Resolves the active [`Selection`] after applying command-line `--select` and `--ignore`.
fn apply_overrides(
    file: Selection<'static, RegisteredRule>,
    overrides: &ConfigOverrides,
) -> Result<Selection<'static, RegisteredRule>, ConfigError> {
    if overrides.select.is_none() && overrides.ignore.is_empty() {
        return Ok(file);
    }
    if overrides.select.is_some() {
        return parse_selection(overrides.select.as_deref(), &overrides.ignore)
            .map(|parsed| parsed.selection);
    }
    let cli_ignores = resolve_all(&overrides.ignore, &Location::Ignore)?
        .into_iter()
        .map(|selector| (selector, Verdict::Exclude));
    file.override_with(cli_ignores)
        .map_err(|conflicts| first_conflict(&conflicts))
}

/// A parsed file or CLI selection alongside any duplicate entries within `select` or `ignore`.
struct ParsedSelection {
    selection: Selection<'static, RegisteredRule>,
    duplicates: BTreeMap<Selector, Verdict>,
}

fn parse_selection(
    select: Option<&[String]>,
    ignore: &[String],
) -> Result<ParsedSelection, ConfigError> {
    let mut directives = Vec::new();
    let mut duplicates = BTreeMap::new();
    if let Some(labels) = select {
        resolve_directives(
            labels,
            &Location::Select,
            Verdict::Include,
            &mut directives,
            &mut duplicates,
        )?;
    }
    resolve_directives(
        ignore,
        &Location::Ignore,
        Verdict::Exclude,
        &mut directives,
        &mut duplicates,
    )?;
    let default = if select.is_some() {
        Verdict::Exclude
    } else {
        Verdict::Include
    };
    let selection = Selection::new(&CATALOG, default, directives)
        .map_err(|conflicts| first_conflict(&conflicts))?;
    Ok(ParsedSelection {
        selection,
        duplicates,
    })
}

fn first_conflict(conflicts: &[tagonomy::Conflict]) -> ConfigError {
    ConfigError::Conflict(taxonomy::selector_label(&CATALOG, conflicts[0].selector))
}

fn resolve_directives(
    labels: &[String],
    location: &Location,
    verdict: Verdict,
    directives: &mut Vec<(Selector, Verdict)>,
    duplicates: &mut BTreeMap<Selector, Verdict>,
) -> Result<(), ConfigError> {
    let mut seen = BTreeSet::new();
    for label in labels {
        let selector = resolve(label, location)?;
        if seen.insert(selector) {
            directives.push((selector, verdict));
        } else {
            duplicates.insert(selector, verdict);
        }
    }
    Ok(())
}

fn resolve_all(labels: &[String], location: &Location) -> Result<BTreeSet<Selector>, ConfigError> {
    labels
        .iter()
        .map(|label| resolve(label, location))
        .collect()
}

fn resolve(label: &str, location: &Location) -> Result<Selector, ConfigError> {
    match CATALOG.resolve(label) {
        Ok(selector) => Ok(selector),
        Err(LabelError::Group(group)) => Err(ConfigError::FacetLabel {
            label: label.to_owned(),
            location: location.clone(),
            values: taxonomy::group_values(group),
        }),
        Err(LabelError::Unknown) => Err(ConfigError::UnknownSelector {
            suggestion: taxonomy::closest_label(label),
            label: label.to_owned(),
            location: location.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rstest::rstest;

    use super::*;
    use crate::rule_declaration::{Consensus, ImpactedQuality, Topic, TopicDoc};

    const DOC: TopicDoc = TopicDoc {
        description: "Doc.",
        scope_note: "Doc.",
    };
    const PARENT: Topic = Topic::root("parent", DOC);
    const CHILD: Topic = Topic::sub("child", &PARENT, DOC);
    const OTHER: Topic = Topic::root("other", DOC);

    fn enabled(config: &Config, path: &str) -> BTreeSet<&'static str> {
        registered_rules()
            .iter()
            .map(|rule| rule.name)
            .filter(|&name| config.is_rule_enabled_for_path(name, Path::new(path)))
            .map(|name| name.0)
            .collect()
    }

    fn all_except(excluded: &[&str]) -> BTreeSet<&'static str> {
        registered_rules()
            .iter()
            .map(|rule| rule.name.0)
            .filter(|name| !excluded.contains(name))
            .collect()
    }

    /// The status of `rule`, on the branches `parent > child`, `other` and `opinionated`.
    #[rstest]
    #[case::default_on_without_select(Verdict::Include, &[], &[], "enabled (default)")]
    #[case::default_off_with_select(Verdict::Exclude, &[], &[], "disabled (not in `select`)")]
    #[case::ignored_without_select(
        Verdict::Include,
        &[],
        &["other"],
        "disabled by `ignore = [\"other\"]`"
    )]
    #[case::ancestor_selected(
        Verdict::Exclude,
        &["parent"],
        &[],
        "enabled by `select = [\"parent\"]` (via parent > child)"
    )]
    #[case::nearest_select_on_a_branch_wins(
        Verdict::Exclude,
        &["child"],
        &["parent"],
        "enabled by `select = [\"child\"]` (via parent > child)"
    )]
    #[case::nearest_ignore_on_a_branch_wins(
        Verdict::Exclude,
        &["parent"],
        &["child"],
        "disabled by `ignore = [\"child\"]` (via parent > child)"
    )]
    #[case::ignore_wins_across_topics(
        Verdict::Exclude,
        &["child"],
        &["other"],
        "disabled by `ignore = [\"other\"]`"
    )]
    #[case::ignore_wins_across_facets(
        Verdict::Exclude,
        &["opinionated"],
        &["parent"],
        "disabled by `ignore = [\"parent\"]` (via parent > child)"
    )]
    #[case::selected_rule_name_beats_tags(
        Verdict::Exclude,
        &["rule"],
        &["parent", "other", "opinionated"],
        "enabled by `select = [\"rule\"]`"
    )]
    #[case::ignored_rule_name_beats_tags(
        Verdict::Exclude,
        &["parent", "other", "opinionated"],
        &["rule"],
        "disabled by `ignore = [\"rule\"]`"
    )]
    fn the_nearest_selector_decides_and_ignore_wins_across_branches(
        #[case] default: Verdict,
        #[case] select: &[&str],
        #[case] ignore: &[&str],
        #[case] expected: &str,
    ) {
        let tags = [
            CHILD.tag(),
            OTHER.tag(),
            Consensus::Opinionated.tag(),
            ImpactedQuality::Maintainability.tag(),
        ];
        let vocabulary = tagonomy::Vocabulary::new(tags).unwrap();
        let catalog = Catalog::new(vocabulary, [tagonomy::Item::named("rule", tags)]).unwrap();
        let directives = select
            .iter()
            .map(|label| (catalog.resolve(label).unwrap(), Verdict::Include))
            .chain(
                ignore
                    .iter()
                    .map(|label| (catalog.resolve(label).unwrap(), Verdict::Exclude)),
            );
        let selection = Selection::new(&catalog, default, directives).unwrap();
        let rule_status = status(&catalog, &selection, catalog.item("rule").unwrap());
        assert_eq!(rule_status.to_string(), expected);
    }

    #[test]
    fn status_routes_witness_branch_through_the_deciding_tag_on_a_polyhierarchy() {
        const GROUP: tagonomy::TagGroup = tagonomy::TagGroup {
            name: "subject",
            cardinality: tagonomy::Cardinality::AtLeastOne,
        };
        const FIRST_ROOT: tagonomy::Tag = tagonomy::Tag::new(&GROUP, "first-root", "First root.");
        const SECOND_ROOT: tagonomy::Tag =
            tagonomy::Tag::new(&GROUP, "second-root", "Second root.");
        const SECOND_BRANCH: tagonomy::Tag =
            tagonomy::Tag::new(&GROUP, "second-branch", "Second branch.").implies(&[SECOND_ROOT]);
        const LEAF: tagonomy::Tag =
            tagonomy::Tag::new(&GROUP, "leaf", "Leaf.").implies(&[FIRST_ROOT, SECOND_BRANCH]);

        let vocabulary =
            tagonomy::Vocabulary::new([FIRST_ROOT, SECOND_ROOT, SECOND_BRANCH, LEAF]).unwrap();
        let catalog = Catalog::new(vocabulary, [tagonomy::Item::named("rule", [LEAF])]).unwrap();
        let selection = Selection::new(
            &catalog,
            Verdict::Exclude,
            [(catalog.resolve("second-branch").unwrap(), Verdict::Include)],
        )
        .unwrap();
        let rule_status = status(&catalog, &selection, catalog.item("rule").unwrap());

        assert_eq!(
            rule_status.to_string(),
            "enabled by `select = [\"second-branch\"]` (via second-root > second-branch > leaf)"
        );
    }

    #[rstest]
    #[case::implied_by_a_broader_selection(
        r#"select = ["testing", "test-timing"]"#,
        &["`test-timing` in `select` changes no rule's outcome; remove it"]
    )]
    #[case::already_ignored(
        r#"ignore = ["testing", "test-timing"]"#,
        &["`test-timing` in `ignore` changes no rule's outcome; remove it"]
    )]
    #[case::duplicate_in_select(
        r#"select = ["testing", "testing"]"#,
        &["`testing` in `select` changes no rule's outcome; remove it"]
    )]
    #[case::duplicate_and_ineffective_emits_single_warning(
        r#"select = ["testing", "test-timing", "test-timing"]"#,
        &["`test-timing` in `select` changes no rule's outcome; remove it"]
    )]
    #[case::synonym_duplicate_in_ignore(
        r#"ignore = ["jj", "jujutsu"]"#,
        &["`jj` in `ignore` changes no rule's outcome; remove it"]
    )]
    #[case::an_exception(
        indoc::indoc! {r#"
            select = ["testing"]
            ignore = ["test-timing"]"#},
        &[]
    )]
    fn selectors_that_change_no_outcome_are_warned_about(
        #[case] config_toml: &str,
        #[case] expected: &[&str],
    ) {
        assert_eq!(parse_config(config_toml).unwrap().warnings, expected);
    }

    #[rstest]
    #[case::cli_ignore_removing_a_file_selection(
        r#"select = ["sleep-in-tests", "nested-function"]"#,
        None,
        vec!["sleep-in-tests".to_owned()]
    )]
    #[case::cli_selection_without_effect(
        "",
        Some(vec!["testing".to_owned(), "test-timing".to_owned()]),
        vec![]
    )]
    fn command_line_overrides_are_not_warned_about(
        #[case] config_toml: &str,
        #[case] select: Option<Vec<String>>,
        #[case] ignore: Vec<String>,
    ) {
        let overrides = ConfigOverrides {
            config_path: None,
            select,
            ignore,
        };
        let config = parse_config_with_overrides(config_toml, &overrides).unwrap();
        assert_eq!(config.warnings, Vec::<String>::new());
    }

    #[test]
    fn per_file_ignores_remove_only_on_matching_paths() {
        let opinionated: Vec<_> = rules_tagged("opinionated")
            .unwrap()
            .iter()
            .map(|rule| rule.name.0)
            .collect();
        let config = parse_config("[per-file-ignores]\n\"tests/**\" = [\"opinionated\"]").unwrap();
        assert_eq!(
            enabled(&config, "tests/test_module.py"),
            all_except(&opinionated)
        );
        assert_eq!(enabled(&config, "src/module.py"), all_except(&[]));
    }

    #[test]
    fn per_file_ignores_apply_after_selection_by_rule_name() {
        let rule = rules_tagged("opinionated").unwrap()[0].name.0;
        let config = parse_config(&indoc::formatdoc! {r#"
            select = ["{rule}"]
            [per-file-ignores]
            "tests/**" = ["opinionated"]"#})
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
        r#"select = ["consensus"]"#,
        "`consensus` in `select` is a facet label, not a selector; use one of its values: \
         opinionated, unopinionated"
    )]
    #[case::multi_word_facet_label(
        r#"select = ["Impacted quality"]"#,
        "`Impacted quality` in `select` is a facet label, not a selector; use one of its \
         values: reliability, maintainability"
    )]
    #[case::typo_in_per_file(
        "[per-file-ignores]\n\"tests/**\" = [\"opinonated\"]",
        "unknown selector `opinonated` in `per-file-ignores.\"tests/**\"`; did you mean \
         `opinionated`?"
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
        indoc::indoc! {r#"
            TOML parse error at line 1, column 1
              |
            1 | selct = ["testing"]
              | ^^^^^
            unknown field `selct`, expected one of `select`, `ignore`, `per-file-ignores`, `rules`, `context`, `architecture`"#}
    )]
    #[case::unknown_context_key(
        "[context]\ntest-paterns = []",
        indoc::indoc! {r"
            TOML parse error at line 2, column 1
              |
            2 | test-paterns = []
              | ^^^^^^^^^^^^
            unknown field `test-paterns`, expected `test-patterns` or `source-roots`"}
    )]
    #[case::forward_architecture_dependency(
        indoc::indoc! {r#"
            [[architecture.components]]
            name = "Api"
            roots = ["app.api"]
            depends-on = ["Core"]

            [[architecture.components]]
            name = "Core"
            roots = ["app.core"]
        "#},
        "`architecture.components`: component `Api` cannot depend on `Core`; dependencies must be declared earlier in `[[architecture.components]]` to guarantee acyclicity"
    )]
    fn invalid_configs_are_rejected_loudly(#[case] config_toml: &str, #[case] message: &str) {
        let error = parse_config(config_toml).unwrap_err().to_string();
        assert!(error.starts_with(message), "{error}");
    }

    #[test]
    fn valid_rule_options_are_stored_per_rule() {
        let config = parse_config(indoc::indoc! {r#"
            [rules.too-many-assertions]
            max-assertions = 6
            [rules.too-many-assertions.rust]
            max-assertions = 8
            [rules.abbreviated-name]
            extend-banned = ["ctx"]"#})
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
    #[case::cli_ignore_without_select_keeps_other_toml_selected_rules(
        "select = [\"sleep-in-tests\", \"nested-function\"]",
        None,
        vec!["sleep-in-tests".to_owned()],
        "nested-function",
        "enabled by `select = [\"nested-function\"]`"
    )]
    #[case::cli_ignore_without_select_keeps_unselected_rules_off(
        "select = [\"sleep-in-tests\", \"nested-function\"]",
        None,
        vec!["sleep-in-tests".to_owned()],
        "single-letter-name",
        "disabled (not in `select`)"
    )]
    #[case::cli_ignore_without_select_preserves_toml_ignore(
        "ignore = [\"sleep-in-tests\"]",
        None,
        vec!["nested-function".to_owned()],
        "sleep-in-tests",
        "disabled by `ignore = [\"sleep-in-tests\"]`"
    )]
    #[case::cli_ignore_tag_without_select_carves_exception_from_toml_select(
        "select = [\"testing\"]",
        None,
        vec!["test-timing".to_owned()],
        "sleep-in-tests",
        "disabled by `ignore = [\"test-timing\"]` (via testing > test-timing)"
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

    #[rstest::rstest]
    #[case::xdg_config_home(
        Some("/tmp/xdg"),
        Some("/tmp/home"),
        Some("/tmp/xdg/omnicoach/config.toml")
    )]
    #[case::home_fallback(
        None,
        Some("/tmp/home"),
        Some("/tmp/home/.config/omnicoach/config.toml")
    )]
    #[case::empty_xdg_falls_back_to_home(
        Some(""),
        Some("/tmp/home"),
        Some("/tmp/home/.config/omnicoach/config.toml")
    )]
    #[case::empty_home_returns_none(None, Some(""), None)]
    #[case::all_unset(None, None, None)]
    fn test_resolve_global_config_path_variants(
        #[case] xdg_config_home: Option<&str>,
        #[case] home: Option<&str>,
        #[case] expected: Option<&str>,
    ) {
        assert_eq!(
            resolve_global_config_path(
                xdg_config_home.map(std::ffi::OsStr::new),
                home.map(std::ffi::OsStr::new),
            ),
            expected.map(PathBuf::from)
        );
    }

    #[rstest::rstest]
    #[case::code_domain_prefers_repo_config(AnalyzedInput::Code, true, "sleep-in-tests")]
    #[case::command_domain_prefers_global_config(AnalyzedInput::Command, true, "nested-function")]
    #[case::command_domain_without_global_falls_back_to_repo(
        AnalyzedInput::Command,
        false,
        "sleep-in-tests"
    )]
    fn test_domain_scoped_config_resolution(
        #[case] domain: AnalyzedInput,
        #[case] has_global_config: bool,
        #[case] expected_substring: &str,
    ) {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let root = temp_dir.path();
        let global_dir = root.join("global_dir");
        let global_config = global_dir.join("omnicoach/config.toml");
        std::fs::create_dir_all(global_dir.join("omnicoach")).expect("create global dir");
        std::fs::write(&global_config, "ignore = [\"nested-function\"]\n").expect("write global");

        let repo_dir = root.join("repo");
        let repo_nested = repo_dir.join("src");
        std::fs::create_dir_all(&repo_nested).expect("create repo dir");
        std::fs::write(
            repo_dir.join(CONFIG_FILE_NAME),
            "ignore = [\"sleep-in-tests\"]\n",
        )
        .expect("write repo config");

        let resolved = resolve_domain_config_with(
            &repo_nested,
            &ConfigOverrides::default(),
            domain,
            has_global_config.then_some(global_config.as_path()),
        )
        .expect("resolve config");

        assert!(resolved.content.contains(expected_substring));
        assert_eq!(resolved.root_dir, repo_dir);
    }

    #[test]
    fn test_code_domain_stops_at_vcs_boundary_and_uses_global_config() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let parent_dir = temp_dir.path();
        std::fs::write(
            parent_dir.join(CONFIG_FILE_NAME),
            "ignore = [\"escaped-multiline-string\"]\n",
        )
        .expect("write parent config");

        let repo_dir = parent_dir.join("repo");
        let nested_dir = repo_dir.join("sub");
        std::fs::create_dir_all(repo_dir.join(".git")).expect("create fake .git dir");
        std::fs::create_dir_all(&nested_dir).expect("create nested dir");

        let global_dir = parent_dir.join("global_dir");
        let global_config = global_dir.join("omnicoach/config.toml");
        std::fs::create_dir_all(global_dir.join("omnicoach")).expect("create global dir");
        std::fs::write(&global_config, "ignore = [\"nested-function\"]\n").expect("write global");

        let resolved = resolve_domain_config_with(
            &nested_dir,
            &ConfigOverrides::default(),
            AnalyzedInput::Code,
            Some(&global_config),
        )
        .expect("resolve config");

        assert_eq!(resolved.root_dir, repo_dir);
        assert!(resolved.content.contains("nested-function"));
        assert!(!resolved.content.contains("escaped-multiline-string"));
    }

    #[test]
    fn discover_project_root_and_load_config_from_root_finds_parent_config() {
        let temp_dir = tempfile::tempdir().expect("tempdir creation should succeed");
        let project_root = temp_dir.path().join("project");
        let nested_dir = project_root.join("src").join("nested");
        std::fs::create_dir_all(&nested_dir).expect("create nested dirs");
        std::fs::write(
            project_root.join(CONFIG_FILE_NAME),
            "ignore = [\"sleep-in-tests\"]\n",
        )
        .expect("write config");

        let discovered_root =
            discover_project_root(&nested_dir).expect("should discover parent project root");
        let config = load_config_from_root(&discovered_root, &ConfigOverrides::default())
            .expect("load config should succeed");
        let rule = find_rule("sleep-in-tests").expect("rule exists");
        assert_eq!(discovered_root, project_root);
        assert!(!config.is_rule_enabled(rule.name));
    }

    #[test]
    fn explicit_config_path_anchors_root_dir_and_manifest_marks_project_root() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let caller_dir = temp_dir.path().join("caller");
        let repo_dir = temp_dir.path().join("repo");
        let manifest_pkg = temp_dir.path().join("manifest_only/src");
        std::fs::create_dir_all(&caller_dir).expect("mkdir caller");
        std::fs::create_dir_all(&repo_dir).expect("mkdir repo");
        std::fs::create_dir_all(&manifest_pkg).expect("mkdir manifest_only/src");
        let explicit_config = repo_dir.join(CONFIG_FILE_NAME);
        std::fs::write(
            &explicit_config,
            "[per-file-ignores]\n\"tests/**\" = [\"single-letter-name\"]\n",
        )
        .expect("write explicit config");
        std::fs::write(
            temp_dir.path().join("manifest_only/pyproject.toml"),
            "[project]\nname = \"demo\"\n",
        )
        .expect("write pyproject.toml");

        let resolved = resolve_domain_config_with(
            &caller_dir,
            &ConfigOverrides {
                config_path: Some(explicit_config),
                ..ConfigOverrides::default()
            },
            AnalyzedInput::Code,
            None,
        )
        .expect("resolve explicit config");
        let discovered_manifest_root = discover_project_root(&manifest_pkg);

        assert_eq!(resolved.root_dir, repo_dir);
        assert_eq!(
            discovered_manifest_root,
            Some(temp_dir.path().join("manifest_only"))
        );
    }

    #[test]
    fn resolve_target_config_start_directory_isolates_external_repos_and_joins_shared_roots() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let caller_repo = temp_dir.path().join("caller_repo");
        let target_repo = temp_dir.path().join("target_repo");
        let other_repo = temp_dir.path().join("other_repo");
        std::fs::create_dir_all(&caller_repo).expect("mkdir caller_repo");
        std::fs::create_dir_all(target_repo.join("src")).expect("mkdir target_repo/src");
        std::fs::create_dir_all(target_repo.join("tests")).expect("mkdir target_repo/tests");
        std::fs::create_dir_all(&other_repo).expect("mkdir other_repo");
        std::fs::write(
            caller_repo.join(CONFIG_FILE_NAME),
            "select = [\"python\"]\n",
        )
        .expect("write caller config");
        std::fs::write(
            target_repo.join("pyproject.toml"),
            "[project]\nname = \"target\"\n",
        )
        .expect("write target pyproject");
        std::fs::write(
            other_repo.join("Cargo.toml"),
            "[package]\nname = \"other\"\n",
        )
        .expect("write other Cargo.toml");
        let target_file = target_repo.join("src/app.py");
        let target_test = target_repo.join("tests/test_app.py");
        std::fs::write(&target_file, "").expect("write app.py");
        std::fs::write(&target_test, "").expect("write test_app.py");

        let shared = resolve_target_config_start_directory(
            &caller_repo,
            &[target_file.clone(), target_test],
        );
        let mixed = resolve_target_config_start_directory(&caller_repo, &[target_file, other_repo]);
        let missing = resolve_target_config_start_directory(
            &caller_repo,
            &[temp_dir.path().join("missing/file.py")],
        );
        let empty = resolve_target_config_start_directory(&caller_repo, &[]);

        assert_eq!(shared, target_repo);
        assert_eq!(mixed, caller_repo);
        assert_eq!(missing, caller_repo);
        assert_eq!(empty, caller_repo);
    }
}
