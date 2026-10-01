//! Shared core module of the Omni linter toolkit.

architecture_component!(CoreVocabulary);

mod rule_options;

pub use self::rule_options::{
    CountOption, DeclaredOptions, ListKind, ListOption, OptionProblem, OptionSpec,
    OptionsDeclaration, RuleOptions, RuleOptionsError, RuleOverrides,
};
use crate::diagnostic::RuleName;
use ast_grep_language::SupportLang;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use strum::{EnumIter, IntoStaticStr};

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

/// Compile-time static descriptor for default filter lists (both allowlists and denylists).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterListDefaults {
    /// Base items active across all supported languages.
    pub base: &'static [&'static str],
    /// Language-specific items added to the base defaults.
    pub extend: &'static [(SupportLang, &'static [&'static str])],
    /// Language-specific items removed from the base defaults.
    pub exempt: &'static [(SupportLang, &'static [&'static str])],
}

impl FilterListDefaults {
    /// Resolves the default set of strings for a specific language.
    #[must_use]
    pub fn resolve_default_for_lang(&self, lang: SupportLang) -> HashSet<String> {
        let mut set: HashSet<String> = self.base.iter().map(|&item| item.to_string()).collect();
        for &(target_lang, items) in self.extend {
            if target_lang == lang {
                set.extend(items.iter().map(|&item| item.to_string()));
            }
        }
        for &(target_lang, items) in self.exempt {
            if target_lang == lang {
                for &item in items {
                    set.remove(item);
                }
            }
        }
        set
    }
}

/// Compile-time static descriptor for scalar or structured default configuration values
/// with optional per-language overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageDefaults<T: Copy + 'static> {
    /// Default value active across all supported languages unless overridden.
    pub base: T,
    /// Language-specific default values that override `base`.
    pub overrides: &'static [(SupportLang, T)],
}

impl<T: Copy + 'static> LanguageDefaults<T> {
    /// Creates a new static language-aware default descriptor.
    #[must_use]
    pub const fn new(base: T, overrides: &'static [(SupportLang, T)]) -> Self {
        Self { base, overrides }
    }

    /// Resolves the compile-time default value for a specific language.
    #[must_use]
    pub fn resolve_default_for_lang(&self, lang: SupportLang) -> T {
        for &(target_lang, value) in self.overrides {
            if target_lang == lang {
                return value;
            }
        }
        self.base
    }
}

/// Enforcement mode for rules targeting sensitive language constructs
/// (e.g. `cast`, `suppress`, `getattr`, `except Exception`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum EnforcementMode {
    /// Completely bans the construct from targeted files (only suppressible via `# omni:ignore`).
    Ban,
    /// Permits the construct only if accompanied by an explanatory comment.
    RequireExplanation,
}

impl EnforcementMode {
    /// The key setting a code rule's enforcement mode under `[rules.<name>]`.
    pub const KEY: &'static str = "enforcement_mode";

    /// What each mode does, for `--explain`.
    pub const DOC: &'static str = "`ban` flags every occurrence; `require-explanation` accepts \
                                   an occurrence explained by an adjacent comment.";
}

/// Every language the linter analyzes.
pub const SUPPORTED_LANGUAGES: &[SupportLang] = &[SupportLang::Python, SupportLang::Rust];

/// Returns the lowercase canonical configuration key for a supported language.
#[must_use]
pub const fn support_lang_name(lang: SupportLang) -> &'static str {
    match lang {
        SupportLang::Python => "python",
        SupportLang::Rust => "rust",
        _ => "",
    }
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

/// The default configuration file name.
pub const CONFIG_FILE_NAME: &str = ".omnilint.toml";

const DEFAULT_TEST_PATTERNS: &[&str] = &[
    "**/tests/**",
    "**/test_*.py",
    "**/*_test.py",
    "**/*_test.rs",
    "**/tests.rs",
];

/// Compiles `pattern` with `*` matching across `/`, as all config globs do.
///
/// # Errors
///
/// Returns the glob syntax error for an invalid pattern.
pub(crate) fn compile_glob(pattern: &str) -> Result<globset::Glob, globset::Error> {
    globset::GlobBuilder::new(pattern)
        .literal_separator(false)
        .build()
}

fn compile_glob_set<'a>(
    patterns: impl IntoIterator<Item = &'a str>,
) -> Result<globset::GlobSet, globset::Error> {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(compile_glob(pattern)?);
    }
    builder.build()
}

fn default_test_patterns() -> globset::GlobSet {
    // The defaults are constants covered by `test_context_test_path_detection`.
    compile_glob_set(DEFAULT_TEST_PATTERNS.iter().copied()).unwrap_or_default()
}

fn deserialize_test_patterns<'de, D>(deserializer: D) -> Result<globset::GlobSet, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let patterns = Vec::<String>::deserialize(deserializer)?;
    compile_glob_set(patterns.iter().map(String::as_str)).map_err(serde::de::Error::custom)
}

/// Configuration settings for path context detection (e.g. test paths).
#[derive(Deserialize, Debug, Clone)]
pub struct ContextConfig {
    /// Glob patterns used to identify test files, compiled at load.
    #[serde(
        default = "default_test_patterns",
        deserialize_with = "deserialize_test_patterns"
    )]
    pub test_patterns: globset::GlobSet,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            test_patterns: default_test_patterns(),
        }
    }
}

/// Configuration settings parsed from `.omnilint.toml`.
///
/// Rule selection is already resolved into rule names: `select`, `ignore` and
/// `per_file_ignores` selectors are parsed by `rule_selection`, never here.
#[derive(Deserialize, Debug, Default, Clone)]
pub struct Config {
    /// Rules disabled by `select` and `ignore`.
    #[serde(skip)]
    pub disabled_rules: HashSet<RuleName>,
    /// Validated `[rules.<name>]` options, keyed by rule; set by `rule_selection`.
    #[serde(skip)]
    pub rule_overrides: HashMap<RuleName, RuleOverrides>,
    /// Path context classifier settings.
    #[serde(default)]
    pub context: ContextConfig,
    /// Per-file rule ignores: glob patterns paired with the rules they disable.
    #[serde(skip)]
    pub per_file_ignores: Vec<(globset::GlobMatcher, HashSet<RuleName>)>,
}

fn normalize_path_for_glob(path: &Path) -> String {
    let relative = if path.is_absolute() {
        std::env::current_dir()
            .ok()
            .and_then(|cwd| path.strip_prefix(cwd).ok())
            .unwrap_or(path)
    } else {
        path
    };

    let stripped = relative.strip_prefix("./").unwrap_or(relative);
    stripped.to_string_lossy().replace('\\', "/")
}

impl Config {
    /// Returns true if the given path matches any configured test pattern.
    #[must_use]
    pub fn is_test_path(&self, path: &Path) -> bool {
        self.context
            .test_patterns
            .is_match(normalize_path_for_glob(path))
    }

    /// Returns true if the given rule is enabled in this configuration.
    #[must_use]
    pub fn is_rule_enabled(&self, rule: RuleName) -> bool {
        !self.disabled_rules.contains(&rule)
    }

    /// Returns true if the given rule is enabled for a specific file path.
    #[must_use]
    pub fn is_rule_enabled_for_path(&self, rule: RuleName, path: &Path) -> bool {
        if !self.is_rule_enabled(rule) {
            return false;
        }
        if self.per_file_ignores.is_empty() {
            return true;
        }

        let normalized = normalize_path_for_glob(path);
        !self
            .per_file_ignores
            .iter()
            .any(|(matcher, rules)| matcher.is_match(&normalized) && rules.contains(&rule))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGGING_RULE: RuleName = RuleName("mock-logging-rule");
    const STYLE_RULE: RuleName = RuleName("mock-style-rule");

    #[test]
    fn test_filter_list_defaults_resolve() {
        const DEFAULTS: FilterListDefaults = FilterListDefaults {
            base: &["common", "shared", "temp"],
            extend: &[(SupportLang::Rust, &["rust_only"])],
            exempt: &[(SupportLang::Rust, &["temp"])],
        };

        let python_defaults = DEFAULTS.resolve_default_for_lang(SupportLang::Python);
        assert_eq!(
            python_defaults,
            HashSet::from(["common", "shared", "temp"].map(String::from))
        );

        let rust_defaults = DEFAULTS.resolve_default_for_lang(SupportLang::Rust);
        assert_eq!(
            rust_defaults,
            HashSet::from(["common", "shared", "rust_only"].map(String::from))
        );
    }

    #[rstest::rstest]
    #[case("tests/foo.rs", true)]
    #[case("src/tests/foo.rs", true)]
    #[case("test_calculator.py", true)]
    #[case("foo/test_calculator.py", true)]
    #[case("foo/calculator_test.py", true)]
    #[case("src/foo_test.rs", true)]
    #[case("src/tests.rs", true)]
    #[case("src/main.rs", false)]
    #[case("src/calculator.py", false)]
    fn test_context_test_path_detection(#[case] path: &str, #[case] expected: bool) {
        let config = Config::default();
        assert_eq!(config.is_test_path(Path::new(path)), expected);
    }

    #[test]
    fn test_disabled_rules_and_per_file_ignores() {
        let config = Config {
            disabled_rules: HashSet::from([LOGGING_RULE]),
            per_file_ignores: vec![(
                compile_glob("tests/**").unwrap().compile_matcher(),
                HashSet::from([STYLE_RULE]),
            )],
            ..Default::default()
        };

        assert!(!config.is_rule_enabled(LOGGING_RULE));
        assert!(!config.is_rule_enabled_for_path(STYLE_RULE, Path::new("tests/my_test.rs")));
        assert!(config.is_rule_enabled_for_path(STYLE_RULE, Path::new("src/lib.rs")));
    }

    #[test]
    fn test_custom_test_patterns_replace_defaults() {
        let toml_content = indoc::indoc! {r#"
            [context]
            test_patterns = ["**/spec/**"]
        "#};
        let config: Config = toml::from_str(toml_content).unwrap();

        assert!(config.is_test_path(Path::new("app/spec/model.py")));
        assert!(!config.is_test_path(Path::new("tests/foo.rs")));
    }

    #[rstest::rstest]
    #[case::test_patterns("[context]\ntest_patterns = [\"src/[\"]")]
    fn test_invalid_glob_is_config_error(#[case] toml_content: &str) {
        let error = toml::from_str::<Config>(toml_content).unwrap_err();
        assert!(error.to_string().contains("src/["), "{error}");
    }

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
