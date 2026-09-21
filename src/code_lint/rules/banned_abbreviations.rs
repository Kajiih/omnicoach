//! Rule targeting banned abbreviations in definitions across multiple languages.

use crate::code_lint::CodeRule;
use crate::core::{FilterListDefaults, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Static defaults for banned abbreviations.
const DEFAULT_BANNED: FilterListDefaults = FilterListDefaults {
    base: &[
        "err", "ctx", "cfg", "res", "msg", "str", "num", "btn", "cb", "ch", "diag", "ty",
    ],
    extend: &[],
    // In Rust, `str` is a primitive type keyword rather than an abbreviation, and it is
    // load-bearing in conventional conversion names (`as_str`, `to_str`, `from_str`).
    // Hungarian `_str` type suffixes remain covered by no-hungarian-notation.
    exempt: &[(SupportLang::Rust, &["str"])],
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Definition name `{name}` contains banned abbreviation `{segment}`.",
    rationale: "Banned abbreviations make identifier names less clear, harder to read, and difficult to search for.",
    suggestion: "Rename the identifier using full words or a non-banned term.",
};

/// Helper to split identifiers into sub-word segments.
fn split_segments(name: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut previous_char: Option<char> = None;

    for character in name.chars() {
        if character == '_' {
            if !current.is_empty() {
                segments.push(current.to_lowercase());
                current.clear();
            }
            previous_char = None;
            continue;
        }

        // Split at lowercase/digit -> uppercase transition
        if let Some(prev) = previous_char
            && character.is_uppercase()
            && (prev.is_lowercase() || prev.is_numeric())
            && !current.is_empty()
        {
            segments.push(current.to_lowercase());
            current.clear();
        }

        current.push(character);
        previous_char = Some(character);
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

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeRule for BannedAbbreviations {
    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<crate::code_lint::SourceDoc>,
        config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        let effective_banned = self.effective_banned_set(*grep.lang(), config, &DEFAULT_BANNED);
        let mut diagnostics = Vec::new();

        for node in crate::code_lint::collect_renameable_bindings(grep) {
            let name = node.text();
            for segment in split_segments(&name) {
                if effective_banned.contains(&segment) {
                    diagnostics.push(self.diagnostic_at_node(
                        path,
                        &node,
                        &[("name", &name), ("segment", &segment)],
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
        let config_toml = r#"
            [rules.banned-abbreviations]
            allowed = ["err"]
            extend_banned = ["req"]
        "#;
        let config: crate::core::Config = toml::from_str(config_toml).unwrap();

        let source = "fn main() { let err = 1; let my_req = 2; }";
        let output = assert_code_rule_snapshot_with_config(&rule, source, "test.rs", &config);
        assert!(!output.contains("err"));
        assert!(output.contains("contains banned abbreviation `req`"));
    }
}
