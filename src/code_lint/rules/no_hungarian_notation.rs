//! Bans type suffixes (Hungarian notation) in variable names.

use crate::code_lint::CodeRule;
use crate::core::{DenyListConfig, DynamicRuleConfig, FilterListDefaults, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
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

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Identifier `{name}` contains a banned type suffix `{actual_suffix}`.",
    rationale: "Naming variables with their type suffixes (Hungarian notation) makes refactoring harder and clutters the code.",
    suggestion: "Rename `{name}` without the type suffix `{actual_suffix}` (e.g. `{base_name}`, or a plural noun for collections).",
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

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
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

                    diagnostics.push(self.diagnostic_at_node(
                        path,
                        &node,
                        &[
                            ("name", &name),
                            ("actual_suffix", actual_suffix),
                            ("base_name", base_name),
                        ],
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
            allowed = ["_str"]
            extend_banned = ["_handle"]
        "#;
        let config: crate::core::Config = toml::from_str(config_toml).unwrap();

        let source = "fn main() { let name_str = 1; let conn_handle = 2; }";
        let output = assert_code_rule_snapshot_with_config(&rule, source, "test.rs", &config);
        assert!(!output.contains("_str"));
        assert!(output.contains("contains a banned type suffix `_handle`"));
    }
}
