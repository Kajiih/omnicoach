//! Shared core module of the Omni linter toolkit.

use crate::diagnostic::{RuleCode, RuleName};
use ast_grep_language::SupportLang;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

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
    /// Creates a new static default filter list descriptor.
    #[must_use]
    pub const fn new(
        base: &'static [&'static str],
        extend: &'static [(SupportLang, &'static [&'static str])],
        exempt: &'static [(SupportLang, &'static [&'static str])],
    ) -> Self {
        Self { base, extend, exempt }
    }

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

/// Configuration for rules controlled by a numeric `max` threshold (e.g., `max-test-assertions`).
#[derive(Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ThresholdConfig {
    /// Optional override for the maximum threshold.
    #[serde(default)]
    pub max: Option<usize>,
}

/// Configuration for rules that filter identifier names, abbreviations, or suffixes (denylist rules).
///
/// Supports explicit replacement of base items, additive items (`extend_banned`),
/// and subtractive items (`allowed`).
#[derive(Deserialize, Debug, Clone, Default)]
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

/// Configuration for rules that enforce an allowlist of valid identifiers (e.g. single-letter variable names).
///
/// Supports explicit replacement of base allowed items, additive items (`extend_allowed`),
/// and subtractive/revocation items (`banned`).
#[derive(Deserialize, Debug, Clone, Default)]
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
pub const fn support_lang_name(lang: SupportLang) -> &'static str {
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

/// The default configuration file name.
pub const CONFIG_FILE_NAME: &str = ".omnilint.toml";

/// Common metadata shared by all lint rules.
pub trait Rule: Send + Sync {
    /// Returns the unique rule code (e.g., "LOG-001").
    fn code(&self) -> RuleCode;
    /// Returns the rule name (e.g., "no-logging-in-except").
    fn name(&self) -> RuleName;
    /// Returns the domain tags of the rule (language tags are derived from `supported_languages`).
    fn tags(&self) -> &'static [crate::rules::Tag];

    /// Returns the languages analyzed by this rule. Non-language rules return an empty slice.
    #[must_use]
    fn supported_languages(&self) -> &'static [SupportLang] {
        &[]
    }

    /// Returns true if the rule carries the given tag, including language tags
    /// derived from `supported_languages`.
    #[must_use]
    fn has_tag(&self, tag: crate::rules::Tag) -> bool {
        self.tags().contains(&tag)
            || tag.to_support_lang().is_some_and(|lang| self.supported_languages().contains(&lang))
    }
}

/// A filter selector parsed from linter configuration settings.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Selector {
    /// Matches a specific rule code.
    Code(RuleCode),
    /// Matches a specific rule name.
    Name(RuleName),
    /// Matches all rules under a category tag.
    Tag(crate::rules::Tag),
}

impl<'de> Deserialize<'de> for Selector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let selector_input = String::deserialize(deserializer)?;

        // TODO(roadmap): Avoid coupling selector deserialization directly to static registries.
        // This prevents dynamic/declarative rules from being loaded via configurations.
        // 1. Try to parse as Tag
        if let Ok(tag) = selector_input.parse::<crate::rules::Tag>() {
            return Ok(Self::Tag(tag));
        }

        // 2. Try to parse as Code or Name from registries
        for rule in crate::rules::CODE_RULES {
            if rule.code().0 == selector_input {
                return Ok(Self::Code(rule.code()));
            }
            if rule.name().0 == selector_input {
                return Ok(Self::Name(rule.name()));
            }
        }

        for rule in crate::rules::COMMAND_RULES {
            if rule.code().0 == selector_input {
                return Ok(Self::Code(rule.code()));
            }
            if rule.name().0 == selector_input {
                return Ok(Self::Name(rule.name()));
            }
        }

        Err(serde::de::Error::custom(format!(
            "invalid rule selector '{selector_input}'. Must be a valid rule code, rule name, or category tag."
        )))
    }
}

/// Configuration settings for path context detection (e.g. test paths).
#[derive(Deserialize, Debug, Clone)]
pub struct ContextConfig {
    /// Glob patterns used to identify test files.
    #[serde(default = "default_test_patterns")]
    pub test_patterns: Vec<String>,
}

fn default_test_patterns() -> Vec<String> {
    vec![
        "**/tests/**".to_string(),
        "**/test_*.py".to_string(),
        "**/*_test.py".to_string(),
        "**/*_test.rs".to_string(),
        "**/tests.rs".to_string(),
    ]
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self { test_patterns: default_test_patterns() }
    }
}

/// Configuration settings parsed from `.omnilint.toml`.
#[derive(Deserialize, Debug, Default, Clone)]
pub struct Config {
    /// Optional set of selected rules or tags to run.
    pub select: Option<HashSet<Selector>>,
    /// Optional set of rules or tags to ignore.
    pub ignore: Option<HashSet<Selector>>,
    /// Generic map of rule-specific configurations.
    #[serde(default)]
    pub rules: std::collections::HashMap<String, serde_json::Value>,
    /// Path context classifier settings.
    #[serde(default)]
    pub context: ContextConfig,
    /// Per-file rule ignores mapping glob patterns to rule selectors.
    #[serde(default)]
    pub per_file_ignores: std::collections::HashMap<String, HashSet<Selector>>,
}

/// Errors encountered during configuration loading and parsing.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Failed to read the configuration file from disk.
    #[error("failed to read `{path}`: {source}")]
    Io {
        /// Path to the configuration file.
        path: &'static str,
        /// The underlying IO error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to parse TOML configuration syntax or schema.
    #[error(transparent)]
    Toml(#[from] toml::de::Error),
}

fn normalize_path_for_glob(path: &Path) -> String {
    let relative = if path.is_absolute() {
        std::env::current_dir().ok().and_then(|cwd| path.strip_prefix(cwd).ok()).unwrap_or(path)
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
        let normalized = normalize_path_for_glob(path);
        for pattern in &self.context.test_patterns {
            if let Ok(glob) = globset::GlobBuilder::new(pattern).literal_separator(false).build() {
                if glob.compile_matcher().is_match(&normalized) {
                    return true;
                }
            }
        }
        false
    }

    /// Returns true if the given rule is enabled in this configuration.
    #[must_use]
    pub fn is_rule_enabled(&self, rule: &dyn Rule) -> bool {
        let code = rule.code();
        let name = rule.name();

        let matches_selector = |sel: &Selector| match sel {
            Selector::Code(code_selector) => *code_selector == code,
            Selector::Name(name_selector) => *name_selector == name,
            Selector::Tag(tag_selector) => rule.has_tag(*tag_selector),
        };

        if let Some(ref select) = self.select {
            if !select.iter().any(matches_selector) {
                return false;
            }
        }

        if let Some(ref ignore) = self.ignore {
            if ignore.iter().any(matches_selector) {
                return false;
            }
        }

        true
    }

    /// Returns true if the given rule is enabled for a specific file path.
    #[must_use]
    pub fn is_rule_enabled_for_path(&self, rule: &dyn Rule, path: &Path) -> bool {
        if !self.is_rule_enabled(rule) {
            return false;
        }

        let normalized = normalize_path_for_glob(path);
        let code = rule.code();
        let name = rule.name();

        let matches_selector = |sel: &Selector| match sel {
            Selector::Code(code_selector) => *code_selector == code,
            Selector::Name(name_selector) => *name_selector == name,
            Selector::Tag(tag_selector) => rule.has_tag(*tag_selector),
        };

        for (pattern, selectors) in &self.per_file_ignores {
            if let Ok(glob) = globset::GlobBuilder::new(pattern).literal_separator(false).build() {
                if glob.compile_matcher().is_match(&normalized)
                    && selectors.iter().any(matches_selector)
                {
                    return false;
                }
            }
        }

        true
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
            .and_then(|val| serde_json::from_value(val.clone()).ok())
            .unwrap_or_default()
    }

    /// Loads configuration settings from the default `.omnilint.toml` in the current directory.
    /// Returns default settings if the file does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] if the file is present but fails to read or has syntax errors.
    pub fn load() -> Result<Self, ConfigError> {
        match std::fs::read_to_string(CONFIG_FILE_NAME) {
            Ok(content) => {
                let config = toml::from_str(&content)?;
                Ok(config)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(ConfigError::Io { path: CONFIG_FILE_NAME, source: error }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Tag;

    struct MockRule {
        code: &'static str,
        name: &'static str,
        tags: &'static [Tag],
    }

    impl Rule for MockRule {
        fn code(&self) -> RuleCode {
            RuleCode(self.code)
        }
        fn name(&self) -> RuleName {
            RuleName(self.name)
        }
        fn tags(&self) -> &'static [Tag] {
            self.tags
        }
    }

    const LOGGING_RULE: MockRule =
        MockRule { code: "T001", name: "mock-logging-rule", tags: &[Tag::Logging] };

    const STYLE_RULE: MockRule =
        MockRule { code: "T002", name: "mock-style-rule", tags: &[Tag::Style] };

    #[test]
    fn test_select_by_tag() {
        let mut select = HashSet::new();
        select.insert(Selector::Tag(Tag::Logging));
        let config = Config { select: Some(select), ignore: None, ..Default::default() };

        assert!(config.is_rule_enabled(&LOGGING_RULE));
        assert!(!config.is_rule_enabled(&STYLE_RULE));
    }

    #[test]
    fn test_ignore_by_tag() {
        let mut ignore = HashSet::new();
        ignore.insert(Selector::Tag(Tag::Logging));
        let config = Config { select: None, ignore: Some(ignore), ..Default::default() };

        assert!(!config.is_rule_enabled(&LOGGING_RULE));
        assert!(config.is_rule_enabled(&STYLE_RULE));
    }

    #[test]
    fn test_tag_description() {
        assert_eq!(
            Tag::Logging.description(),
            "Checks related to logging configurations and invocations"
        );
        assert_eq!(Tag::Exceptions.description(), "Checks targeting exception handling structures");
    }

    #[test]
    fn test_selector_deserialization() {
        let toml_content = r#"
            select = ["logging", "JJ-001"]
        "#;
        let config: Config = toml::from_str(toml_content).unwrap();
        let selectors = config.select.unwrap();
        assert_eq!(selectors.len(), 2);
        assert!(selectors.contains(&Selector::Tag(Tag::Logging)));
        assert!(selectors.contains(&Selector::Code(RuleCode("JJ-001"))));
    }

    #[test]
    fn test_filter_list_defaults_resolve() {
        const DEFAULTS: FilterListDefaults = FilterListDefaults {
            base: &["common", "shared", "temp"],
            extend: &[(SupportLang::Rust, &["rust_only"])],
            exempt: &[(SupportLang::Rust, &["temp"])],
        };

        let python_defaults = DEFAULTS.resolve_default_for_lang(SupportLang::Python);
        assert_eq!(python_defaults, HashSet::from(["common", "shared", "temp"].map(String::from)));

        let rust_defaults = DEFAULTS.resolve_default_for_lang(SupportLang::Rust);
        assert_eq!(
            rust_defaults,
            HashSet::from(["common", "shared", "rust_only"].map(String::from))
        );
    }

    #[test]
    fn test_dynamic_deny_list_config() {
        const DEFAULTS: FilterListDefaults =
            FilterListDefaults { base: &["default_one", "common_ok"], extend: &[], exempt: &[] };

        let toml_content = r#"
            allowed = ["common_ok"]
            extend_banned = ["global_bad"]

            [rust]
            allowed = ["rust_ok"]
            extend_banned = ["rust_bad"]

            [python]
            banned = ["py_only_bad"]
        "#;

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
        assert_eq!(py_effective, HashSet::from(["py_only_bad", "global_bad"].map(String::from)));
    }

    #[test]
    fn test_dynamic_allow_list_config() {
        const DEFAULTS: FilterListDefaults = FilterListDefaults {
            base: &["default_base", "revoked", "rust_revoked"],
            extend: &[(SupportLang::Rust, &["rust_extra"])],
            exempt: &[],
        };

        let toml_content = r#"
            banned = ["revoked"]
            extend_allowed = ["global_allowed"]

            [rust]
            banned = ["rust_revoked"]
            extend_allowed = ["rust_allowed"]

            [python]
            allowed = ["py_only_allowed"]
        "#;

        let config: DynamicRuleConfig<AllowListConfig> = toml::from_str(toml_content).unwrap();

        // Rust resolution:
        // Base: default_base, revoked, rust_revoked + rust_extra
        // Additive: global ("global_allowed") + rust ("rust_allowed")
        // Subtractive: global ("revoked") + rust ("rust_revoked")
        let rust_effective = config.effective_allowed_for_lang(SupportLang::Rust, &DEFAULTS);
        assert_eq!(
            rust_effective,
            HashSet::from(
                ["default_base", "rust_extra", "global_allowed", "rust_allowed",].map(String::from)
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
    fn test_per_file_ignores() {
        let toml_content = r#"
            [per_file_ignores]
            "tests/**" = ["style"]
        "#;
        let config: Config = toml::from_str(toml_content).unwrap();

        assert!(!config.is_rule_enabled_for_path(&STYLE_RULE, Path::new("tests/my_test.rs")));
        assert!(config.is_rule_enabled_for_path(&STYLE_RULE, Path::new("src/lib.rs")));
        assert!(config.is_rule_enabled_for_path(&LOGGING_RULE, Path::new("tests/my_test.rs")));
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
}
