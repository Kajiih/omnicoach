//! Rule-selection and per-rule override configuration resolved from `.omnicoach.toml`.

architecture_component!(Config);

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::code_lint::project::ProjectConfig;
use crate::diagnostic::RuleName;
use crate::rule_declaration::RuleOverrides;

/// Configuration settings resolved from `.omnicoach.toml` by `rule_selection::parse_config`.
#[derive(Debug, Default, Clone)]
pub struct Config {
    /// Rules disabled by `select` and `ignore`.
    pub disabled_rules: HashSet<RuleName>,
    /// Validated `[rules.<name>]` options, keyed by rule.
    pub rule_overrides: HashMap<RuleName, RuleOverrides>,
    /// Project layout, path context (`test-patterns`, `source-roots`), and `[architecture]` settings.
    pub project: ProjectConfig,
    /// Per-file rule ignores: glob patterns paired with the rules they disable.
    pub per_file_ignores: Vec<(globset::GlobMatcher, HashSet<RuleName>)>,
    /// Problems that do not prevent loading, one message each, such as a `select` or
    /// `ignore` entry that changes no rule's outcome.
    pub warnings: Vec<String>,
}

impl Config {
    /// Returns true if the given path matches any configured test pattern.
    #[must_use]
    pub fn is_test_path(&self, path: &Path) -> bool {
        self.project.is_test_path(path)
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

        let normalized = self.project.normalize_path_for_glob(path);
        !self
            .per_file_ignores
            .iter()
            .any(|(matcher, rules)| matcher.is_match(&normalized) && rules.contains(&rule))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::code_lint::project::compile_glob;

    const LOGGING_RULE: RuleName = RuleName("mock-logging-rule");
    const STYLE_RULE: RuleName = RuleName("mock-style-rule");

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
    fn test_root_dir_strips_prefix_for_per_file_ignores() {
        let config = Config {
            per_file_ignores: vec![(
                compile_glob("tests/**").unwrap().compile_matcher(),
                HashSet::from([STYLE_RULE]),
            )],
            project: ProjectConfig {
                root_dir: Some(PathBuf::from("/repo")),
                ..ProjectConfig::default()
            },
            ..Default::default()
        };

        // Absolute path under root
        assert!(!config.is_rule_enabled_for_path(STYLE_RULE, Path::new("/repo/tests/cli.rs")));
        assert!(config.is_rule_enabled_for_path(STYLE_RULE, Path::new("/repo/src/lib.rs")));

        // Path relative to root
        assert!(!config.is_rule_enabled_for_path(STYLE_RULE, Path::new("tests/cli.rs")));
        assert!(config.is_rule_enabled_for_path(STYLE_RULE, Path::new("src/lib.rs")));
    }
}
