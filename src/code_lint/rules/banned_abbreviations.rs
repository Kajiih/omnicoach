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
        "err", "ctx", "cfg", "res", "msg", "str", "num", "btn", "cb", "ch", "diag", "ty", "cat",
        "stmt", "ext",
    ],
    extend: &[],
    // In Rust, `str` is a primitive type keyword rather than an abbreviation, and it is
    // load-bearing in conventional conversion names (`as_str`, `to_str`, `from_str`).
    // Hungarian `_str` type suffixes remain covered by no-hungarian-notation.
    exempt: &[(SupportLang::Rust, &["str"])],
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Identifier `{name}` contains abbreviated token `{token}`.",
    rationale: "Ambiguous shorthand tokens (`ctx`, `mgr`, `val`, `cfg`) force readers to guess domain vocabulary and fragment codebase searchability.",
    suggestion: "Rename `{name}` using full, self-explanatory domain words (e.g., `context`, `manager`, `value`, `config`).",
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

        for node in crate::code_lint::bindings::collect_renameable_bindings(grep) {
            let name = node.text();
            for segment in split_segments(&name) {
                if effective_banned.contains(&segment) {
                    diagnostics.push(self.diagnostic_at_node(
                        path,
                        &node,
                        &[("name", &name), ("token", &segment), ("segment", &segment)],
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
crate::rule_test!(
    BannedAbbreviations,
    {
        Python => {
            pass: [
                full_domain_words => r#"
                    def handle_message(context):
                        manager = "active"
                        configuration = 42
                        result = "ok"
                "#,
                unaliased_imports_exempt => r#"
                    import os
                    import sys
                    from os import path
                "#,
            ],
            fail: [
                aliased_import_abbreviation => r#"
                    import os as os_cfg
                "# => ["os_cfg"],
                function_and_parameter_abbreviations => r#"
                    def handle_msg(msg):
                        pass
                "# => ["handle_msg", "msg"],
                variable_abbreviation => r#"
                    def run():
                        req_ctx = "request"
                "# => ["req_ctx"],
                string_abbreviation_in_python => r#"
                    def to_str():
                        pass
                "# => ["to_str"],
                class_abbreviation => r#"
                    class TaskRes:
                        pass
                "# => ["TaskRes"],
            ],
        },
        Rust => {
            pass: [
                full_domain_words => r#"
                    fn handle_context(manager: &str) {
                        let configuration = 1;
                        let result = 2;
                    }
                "#,
                str_keyword_and_conversions_exempt => r#"
                    fn as_str() {}
                    fn to_str() {}
                    fn from_str() {}
                    fn build_str_cache() {
                        let str_buffer = 1;
                    }
                "#,
                unaliased_imports_exempt => r#"
                    use std::fmt::Result;
                    use std::error::Error;
                "#,
                trait_impl_contract_members_exempt => r#"
                    impl Decoder for Wrapper {
                        type Err = ();
                        fn from_ctx(&self) {}
                    }
                "#,
            ],
            fail: [
                aliased_import_abbreviation => r#"
                    use std::collections::HashMap as my_cfg;
                "# => ["my_cfg"],
                function_and_local_bindings => r#"
                    fn process_err() {
                        let ctx = 1;
                        let my_cfg_val = 2;
                    }
                "# => ["process_err", "ctx", "my_cfg_val"],
                struct_abbreviation => r#"
                    struct MyRes;
                "# => ["MyRes"],
                trait_impl_parameter_not_exempt => r#"
                    impl Decoder for Wrapper {
                        fn from_ctx(&self, msg: u8) {}
                    }
                "# => ["msg"],
                inherent_method_not_exempt => r#"
                    impl Wrapper {
                        fn from_ctx(&self) {}
                    }
                "# => ["from_ctx"],
            ],
        },
    }
);
