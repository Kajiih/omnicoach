//! Project configuration resolved from `.omnilint.toml`.

architecture_component!(Config);

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;

use crate::diagnostic::RuleName;
use crate::rule_declaration::RuleOverrides;

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

fn compile_glob_set<'a>(
    patterns: impl IntoIterator<Item = &'a str>,
) -> Result<globset::GlobSet, globset::Error> {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(compile_glob(pattern)?);
    }
    builder.build()
}

/// Configuration settings for path context detection (e.g. test paths).
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
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

/// Configuration settings resolved from `.omnilint.toml` by `rule_selection::parse_config`.
#[derive(Debug, Default, Clone)]
pub struct Config {
    /// Rules disabled by `select` and `ignore`.
    pub disabled_rules: HashSet<RuleName>,
    /// Validated `[rules.<name>]` options, keyed by rule.
    pub rule_overrides: HashMap<RuleName, RuleOverrides>,
    /// Path context classifier settings.
    pub context: ContextConfig,
    /// Per-file rule ignores: glob patterns paired with the rules they disable.
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

    /// Returns true if the given rule is enabled in this configuration.
    #[must_use]
    pub fn is_rule_enabled(&self, rule: RuleName) -> bool {
        !self.disabled_rules.contains(&rule)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGGING_RULE: RuleName = RuleName("mock-logging-rule");
    const STYLE_RULE: RuleName = RuleName("mock-style-rule");

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
        let context: ContextConfig = toml::from_str(r#"test-patterns = ["**/spec/**"]"#).unwrap();
        let config = Config {
            context,
            ..Default::default()
        };

        assert!(config.is_test_path(Path::new("app/spec/model.py")));
        assert!(!config.is_test_path(Path::new("tests/foo.rs")));
    }

    #[rstest::rstest]
    #[case::test_patterns("test-patterns = [\"src/[\"]")]
    fn test_invalid_glob_is_config_error(#[case] toml_content: &str) {
        let error = toml::from_str::<ContextConfig>(toml_content).unwrap_err();
        assert!(error.to_string().contains("src/["), "{error}");
    }
}
