//! Rule targeting banned abbreviations in definitions across multiple languages.

use crate::code_lint::CodeRule;
use crate::core::{DenyListConfig, DynamicRuleConfig, FilterListDefaults, Rule};
use crate::diagnostic::{
    Diagnostic, LocationContext, RuleName, SourceLocation, SourceSpan, ViolationMessage,
};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Configuration for the `BannedAbbreviations` rule.
pub type BannedAbbreviationsConfig = DynamicRuleConfig<DenyListConfig>;

/// Static defaults for banned abbreviations.
const DEFAULT_BANNED: FilterListDefaults = FilterListDefaults {
    base: &["err", "ctx", "cfg", "res", "msg", "str", "num", "btn", "cb", "ch", "diag"],
    extend: &[],
    // In Rust, `str` is a primitive type keyword rather than an abbreviation, and it is
    // load-bearing in conventional conversion names (`as_str`, `to_str`, `from_str`).
    // Hungarian `_str` type suffixes remain covered by no-hungarian-notation.
    exempt: &[(SupportLang::Rust, &["str"])],
};

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
    fn name(&self) -> RuleName {
        RuleName("banned-abbreviations")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Style, Tag::Naming, Tag::Heuristic]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
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
        let effective_banned = rule_config.effective_banned_for_lang(*grep.lang(), &DEFAULT_BANNED);

        let mut diagnostics = Vec::new();

        let bindings = crate::code_lint::collect_bindings(grep);
        let lang = *grep.lang();

        for node in bindings {
            if crate::code_lint::is_unaliased_import_binding(&node, lang)
                || crate::code_lint::is_trait_impl_member(&node, lang)
            {
                continue;
            }
            let name = node.text();
            let segments = split_segments(&name);
            for segment in segments {
                if effective_banned.contains(&segment) {
                    diagnostics.push(Diagnostic::new(
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
        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, source, "test.rs"), @"
        [banned-abbreviations] Line 2, Col 46: Definition name `my_cfg` contains banned abbreviation `cfg`.
        [banned-abbreviations] Line 3, Col 16: Definition name `process_err` contains banned abbreviation `err`.
        [banned-abbreviations] Line 4, Col 21: Definition name `ctx` contains banned abbreviation `ctx`.
        [banned-abbreviations] Line 5, Col 21: Definition name `my_cfg_val` contains banned abbreviation `cfg`.
        [banned-abbreviations] Line 7, Col 20: Definition name `MyRes` contains banned abbreviation `res`.
        ");
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
        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, source, "test.py"), @"
        [banned-abbreviations] Line 2, Col 14: Definition name `os_cfg` contains banned abbreviation `cfg`.
        [banned-abbreviations] Line 3, Col 5: Definition name `handle_msg` contains banned abbreviation `msg`.
        [banned-abbreviations] Line 3, Col 16: Definition name `msg` contains banned abbreviation `msg`.
        [banned-abbreviations] Line 4, Col 5: Definition name `str_val` contains banned abbreviation `str`.
        ");
    }

    #[test]
    fn test_rust_default_exempts_str_abbreviation() {
        let rule = BannedAbbreviations;

        let rust_source = r"
            fn as_str() {}
            fn to_str() {}
            fn from_str() {}
            fn build_str_cache() { let str_buffer = 1; }
        ";
        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, rust_source, "test.rs"), @"");

        // Python keeps the base ban, since `str` is a plain abbreviation there.
        let py_source = "def to_str():\n    pass\n";
        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, py_source, "test.py"), @"[banned-abbreviations] Line 1, Col 5: Definition name `to_str` contains banned abbreviation `str`.");
    }

    #[test]
    fn test_rust_trait_impl_members_are_exempt() {
        let rule = BannedAbbreviations;

        // `Err` and `from_ctx` are mandated by the trait contract and cannot be renamed, so
        // they are exempt. The exemption is scoped to the member name itself: the parameter
        // `msg` and the identical inherent method are the author's choice, so both are flagged.
        let source = r"
            impl Decoder for Wrapper {
                type Err = ();
                fn from_ctx(&self, msg: u8) {}
            }
            impl Wrapper {
                fn from_ctx(&self) {}
            }
        ";
        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, source, "test.rs"), @"
        [banned-abbreviations] Line 4, Col 36: Definition name `msg` contains banned abbreviation `msg`.
        [banned-abbreviations] Line 7, Col 20: Definition name `from_ctx` contains banned abbreviation `ctx`.
        ");
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
        insta::assert_snapshot!(assert_code_rule_snapshot_with_config(&rule, source, "test.rs", &config), @"[banned-abbreviations] Line 1, Col 30: Definition name `my_foo` contains banned abbreviation `foo`.");
    }

    #[test]
    fn test_global_allowed_and_extend_banned() {
        let rule = BannedAbbreviations;

        let config_toml = r#"
            [rules.banned-abbreviations]
            allowed = ["err"]
            extend_banned = ["req"]
        "#;
        let config: crate::core::Config = toml::from_str(config_toml).unwrap();

        // 'err' is allowed (no violation), 'req' is banned (violates), 'ctx' is still default banned (violates)
        let source = "fn main() { let err = 1; let my_req = 2; let ctx = 3; }";
        let output = assert_code_rule_snapshot_with_config(&rule, source, "test.rs", &config);
        assert!(!output.contains("err"));
        assert!(output.contains("contains banned abbreviation `req`"));
        assert!(output.contains("contains banned abbreviation `ctx`"));
    }

    #[test]
    fn test_language_specific_overrides() {
        let rule = BannedAbbreviations;

        // In Rust allow 'str', in Python ban extra 'lst'
        let config_toml = r#"
            [rules.banned-abbreviations.rust]
            allowed = ["str"]

            [rules.banned-abbreviations.python]
            extend_banned = ["lst"]
        "#;
        let config: crate::core::Config = toml::from_str(config_toml).unwrap();

        // In Rust, 'str' should not be flagged:
        let rust_source = "fn as_str() { let my_str = 1; }";
        let rust_output =
            assert_code_rule_snapshot_with_config(&rule, rust_source, "test.rs", &config);
        assert!(!rust_output.contains("str"));

        // In Python, 'str' should still be flagged by default, AND 'lst' should be flagged:
        let py_source = "def handle(my_str, my_lst):\n    pass\n";
        let py_output = assert_code_rule_snapshot_with_config(&rule, py_source, "test.py", &config);
        assert!(py_output.contains("contains banned abbreviation `str`"));
        assert!(py_output.contains("contains banned abbreviation `lst`"));
    }
}
