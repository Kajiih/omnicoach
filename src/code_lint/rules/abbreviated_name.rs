//! Rule targeting banned abbreviations in definitions across multiple languages.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::collections::HashSet;
use std::path::Path;

const BANNED: ListOption = ListOption {
    kind: ListKind::Deny,
    doc: "Abbreviations flagged as a word of an identifier.",
    default: FilterListDefaults {
        base: &[
            "err", "ctx", "cfg", "res", "msg", "str", "num", "btn", "cb", "ch", "diag", "ty",
            "cat", "stmt", "ext", "fmt", "arch", "vis",
        ],
        extend: &[],
        // In Rust, `str` is a primitive type keyword rather than an abbreviation, and it is
        // load-bearing in conventional conversion names (`as_str`, `to_str`, `from_str`).
        // Hungarian `_str` type suffixes remain covered by type-suffixed-name.
        remove: &[(SupportLang::Rust, &["str"])],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Name `{name}` contains the abbreviation `{token}`.",
    rationale: "An ambiguous shorthand forces the reader to guess the word it stands for and fragments searches across the codebase.",
    suggestion: "Rename `{name}` to spell out `{token}` in full.",
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

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("abbreviated-name"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python, SupportLang::Rust],
        options: RuleOptions::code_rule(BANNED),
        classification: Classification {
            topics: &[Topic::ABBREVIATED_NAMES],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags names that contain an abbreviation such as `ctx` or `msg`.",
            what_it_does: "Splits each name the code defines into words, at underscores and \
                           at lowercase-to-uppercase or digit-to-uppercase boundaries, and \
                           flags the name if any word is a banned abbreviation, ignoring case. \
                           Only whole words match: `strategy` and \
                           `category` are not flagged, `handle_msg` and `TaskRes` are. \
                           Checked names: variables, parameters, loop and pattern bindings, \
                           functions, classes, structs, enums, traits, type aliases and \
                           constants. Not checked: imports (including aliased imports such as \
                           `import os as os_cfg`), attributes (`self.ctx = ...`), struct \
                           fields, and names imposed by a contract (Python methods marked \
                           `@override`, members of a Rust `impl Trait for Type` block), \
                           although the parameters of those methods are still checked.",
            why_is_this_bad: "An abbreviation makes the reader guess: `res` can be a result, a \
                              response or a resource, `ch` a channel or a character. Different \
                              authors also shorten the same word differently (`cfg`, `conf`, \
                              `config`), so a search for one spelling misses the others.\n\n\
                              Spell the word out (`context`, `message`, `result`, `config`).",
            references: &[Reference {
                title: "Google Python Style Guide: Naming",
                url: "https://google.github.io/styleguide/pyguide.html#316-naming",
            }],
            examples: &[
                Example {
                    language: SupportLang::Python,
                    flagged: indoc::indoc! {r"
                        def send_alert(notifier, msg):
                            notifier.publish(msg)
                    "},
                    flagged_span: "msg",
                    fixed: indoc::indoc! {r"
                        def send_alert(notifier, message):
                            notifier.publish(message)
                    "},
                },
                Example {
                    language: SupportLang::Rust,
                    flagged: indoc::indoc! {r"
                        fn send_alert(notifier: &Notifier, msg: &str) {
                            notifier.publish(msg);
                        }
                    "},
                    flagged_span: "msg",
                    fixed: indoc::indoc! {r"
                        fn send_alert(notifier: &Notifier, message: &str) {
                            notifier.publish(message);
                        }
                    "},
                },
            ],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

fn check_file(
    rule: &CodeRule<ListOption>,
    path: &Path,
    file: &ParsedFile,
    banned: &HashSet<String>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for node in crate::code_lint::semantic::bindings::collect_renameable_bindings(file) {
        let name = node.text();
        for segment in split_segments(&name) {
            if banned.contains(&segment) {
                diagnostics.push(rule.diagnostic_at_node(
                    path,
                    &node,
                    &[("name", &name), ("token", &segment)],
                ));
                // Flag each node at most once
                break;
            }
        }
    }

    diagnostics
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
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
                aliased_import_exempt => r#"
                    import os as os_cfg
                "#,
                override_method_contract_exempt => r#"
                    from typing import override

                    class CustomDecoder(BaseDecoder):
                        @override
                        def from_ctx(self, data: bytes) -> None:
                            pass
                "#,
                attribute_and_subscript_targets_exempt => r#"
                    class Worker:
                        def update(self, items, index) -> None:
                            self.ctx = 1
                            items[index] = 2
                "#,
            ],
            fail: [
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
                typed_varargs_parameter_name => r#"
                    def process(*req_ctx: int):
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
                aliased_import_exempt => r#"
                    use std::collections::HashMap as my_cfg;
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
