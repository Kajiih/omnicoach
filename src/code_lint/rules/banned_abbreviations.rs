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
                full_domain_words_allowed => r#"
                    def handle_message(context, error_message):
                        manager = "active"
                        configuration = 42
                "#,
                substring_containing_banned_token_allowed => r#"
                    def process(strategy, category):
                        pass
                "#,
                unaliased_import_statement_exempt => r#"
                    import os
                "#,
                unaliased_from_import_exempt => r#"
                    from os import path
                "#,
                override_method_contract_exempt => r#"
                    from typing import override

                    class CustomDecoder(BaseDecoder):
                        @override
                        def from_ctx(self, data: bytes) -> None:
                            pass
                "#,
            ],
            fail: [
                aliased_import_abbreviation => r#"
                    import os as os_cfg
                "# => "os_cfg",
                camel_case_class_name => r#"
                    class TaskRes:
                        pass
                "# => "TaskRes",
                snake_case_function_name => r#"
                    def handle_msg():
                        pass
                "# => "handle_msg",
                parameter_name => r#"
                    def process(req_ctx):
                        pass
                "# => "req_ctx",
                digit_to_uppercase_split => r#"
                    def process(v2Ctx):
                        pass
                "# => "v2Ctx",
                single_diagnostic_when_multiple_tokens_banned => r#"
                    err_msg = "failed"
                "# => "err_msg",
                str_banned_in_python => r#"
                    def to_str():
                        pass
                "# => "to_str",
                unannotated_method_flagged => r#"
                    class CustomDecoder:
                        def from_ctx(self, data: bytes) -> None:
                            pass
                "# => "from_ctx",
            ],
        },
        Rust => {
            pass: [
                full_domain_words_allowed => r#"
                    fn handle_context(error_message: &str) {
                        let configuration = 1;
                    }
                "#,
                substring_containing_banned_token_allowed => r#"
                    fn process(strategy: usize) {}
                "#,
                str_conversion_functions_exempt => r#"
                    fn as_str() {}
                    fn to_str() {}
                    fn from_str() {}
                "#,
                str_prefix_binding_exempt => r#"
                    fn run() {
                        let str_buffer = 1;
                    }
                "#,
                unaliased_imports_exempt => r#"
                    use std::fmt::Result;
                    use std::error::Error;
                "#,
                trait_impl_associated_type_exempt => r#"
                    impl Decoder for Wrapper {
                        type Err = ();
                    }
                "#,
                trait_impl_method_exempt => r#"
                    impl Decoder for Wrapper {
                        fn from_ctx(&self) {}
                    }
                "#,
            ],
            fail: [
                aliased_import_abbreviation => r#"
                    use std::collections::HashMap as my_cfg;
                "# => "my_cfg",
                camel_case_struct_name => r#"
                    struct MyRes;
                "# => "MyRes",
                snake_case_function_name => r#"
                    fn process_err() {}
                "# => "process_err",
                digit_to_uppercase_split => r#"
                    fn main() {
                        let v2Ctx = 2;
                    }
                "# => "v2Ctx",
                single_diagnostic_when_multiple_tokens_banned => r#"
                    fn main() {
                        let err_msg = 1;
                    }
                "# => "err_msg",
                trait_impl_parameter_flagged => r#"
                    impl Decoder for Wrapper {
                        fn from_ctx(&self, msg: u8) {}
                    }
                "# => "msg",
                inherent_method_flagged => r#"
                    impl Wrapper {
                        fn from_ctx(&self) {}
                    }
                "# => "from_ctx",
            ],
        },
    }
);
