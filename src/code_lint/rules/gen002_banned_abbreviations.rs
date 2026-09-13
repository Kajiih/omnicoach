//! Rule targeting banned abbreviations in definitions across multiple languages.

use crate::code_lint::CodeRule;
use crate::core::Rule;
use crate::diagnostic::{
    Diagnostic, LocationContext, RuleCode, RuleName, SourceLocation, SourceSpan, ViolationMessage,
};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

/// Configuration for the `BannedAbbreviations` rule.
#[derive(Deserialize, Debug, Clone)]
pub struct BannedAbbreviationsConfig {
    /// Banned abbreviation words.
    #[serde(default = "default_banned")]
    pub banned: HashSet<String>,
}

impl Default for BannedAbbreviationsConfig {
    fn default() -> Self {
        Self {
            banned: default_banned(),
        }
    }
}

fn default_banned() -> HashSet<String> {
    // Currently allowed: prev, curr, arg
    [
        "err", "ctx", "cfg", "res", "msg", "str", "num", "btn", "cb", "ch", "diag",
    ]
    .into_iter()
    .map(ToString::to_string)
    .collect()
}

/// Helper to split identifiers into sub-word segments.
fn split_segments(name: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();

    let chars: Vec<char> = name.chars().collect();
    for i in 0..chars.len() {
        let c = chars[i];
        if c == '_' {
            if !current.is_empty() {
                segments.push(current.to_lowercase());
                current = String::new();
            }
            continue;
        }

        // Split at lowercase/digit -> uppercase transition
        if i > 0 && c.is_uppercase() {
            let prev = chars[i - 1];
            if (prev.is_lowercase() || prev.is_numeric()) && !current.is_empty() {
                segments.push(current.to_lowercase());
                current = String::new();
            }
        }

        current.push(c);
    }

    if !current.is_empty() {
        segments.push(current.to_lowercase());
    }

    segments
}

/// Rule that bans abbreviations in identifier bindings.
pub struct BannedAbbreviations;

impl Rule for BannedAbbreviations {
    fn code(&self) -> RuleCode {
        RuleCode("GEN002")
    }

    fn name(&self) -> RuleName {
        RuleName("banned-abbreviations")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Style, Tag::Python, Tag::Rust]
    }
}

impl CodeRule for BannedAbbreviations {
    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<crate::code_lint::SourceDoc>,
        config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        let rule_config: BannedAbbreviationsConfig = config.get_rule_config(self.name().0);

        let mut diagnostics = Vec::new();

        let bindings = crate::code_lint::collect_bindings(grep);

        for node in bindings {
            let name = node.text();
            let segments = split_segments(&name);
            for segment in segments {
                if rule_config.banned.contains(&segment) {
                    diagnostics.push(Diagnostic::new(
                        self.code(),
                        self.name(),
                        ViolationMessage {
                            summary: format!("Definition name `{name}` contains banned abbreviation `{segment}`."),
                            rationale: "Banned abbreviations make identifier names less clear, harder to read, and difficult to search for.".to_string(),
                            suggestion: "Rename the identifier using full words or a non-banned term.".to_string(),
                        },
                        SourceLocation {
                            context: LocationContext::File(path.to_path_buf()),
                            span: SourceSpan {
                                start: node.range().start,
                                end: node.range().end,
                            },
                        },
                    ));
                    // Flag each node at most once
                    break;
                }
            }
        }
        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{assert_code_rule_snapshot, assert_code_rule_snapshot_with_config};

    #[test]
    fn test_rust_snapshots() {
        let rule = BannedAbbreviations;

        // Banned variables, functions, and structs
        let source = r"
            use std::collections::HashMap as my_cfg;
            fn process_err() {
                let ctx = 1;
                let my_cfg_val = 2;
            }
            struct MyRes;
        ";
        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, source, "test.rs"), @r###"
        [GEN002] Line 2, Col 46: Definition name `my_cfg` contains banned abbreviation `cfg`.
        [GEN002] Line 3, Col 16: Definition name `process_err` contains banned abbreviation `err`.
        [GEN002] Line 4, Col 21: Definition name `ctx` contains banned abbreviation `ctx`.
        [GEN002] Line 5, Col 21: Definition name `my_cfg_val` contains banned abbreviation `cfg`.
        [GEN002] Line 7, Col 20: Definition name `MyRes` contains banned abbreviation `res`.
        "###);
    }

    #[test]
    fn test_python_snapshots() {
        let rule = BannedAbbreviations;

        let source = r#"
import os as os_cfg
def handle_msg(msg):
    str_val = "hello"
    pass
        "#;
        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, source, "test.py"), @r###"
        [GEN002] Line 2, Col 14: Definition name `os_cfg` contains banned abbreviation `cfg`.
        [GEN002] Line 3, Col 5: Definition name `handle_msg` contains banned abbreviation `msg`.
        [GEN002] Line 3, Col 16: Definition name `msg` contains banned abbreviation `msg`.
        [GEN002] Line 4, Col 5: Definition name `str_val` contains banned abbreviation `str`.
        "###);
    }

    #[test]
    fn test_configuration_override() {
        let rule = BannedAbbreviations;

        // Custom banned list: ban only 'foo' and 'bar'
        let config_toml = r#"
            [rules.banned-abbreviations]
            banned = ["foo", "bar"]
        "#;
        let config: crate::core::Config = toml::from_str(config_toml).unwrap();

        // 'err' should be allowed now, but 'my_foo' should violate
        let source = "fn main() { let err = 1; let my_foo = 2; }";
        insta::assert_snapshot!(assert_code_rule_snapshot_with_config(&rule, source, "test.rs", &config), @r###"
        [GEN002] Line 1, Col 30: Definition name `my_foo` contains banned abbreviation `foo`.
        "###);
    }
}
