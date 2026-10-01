//! Shared core module of the Omni linter toolkit.

architecture_component!(CoreVocabulary);

use crate::diagnostic::{
    Diagnostic, RuleName, SourceLocation, ViolationMessage, ViolationTemplate,
};
use ast_grep_language::SupportLang;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

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

/// Configuration for rules controlled by numeric `min` or `max` thresholds (e.g., `max-test-assertions`, `no-identical-positional-types`).
#[derive(Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(Serialize))]
pub struct ThresholdConfig {
    /// Optional override for the minimum threshold.
    #[serde(default)]
    pub min: Option<usize>,

    /// Optional override for the maximum threshold.
    #[serde(default)]
    pub max: Option<usize>,
}

impl ThresholdConfig {
    /// The TOML keys of this shape, checked against the serde fields by a test.
    pub const KEYS: &'static [&'static str] = &["min", "max"];
}

/// Configuration for rules that filter identifier names, abbreviations, or suffixes (denylist rules).
///
/// Supports explicit replacement of base items, additive items (`extend_banned`),
/// and subtractive items (`allowed`).
#[derive(Deserialize, Debug, Clone, Default)]
#[cfg_attr(test, derive(Serialize))]
pub struct DenyListConfig {
    /// Explicit replacement for the default base set (e.g., banned words or suffixes).
    /// If `None`, the rule's built-in defaults are used.
    /// If `Some`, completely replaces the default base set.
    #[serde(default)]
    pub banned: Option<HashSet<String>>,

    /// Additional items to include in the banned set.
    #[serde(default)]
    pub extend_banned: HashSet<String>,

    /// Allowed items exempted/removed from the banned set.
    #[serde(default)]
    pub allowed: HashSet<String>,
}

impl DenyListConfig {
    /// The TOML keys of this shape, checked against the serde fields by a test.
    pub const KEYS: &'static [&'static str] = &["banned", "extend_banned", "allowed"];
}

/// Configuration for rules that enforce an allowlist of valid identifiers (e.g. single-letter variable names).
///
/// Supports explicit replacement of base allowed items, additive items (`extend_allowed`),
/// and subtractive/revocation items (`banned`).
#[derive(Deserialize, Debug, Clone, Default)]
#[cfg_attr(test, derive(Serialize))]
pub struct AllowListConfig {
    /// Explicit replacement for the default allowed set.
    /// If `None`, the rule's built-in defaults are used.
    /// If `Some`, completely replaces the default allowed set.
    #[serde(default)]
    pub allowed: Option<HashSet<String>>,

    /// Additional items to include in the allowed set.
    #[serde(default)]
    pub extend_allowed: HashSet<String>,

    /// Banned items revoked/removed from the allowed set.
    #[serde(default)]
    pub banned: HashSet<String>,
}

impl AllowListConfig {
    /// The TOML keys of this shape, checked against the serde fields by a test.
    pub const KEYS: &'static [&'static str] = &["allowed", "extend_allowed", "banned"];
}

/// Enforcement mode for rules targeting sensitive language constructs
/// (e.g. `cast`, `suppress`, `getattr`, `except Exception`).
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EnforcementMode {
    /// Completely bans the construct from targeted files (only suppressible via `# omni:ignore`).
    Ban,
    /// Permits the construct only if accompanied by an explanatory comment.
    RequireExplanation,
}

/// Configuration for rules that support configurable enforcement modes.
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EnforcementConfig {
    /// The enforcement mode (`ban` or `require-explanation`).
    #[serde(default)]
    pub mode: Option<EnforcementMode>,
}

impl EnforcementConfig {
    /// The TOML keys of this shape, checked against the serde fields by a test.
    pub const KEYS: &'static [&'static str] = &["mode"];
}

/// A generic configuration container that supports global settings across all
/// supported languages, as well as dynamic per-language overrides.
#[derive(Deserialize, Debug, Clone, Default)]
pub struct DynamicRuleConfig<T = DenyListConfig> {
    /// Global settings that apply across all languages.
    #[serde(flatten)]
    pub global: T,

    /// Dynamic language-specific overrides, keyed by language name (e.g. "rust", "python").
    #[serde(flatten)]
    pub languages: std::collections::HashMap<String, T>,
}

/// Returns the lowercase canonical configuration key for a supported language.
#[must_use]
const fn support_lang_name(lang: SupportLang) -> &'static str {
    match lang {
        SupportLang::Python => "python",
        SupportLang::Rust => "rust",
        _ => "",
    }
}

impl<T> DynamicRuleConfig<T> {
    /// Returns the override configuration for a specific language, if configured.
    #[must_use]
    pub fn for_lang(&self, lang: SupportLang) -> Option<&T> {
        self.languages.get(support_lang_name(lang))
    }

    /// Resolves an effective value for `lang` with precedence:
    /// 1. Language-specific override (`[rules.<name>.<lang>]`)
    /// 2. Global rule setting (`[rules.<name>]`)
    /// 3. Compile-time language default (`defaults.resolve_default_for_lang(lang)`)
    #[must_use]
    pub fn resolve_with<V: Copy + 'static>(
        &self,
        lang: SupportLang,
        defaults: &LanguageDefaults<V>,
        extractor: impl Fn(&T) -> Option<V>,
    ) -> V {
        self.for_lang(lang)
            .and_then(&extractor)
            .or_else(|| extractor(&self.global))
            .unwrap_or_else(|| defaults.resolve_default_for_lang(lang))
    }
}

impl DynamicRuleConfig<ThresholdConfig> {
    /// Resolves the effective `min` threshold for `lang` against `defaults`.
    #[must_use]
    pub fn effective_min_for_lang(
        &self,
        lang: SupportLang,
        defaults: &LanguageDefaults<usize>,
    ) -> usize {
        self.resolve_with(lang, defaults, |config| config.min)
    }

    /// Resolves the effective `max` threshold for `lang` against `defaults`.
    #[must_use]
    pub fn effective_max_for_lang(
        &self,
        lang: SupportLang,
        defaults: &LanguageDefaults<usize>,
    ) -> usize {
        self.resolve_with(lang, defaults, |config| config.max)
    }
}

impl DynamicRuleConfig<DenyListConfig> {
    /// Computes the effective banned set for a specific language given the default descriptor.
    ///
    /// The resolution precedence is:
    /// 1. Base set: Language-specific `banned` -> Global `banned` -> `defaults.resolve_default_for_lang(lang)`.
    /// 2. Additive: Union with global `extend_banned` and language-specific `extend_banned`.
    /// 3. Subtractive: Difference with global `allowed` and language-specific `allowed`.
    #[must_use]
    pub fn effective_banned_for_lang(
        &self,
        lang: SupportLang,
        defaults: &FilterListDefaults,
    ) -> HashSet<String> {
        let lang_override = self.for_lang(lang);

        // 1. Base set
        let mut effective = lang_override
            .and_then(|override_config| override_config.banned.as_ref())
            .or(self.global.banned.as_ref())
            .map_or_else(|| defaults.resolve_default_for_lang(lang), Clone::clone);

        // 2. Additive
        effective.extend(self.global.extend_banned.iter().cloned());
        if let Some(override_config) = lang_override {
            effective.extend(override_config.extend_banned.iter().cloned());
        }

        // 3. Subtractive
        for item in &self.global.allowed {
            effective.remove(item);
        }
        if let Some(override_config) = lang_override {
            for item in &override_config.allowed {
                effective.remove(item);
            }
        }

        effective
    }
}

impl DynamicRuleConfig<AllowListConfig> {
    /// Computes the effective allowed set for a specific language given the default descriptor.
    ///
    /// The resolution precedence is:
    /// 1. Base set: Language-specific `allowed` -> Global `allowed` -> `defaults.resolve_default_for_lang(lang)`.
    /// 2. Additive: Union with global `extend_allowed` and language-specific `extend_allowed`.
    /// 3. Subtractive: Difference with global `banned` and language-specific `banned`.
    #[must_use]
    pub fn effective_allowed_for_lang(
        &self,
        lang: SupportLang,
        defaults: &FilterListDefaults,
    ) -> HashSet<String> {
        let lang_override = self.for_lang(lang);

        // 1. Base set
        let mut effective = lang_override
            .and_then(|override_config| override_config.allowed.as_ref())
            .or(self.global.allowed.as_ref())
            .map_or_else(|| defaults.resolve_default_for_lang(lang), Clone::clone);

        // 2. Additive
        effective.extend(self.global.extend_allowed.iter().cloned());
        if let Some(override_config) = lang_override {
            effective.extend(override_config.extend_allowed.iter().cloned());
        }

        // 3. Subtractive
        for item in &self.global.banned {
            effective.remove(item);
        }
        if let Some(override_config) = lang_override {
            for item in &override_config.banned {
                effective.remove(item);
            }
        }

        effective
    }
}

impl DynamicRuleConfig<EnforcementConfig> {
    /// Resolves the effective enforcement mode for `lang` against `defaults`.
    #[must_use]
    pub fn effective_mode_for_lang(
        &self,
        lang: SupportLang,
        defaults: &LanguageDefaults<EnforcementMode>,
    ) -> EnforcementMode {
        self.resolve_with(lang, defaults, |config| config.mode)
    }
}

/// The default configuration file name.
pub const CONFIG_FILE_NAME: &str = ".omnilint.toml";

/// The part of a rule that finds violations: its name, message template and languages.
/// Runners execute detectors and never see a rule's classification or doc.
pub trait Detector: Send + Sync {
    /// Returns the rule name (e.g., `RuleName("no-logging-error-in-except")`).
    #[must_use]
    fn name(&self) -> RuleName;

    /// Returns the single violation template for this rule (`1 Rule = 1 Template`).
    #[must_use]
    fn violation_template(&self) -> &'static ViolationTemplate;

    /// Returns the languages analyzed by this rule. Command rules default to an empty slice.
    #[must_use]
    fn supported_languages(&self) -> &'static [SupportLang] {
        &[]
    }

    /// Returns the default enforcement mode for this rule across languages.
    /// Most rules default to `EnforcementMode::Ban`.
    #[must_use]
    fn default_enforcement_mode(&self) -> LanguageDefaults<EnforcementMode> {
        LanguageDefaults {
            base: EnforcementMode::Ban,
            overrides: &[],
        }
    }

    /// Resolves the effective enforcement mode for this rule given language and config.
    #[must_use]
    fn enforcement_mode(&self, lang: SupportLang, config: &Config) -> EnforcementMode {
        config.get_rule_enforcement_mode(self.name().0, lang, &self.default_enforcement_mode())
    }

    /// Resolves the effective banned set for this rule given language, config, and defaults.
    #[must_use]
    fn effective_banned_set(
        &self,
        lang: SupportLang,
        config: &Config,
        defaults: &FilterListDefaults,
    ) -> HashSet<String> {
        config
            .get_rule_config::<DynamicRuleConfig<DenyListConfig>>(self.name().0)
            .effective_banned_for_lang(lang, defaults)
    }

    /// Resolves the effective allowed set for this rule given language, config, and defaults.
    #[must_use]
    fn effective_allowed_set(
        &self,
        lang: SupportLang,
        config: &Config,
        defaults: &FilterListDefaults,
    ) -> HashSet<String> {
        config
            .get_rule_config::<DynamicRuleConfig<AllowListConfig>>(self.name().0)
            .effective_allowed_for_lang(lang, defaults)
    }

    /// Resolves the effective `min` threshold for this rule given language, config, and defaults.
    #[must_use]
    fn effective_min_threshold(
        &self,
        lang: SupportLang,
        config: &Config,
        defaults: &LanguageDefaults<usize>,
    ) -> usize {
        config
            .get_rule_config::<DynamicRuleConfig<ThresholdConfig>>(self.name().0)
            .effective_min_for_lang(lang, defaults)
    }

    /// Resolves the effective `max` threshold for this rule given language, config, and defaults.
    #[must_use]
    fn effective_max_threshold(
        &self,
        lang: SupportLang,
        config: &Config,
        defaults: &LanguageDefaults<usize>,
    ) -> usize {
        config
            .get_rule_config::<DynamicRuleConfig<ThresholdConfig>>(self.name().0)
            .effective_max_for_lang(lang, defaults)
    }

    /// Constructs a `Diagnostic` with this rule's name.
    #[must_use]
    fn create_diagnostic(&self, message: ViolationMessage, location: SourceLocation) -> Diagnostic {
        Diagnostic::new(self.name(), message, location)
    }

    /// Renders a diagnostic using the base template (for command and language-independent rules).
    #[must_use]
    fn render_diagnostic(&self, params: &[(&str, &str)], location: SourceLocation) -> Diagnostic {
        self.create_diagnostic(self.violation_template().render(params), location)
    }

    /// Renders a diagnostic for a specific programming language (for code rules).
    #[must_use]
    fn render_diagnostic_for_lang(
        &self,
        lang: SupportLang,
        params: &[(&str, &str)],
        location: SourceLocation,
    ) -> Diagnostic {
        self.create_diagnostic(
            self.violation_template().render_for_lang(lang, params),
            location,
        )
    }
}

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
    /// Generic map of rule-specific configurations.
    #[serde(default)]
    pub rules: std::collections::HashMap<String, serde_json::Value>,
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

    /// Deserializes a rule-specific configuration.
    /// Returns default value if not present or fails to deserialize.
    #[must_use]
    pub fn get_rule_config<T>(&self, rule_name: &str) -> T
    where
        T: serde::de::DeserializeOwned + Default,
    {
        self.rules
            .get(rule_name)
            .and_then(|val| T::deserialize(val).ok())
            .unwrap_or_default()
    }

    /// Resolves the effective enforcement mode for a rule and language against defaults.
    #[must_use]
    pub fn get_rule_enforcement_mode(
        &self,
        rule_name: &str,
        lang: SupportLang,
        defaults: &LanguageDefaults<EnforcementMode>,
    ) -> EnforcementMode {
        let rule_config: DynamicRuleConfig<EnforcementConfig> = self.get_rule_config(rule_name);
        rule_config.effective_mode_for_lang(lang, defaults)
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

    #[test]
    fn test_dynamic_deny_list_config() {
        const DEFAULTS: FilterListDefaults = FilterListDefaults {
            base: &["default_one", "common_ok"],
            extend: &[],
            exempt: &[],
        };

        let toml_content = indoc::indoc! {r#"
            allowed = ["common_ok"]
            extend_banned = ["global_bad"]

            [rust]
            allowed = ["rust_ok"]
            extend_banned = ["rust_bad"]

            [python]
            banned = ["py_only_bad"]
        "#};

        let config: DynamicRuleConfig<DenyListConfig> = toml::from_str(toml_content).unwrap();

        // Rust resolution:
        // Base: default_banned ("default_one", "common_ok")
        // Additive: global ("global_bad") + rust ("rust_bad")
        // Subtractive: global ("common_ok") + rust ("rust_ok")
        let rust_effective = config.effective_banned_for_lang(SupportLang::Rust, &DEFAULTS);
        assert_eq!(
            rust_effective,
            HashSet::from(["default_one", "global_bad", "rust_bad"].map(String::from))
        );

        // Python resolution:
        // Base: explicit python banned ("py_only_bad")
        // Additive: global ("global_bad")
        // Subtractive: global ("common_ok")
        let py_effective = config.effective_banned_for_lang(SupportLang::Python, &DEFAULTS);
        assert_eq!(
            py_effective,
            HashSet::from(["py_only_bad", "global_bad"].map(String::from))
        );
    }

    #[test]
    fn test_dynamic_allow_list_config() {
        const DEFAULTS: FilterListDefaults = FilterListDefaults {
            base: &["default_base", "revoked", "rust_revoked"],
            extend: &[(SupportLang::Rust, &["rust_extra"])],
            exempt: &[],
        };

        let toml_content = indoc::indoc! {r#"
            banned = ["revoked"]
            extend_allowed = ["global_allowed"]

            [rust]
            banned = ["rust_revoked"]
            extend_allowed = ["rust_allowed"]

            [python]
            allowed = ["py_only_allowed"]
        "#};

        let config: DynamicRuleConfig<AllowListConfig> = toml::from_str(toml_content).unwrap();

        // Rust resolution:
        // Base: default_base, revoked, rust_revoked + rust_extra
        // Additive: global ("global_allowed") + rust ("rust_allowed")
        // Subtractive: global ("revoked") + rust ("rust_revoked")
        let rust_effective = config.effective_allowed_for_lang(SupportLang::Rust, &DEFAULTS);
        assert_eq!(
            rust_effective,
            HashSet::from(
                [
                    "default_base",
                    "rust_extra",
                    "global_allowed",
                    "rust_allowed",
                ]
                .map(String::from)
            )
        );

        // Python resolution:
        // Base: explicit python allowed ("py_only_allowed")
        // Additive: global ("global_allowed")
        // Subtractive: global ("revoked")
        let py_effective = config.effective_allowed_for_lang(SupportLang::Python, &DEFAULTS);
        assert_eq!(
            py_effective,
            HashSet::from(["py_only_allowed", "global_allowed"].map(String::from))
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
    #[case("", SupportLang::Python, 4)]
    #[case("", SupportLang::Rust, 6)]
    #[case("max = 5", SupportLang::Python, 5)]
    #[case("max = 5", SupportLang::Rust, 5)]
    #[case("max = 5\n[rust]\nmax = 10", SupportLang::Python, 5)]
    #[case("max = 5\n[rust]\nmax = 10", SupportLang::Rust, 10)]
    fn test_language_defaults_and_threshold_config(
        #[case] toml_content: &str,
        #[case] lang: SupportLang,
        #[case] expected: usize,
    ) {
        const DEFAULTS: LanguageDefaults<usize> =
            LanguageDefaults::new(4, &[(SupportLang::Rust, 6)]);
        let config: DynamicRuleConfig<ThresholdConfig> = if toml_content.is_empty() {
            DynamicRuleConfig::default()
        } else {
            toml::from_str(toml_content).unwrap()
        };
        assert_eq!(config.effective_max_for_lang(lang, &DEFAULTS), expected);
    }

    #[rstest::rstest]
    #[case("", SupportLang::Python, EnforcementMode::Ban)]
    #[case("", SupportLang::Rust, EnforcementMode::RequireExplanation)]
    #[case("mode = \"ban\"", SupportLang::Rust, EnforcementMode::Ban)]
    #[case(
        "mode = \"require-explanation\"",
        SupportLang::Python,
        EnforcementMode::RequireExplanation
    )]
    #[case(
        "mode = \"ban\"\n[python]\nmode = \"require-explanation\"",
        SupportLang::Python,
        EnforcementMode::RequireExplanation
    )]
    #[case(
        "mode = \"ban\"\n[python]\nmode = \"require-explanation\"",
        SupportLang::Rust,
        EnforcementMode::Ban
    )]
    fn test_dynamic_enforcement_config(
        #[case] toml_content: &str,
        #[case] lang: SupportLang,
        #[case] expected: EnforcementMode,
    ) {
        const DEFAULTS: LanguageDefaults<EnforcementMode> = LanguageDefaults::new(
            EnforcementMode::Ban,
            &[(SupportLang::Rust, EnforcementMode::RequireExplanation)],
        );
        let config: DynamicRuleConfig<EnforcementConfig> = if toml_content.is_empty() {
            DynamicRuleConfig::default()
        } else {
            toml::from_str(toml_content).unwrap()
        };
        assert_eq!(config.effective_mode_for_lang(lang, &DEFAULTS), expected);
    }

    #[rstest::rstest]
    #[case::threshold(
        serde_json::to_value(ThresholdConfig::default()),
        ThresholdConfig::KEYS
    )]
    #[case::deny_list(serde_json::to_value(DenyListConfig::default()), DenyListConfig::KEYS)]
    #[case::allow_list(
        serde_json::to_value(AllowListConfig::default()),
        AllowListConfig::KEYS
    )]
    #[case::enforcement(
        serde_json::to_value(EnforcementConfig::default()),
        EnforcementConfig::KEYS
    )]
    fn test_config_keys_match_serde_fields(
        #[case] serialized: serde_json::Result<serde_json::Value>,
        #[case] keys: &[&str],
    ) {
        let serialized = serialized.unwrap();
        let fields: HashSet<&str> = serialized
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(fields, keys.iter().copied().collect());
    }
}
