//! `GEN003`: Bans type suffixes (Hungarian notation) in variable names.

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

/// Configuration for the `NoHungarianNotation` rule.
#[derive(Deserialize, Debug, Clone)]
pub struct NoHungarianNotationConfig {
    /// Suffixes that are banned.
    #[serde(default = "default_banned_suffixes")]
    pub banned_suffixes: HashSet<String>,
}

impl Default for NoHungarianNotationConfig {
    fn default() -> Self {
        Self {
            banned_suffixes: default_banned_suffixes(),
        }
    }
}

fn default_banned_suffixes() -> HashSet<String> {
    [
        "_list", "_arr", "_dict", "_map", "_vec", "_str", "_int", "_bool", "_set", "_ptr", "_num",
        "_float", "_byte",
    ]
    .into_iter()
    .map(ToString::to_string)
    .collect()
}

/// Rule that bans Hungarian notation type suffixes.
pub struct NoHungarianNotation;

impl Rule for NoHungarianNotation {
    fn code(&self) -> RuleCode {
        RuleCode("GEN003")
    }

    fn name(&self) -> RuleName {
        RuleName("no-hungarian-notation")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Style, Tag::Python, Tag::Rust]
    }
}

impl CodeRule for NoHungarianNotation {
    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<crate::code_lint::SourceDoc>,
        config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        let rule_config: NoHungarianNotationConfig = config.get_rule_config(self.name().0);

        let mut diagnostics = Vec::new();

        let bindings = crate::code_lint::collect_bindings(grep);

        let lang = grep.lang();
        for node in bindings {
            // Check exemptions
            if crate::code_lint::is_import_binding(&node, *lang)
                || crate::code_lint::is_structural_definition(&node, *lang)
            {
                continue;
            }

            let name = node.text();
            let name_lower = name.to_lowercase();

            for suffix in &rule_config.banned_suffixes {
                let suffix_lower = suffix.to_lowercase();
                if name_lower.ends_with(&suffix_lower) {
                    let base_name = &name[..name.len() - suffix.len()];
                    let actual_suffix = &name[name.len() - suffix.len()..];

                    // Case preservation for suggestions (Screaming Snake Case)
                    let is_uppercase = name.chars().all(|c| !c.is_alphabetic() || c.is_uppercase());

                    let suggestion = match suffix_lower.as_str() {
                        "_list" | "_arr" | "_vec" | "_set" => {
                            let plural_suffix =
                                if base_name.ends_with('s') || base_name.ends_with('S') {
                                    ""
                                } else if is_uppercase {
                                    "S"
                                } else {
                                    "s"
                                };
                            format!("Rename the identifier to use plural form (e.g. `{base_name}{plural_suffix}`) or remove the suffix.")
                        }
                        _ => {
                            format!(
                                "Rename the identifier without the type suffix `{actual_suffix}`."
                            )
                        }
                    };

                    diagnostics.push(Diagnostic::new(
                        self.code(),
                        self.name(),
                        ViolationMessage {
                            summary: format!("Identifier `{name}` contains a banned type suffix `{actual_suffix}`."),
                            rationale: "Naming variables with their type suffixes (Hungarian notation) makes refactoring harder and clutters the code.".to_string(),
                            suggestion,
                        },
                        SourceLocation {
                            context: LocationContext::File(path.to_path_buf()),
                            span: SourceSpan {
                                start: node.range().start,
                                end: node.range().end,
                            },
                        },
                    ));
                    // Check only one suffix per node
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
    use crate::code_lint::SupportLang;
    use crate::test_utils::{assert_code_rule_snapshot, assert_code_rule_snapshot_with_config};

    #[test]
    fn test_rust_snapshots() {
        let rule = NoHungarianNotation;

        let source = r#"
            use std::collections::VecDeque; // OK (import)
            use std::collections::HashMap as my_map; // OK (import alias)
            struct UserList; // OK (struct definition)
            fn process_arr() { // OK (function name)
                let user_list = vec!["alice"];
                let id_set = std::collections::HashSet::new();
                let name_str = "bob";
                const MY_INT: i32 = 42;
                let age = 30; // OK
            }
        "#;
        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, source, "test.rs"), @r###"
        [GEN003] Line 6, Col 21: Identifier `user_list` contains a banned type suffix `_list`.
        [GEN003] Line 7, Col 21: Identifier `id_set` contains a banned type suffix `_set`.
        [GEN003] Line 8, Col 21: Identifier `name_str` contains a banned type suffix `_str`.
        [GEN003] Line 9, Col 23: Identifier `MY_INT` contains a banned type suffix `_INT`.
        "###);
    }

    #[test]
    fn test_python_snapshots() {
        let rule = NoHungarianNotation;

        let source = r"
import os_path # OK (import)
from sys import stderr as err_file # OK (import alias)
class ItemsArr: # OK (class definition)
    def handle_dict(self): # OK (method definition)
        users_dict = {}
        items_arr = []
        value_int = 42
        data = None # OK
        ";
        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, source, "test.py"), @r###"
        [GEN003] Line 6, Col 9: Identifier `users_dict` contains a banned type suffix `_dict`.
        [GEN003] Line 7, Col 9: Identifier `items_arr` contains a banned type suffix `_arr`.
        [GEN003] Line 8, Col 9: Identifier `value_int` contains a banned type suffix `_int`.
        "###);
    }

    #[test]
    fn test_configuration_override() {
        let rule = NoHungarianNotation;

        let config_toml = r#"
            [rules.no-hungarian-notation]
            banned_suffixes = ["_custom"]
        "#;
        let config: crate::core::Config = toml::from_str(config_toml).unwrap();

        let source = "fn main() { let x_list = 1; let y_custom = 2; }";
        insta::assert_snapshot!(assert_code_rule_snapshot_with_config(&rule, source, "test.rs", &config), @r###"
        [GEN003] Line 1, Col 33: Identifier `y_custom` contains a banned type suffix `_custom`.
        "###);
    }

    #[test]
    fn test_suggestions() {
        let rule = NoHungarianNotation;
        let source = r#"
            fn main() {
                let user_list = vec!["alice"];
                let users_list = vec!["bob"];
                const USER_LIST: &[&str] = &["charlie"];
                let val_int = 42;
            }
        "#;
        let grep = AstGrep::new(source, SupportLang::Rust);
        let diags = rule.check_file(Path::new("test.rs"), &grep, &crate::core::Config::default());

        assert_eq!(diags.len(), 4);
        assert_eq!(
            diags[0].message.suggestion,
            "Rename the identifier to use plural form (e.g. `users`) or remove the suffix."
        );
        assert_eq!(
            diags[1].message.suggestion,
            "Rename the identifier to use plural form (e.g. `users`) or remove the suffix."
        );
        assert_eq!(
            diags[2].message.suggestion,
            "Rename the identifier to use plural form (e.g. `USERS`) or remove the suffix."
        );
        assert_eq!(
            diags[3].message.suggestion,
            "Rename the identifier without the type suffix `_int`."
        );
    }
}
