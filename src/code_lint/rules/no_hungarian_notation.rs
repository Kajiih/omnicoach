//! Bans type suffixes (Hungarian notation) in variable names.

use crate::code_lint::CodeRule;
use crate::core::{DenyListConfig, DynamicRuleConfig, FilterListDefaults, Rule};
use crate::diagnostic::{
    Diagnostic, LocationContext, RuleName, SourceLocation, SourceSpan, ViolationMessage,
};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Configuration for the `NoHungarianNotation` rule.
pub type NoHungarianNotationConfig = DynamicRuleConfig<DenyListConfig>;

/// Static defaults for banned type suffixes.
const DEFAULT_BANNED_SUFFIXES: FilterListDefaults = FilterListDefaults {
    base: &[
        "_list", "_arr", "_dict", "_map", "_vec", "_str", "_int", "_bool", "_set", "_ptr", "_num",
        "_float", "_byte",
    ],
    extend: &[],
    exempt: &[],
};

/// Rule that bans Hungarian notation type suffixes.
pub struct NoHungarianNotation;

impl Rule for NoHungarianNotation {
    fn name(&self) -> RuleName {
        RuleName("no-hungarian-notation")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Style, Tag::Naming]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
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
        let effective_banned =
            rule_config.effective_banned_for_lang(*grep.lang(), &DEFAULT_BANNED_SUFFIXES);

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

            for suffix in &effective_banned {
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
        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, source, "test.rs"), @"
        [no-hungarian-notation] Line 6, Col 21: Identifier `user_list` contains a banned type suffix `_list`.
        [no-hungarian-notation] Line 7, Col 21: Identifier `id_set` contains a banned type suffix `_set`.
        [no-hungarian-notation] Line 8, Col 21: Identifier `name_str` contains a banned type suffix `_str`.
        [no-hungarian-notation] Line 9, Col 23: Identifier `MY_INT` contains a banned type suffix `_INT`.
        ");
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
        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, source, "test.py"), @"
        [no-hungarian-notation] Line 6, Col 9: Identifier `users_dict` contains a banned type suffix `_dict`.
        [no-hungarian-notation] Line 7, Col 9: Identifier `items_arr` contains a banned type suffix `_arr`.
        [no-hungarian-notation] Line 8, Col 9: Identifier `value_int` contains a banned type suffix `_int`.
        ");
    }

    #[test]
    fn test_configuration_override() {
        let rule = NoHungarianNotation;

        let config_toml = r#"
            [rules.no-hungarian-notation]
            banned = ["_custom"]
        "#;
        let config: crate::core::Config = toml::from_str(config_toml).unwrap();

        let source = "fn main() { let x_list = 1; let y_custom = 2; }";
        insta::assert_snapshot!(assert_code_rule_snapshot_with_config(&rule, source, "test.rs", &config), @"[no-hungarian-notation] Line 1, Col 33: Identifier `y_custom` contains a banned type suffix `_custom`.");
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

        let suggestions: Vec<&str> =
            diags.iter().map(|diagnostic| diagnostic.message.suggestion.as_str()).collect();
        assert_eq!(
            suggestions,
            vec![
                "Rename the identifier to use plural form (e.g. `users`) or remove the suffix.",
                "Rename the identifier to use plural form (e.g. `users`) or remove the suffix.",
                "Rename the identifier to use plural form (e.g. `USERS`) or remove the suffix.",
                "Rename the identifier without the type suffix `_int`.",
            ]
        );
    }

    #[test]
    fn test_global_allowed_and_extend_banned() {
        let rule = NoHungarianNotation;

        let config_toml = r#"
            [rules.no-hungarian-notation]
            allowed = ["_str"]
            extend_banned = ["_handle"]
        "#;
        let config: crate::core::Config = toml::from_str(config_toml).unwrap();

        // _str is allowed (no violation), _handle is banned (violates), _list is default banned (violates)
        let source = "fn main() { let name_str = 1; let conn_handle = 2; let item_list = 3; }";
        let output = assert_code_rule_snapshot_with_config(&rule, source, "test.rs", &config);
        assert!(!output.contains("_str"));
        assert!(output.contains("contains a banned type suffix `_handle`"));
        assert!(output.contains("contains a banned type suffix `_list`"));
    }

    #[test]
    fn test_language_specific_overrides() {
        let rule = NoHungarianNotation;

        let config_toml = r#"
            [rules.no-hungarian-notation.rust]
            allowed = ["_vec"]

            [rules.no-hungarian-notation.python]
            extend_banned = ["_tbl"]
        "#;
        let config: crate::core::Config = toml::from_str(config_toml).unwrap();

        // In Rust, _vec is allowed:
        let rust_source = "fn main() { let users_vec = 1; }";
        let rust_output =
            assert_code_rule_snapshot_with_config(&rule, rust_source, "test.rs", &config);
        assert!(!rust_output.contains("_vec"));

        // In Python, _vec is still banned by default, AND _tbl is banned:
        let py_source = "users_vec = []\nusers_tbl = []\n";
        let py_output = assert_code_rule_snapshot_with_config(&rule, py_source, "test.py", &config);
        assert!(py_output.contains("contains a banned type suffix `_vec`"));
        assert!(py_output.contains("contains a banned type suffix `_tbl`"));
    }
}
