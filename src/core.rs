//! Shared core module of the Omni linter toolkit.

use crate::diagnostic::{RuleCode, RuleName};
use serde::Deserialize;
use std::collections::HashSet;

/// The default configuration file name.
pub const CONFIG_FILE_NAME: &str = ".omnilint.toml";

/// Common metadata shared by all lint rules.
pub trait Rule: Send + Sync {
    /// Returns the unique rule code (e.g., "PY001").
    fn code(&self) -> RuleCode;
    /// Returns the rule name (e.g., "no-logging-in-except").
    fn name(&self) -> RuleName;
    /// Returns the list of tags associated with the rule.
    fn tags(&self) -> &'static [crate::rules::Tag];
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
}

impl Config {
    /// Returns true if the given rule is enabled in this configuration.
    #[must_use]
    pub fn is_rule_enabled(&self, rule: &dyn Rule) -> bool {
        let code = rule.code();
        let name = rule.name();

        let matches_selector = |sel: &Selector| match sel {
            Selector::Code(code_selector) => *code_selector == code,
            Selector::Name(name_selector) => *name_selector == name,
            Selector::Tag(tag_selector) => rule.tags().contains(tag_selector),
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

    /// Loads configuration settings from the default `.omnilint.toml` in the current directory.
    /// Returns default settings if the file does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the file is present but fails to read or has syntax errors.
    pub fn load() -> anyhow::Result<Self> {
        match std::fs::read_to_string(CONFIG_FILE_NAME) {
            Ok(content) => {
                let config = toml::from_str(&content)?;
                Ok(config)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(anyhow::anyhow!(
                "failed to read `{CONFIG_FILE_NAME}`: {error}"
            )),
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

    const LOGGING_RULE: MockRule = MockRule {
        code: "T001",
        name: "mock-logging-rule",
        tags: &[Tag::Logging],
    };

    const STYLE_RULE: MockRule = MockRule {
        code: "T002",
        name: "mock-style-rule",
        tags: &[Tag::Style],
    };

    #[test]
    fn test_select_by_tag() {
        let mut select = HashSet::new();
        select.insert(Selector::Tag(Tag::Logging));
        let config = Config {
            select: Some(select),
            ignore: None,
            rules: std::collections::HashMap::new(),
        };

        assert!(config.is_rule_enabled(&LOGGING_RULE));
        assert!(!config.is_rule_enabled(&STYLE_RULE));
    }

    #[test]
    fn test_ignore_by_tag() {
        let mut ignore = HashSet::new();
        ignore.insert(Selector::Tag(Tag::Logging));
        let config = Config {
            select: None,
            ignore: Some(ignore),
            rules: std::collections::HashMap::new(),
        };

        assert!(!config.is_rule_enabled(&LOGGING_RULE));
        assert!(config.is_rule_enabled(&STYLE_RULE));
    }

    #[test]
    fn test_tag_description() {
        assert_eq!(
            Tag::Logging.description(),
            "Checks related to logging configurations and invocations"
        );
        assert_eq!(
            Tag::Exceptions.description(),
            "Checks targeting exception handling structures"
        );
    }



    #[test]
    fn test_selector_deserialization() {
        let toml_content = r#"
            select = ["logging", "VCS001"]
        "#;
        let config: Config = toml::from_str(toml_content).unwrap();
        let selectors = config.select.unwrap();
        assert_eq!(selectors.len(), 2);
        assert!(selectors.contains(&Selector::Tag(Tag::Logging)));
        assert!(selectors.contains(&Selector::Code(RuleCode("VCS001"))));
    }
}
