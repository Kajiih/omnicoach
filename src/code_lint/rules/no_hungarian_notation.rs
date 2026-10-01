//! Bans type suffixes (Hungarian notation) in variable names.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::rule::CodeDetector;
use crate::core::{
    Detector, FilterListDefaults, ListKind, ListOption, OptionSpec, ResolvedOptions, RuleOptions,
};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::Rule;
use crate::rule_documentation::{Reference, RuleDoc};
use crate::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Topic};
use ast_grep_language::SupportLang;
use std::path::Path;

const BANNED_SUFFIXES: ListOption = ListOption {
    kind: ListKind::Deny,
    doc: "Type suffixes flagged at the end of an identifier.",
    default: FilterListDefaults {
        base: &[
            "_list", "_arr", "_dict", "_map", "_vec", "_str", "_int", "_bool", "_set", "_ptr",
            "_num", "_float", "_byte",
        ],
        extend: &[],
        exempt: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Identifier `{name}` ends with type suffix `{actual_suffix}`.",
    rationale: "Encoding container or primitive types in variable names duplicates static type annotations and becomes misleading when the underlying type changes.",
    suggestion: "Rename `{name}` to a semantic or domain-plural noun such as `{base_name}` (e.g., `users`, `name`).",
};

/// Rule that bans Hungarian notation type suffixes.
struct NoHungarianNotation;

/// The rule's declaration.
pub const RULE: Rule<dyn CodeDetector> = Rule {
    detector: &NoHungarianNotation,
    classification: Classification {
        topics: &[Topic::TYPE_ENCODED_NAMES],
        precision: Precision::Heuristic,
        consensus: Consensus::Opinionated,
        impacted_quality: ImpactedQuality::Maintainability,
    },
    doc: RuleDoc {
        summary: "Flags variable names that end with a type suffix such as `_list` or `_str`.",
        what_it_does: "Flags variables, parameters, loop and pattern bindings, and \
                       constants whose name ends, ignoring case, with a type suffix: \
                       `_list`, `_arr`, `_dict`, `_map`, `_vec`, `_str`, `_int`, `_bool`, \
                       `_set`, `_ptr`, `_num`, `_float` or `_byte` by default \
                       (`users_dict`, `MY_INT`). A name that is only the suffix, such as \
                       `_list`, is not flagged. Functions, classes, structs, enums, \
                       traits, type aliases, imports (aliased or not) and members of a \
                       Rust `impl Trait for Type` block are not checked; neither are \
                       attributes (`self.users_dict = ...`) or struct fields.",
        why_is_this_bad: "The suffix repeats what the type annotation or the compiler \
                          already knows, and it lies as soon as the type changes: a \
                          `user_list` that becomes a set or a generator keeps its old \
                          name unless every use is renamed. It also takes the place of \
                          what the name should say: what the value means.\n\n\
                          Name the value by its role, using a plural for collections \
                          (`users`, `name`, `scores_by_player`), and leave the type to the \
                          annotation.",
        references: &[Reference {
            title: "Making Wrong Code Look Wrong (Joel Spolsky)",
            url: "https://www.joelonsoftware.com/2005/05/11/making-wrong-code-look-wrong/",
        }],
    },
    options: RuleOptions {
        options: &[OptionSpec::List(&BANNED_SUFFIXES)],
        ..RuleOptions::CODE_RULE
    },
};

impl Detector for NoHungarianNotation {
    fn name(&self) -> RuleName {
        RuleName("no-hungarian-notation")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeDetector for NoHungarianNotation {
    fn check_file(
        &self,
        path: &Path,
        file: &ParsedFile,
        options: &ResolvedOptions<'_>,
    ) -> Vec<Diagnostic> {
        self.check_banned_suffixes(path, file, &options.list(&BANNED_SUFFIXES))
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                unaliased_import_exempt => r#"
                    import os_path
                "#,
                aliased_import_exempt => r#"
                    from sys import stderr as err_file
                "#,
                class_exempt => r#"
                    class ItemsArr:
                        pass
                "#,
                method_exempt => r#"
                    def handle_dict(self):
                        pass
                "#,
                unsuffixed_variable => r#"
                    users = ["alice"]
                    data = None
                "#,
                exact_suffix_exempt => r#"
                    _list = []
                "#,
            ],
            fail: [
                assignment_binding => r#"
                    users_dict = {}
                "# => "users_dict",
                parameter_binding => r#"
                    def process(user_list: list[str]) -> None:
                        pass
                "# => "user_list",
                loop_target_binding => r#"
                    for item_str in user_list:
                        pass
                "# => "item_str",
            ],
        },
        Rust => {
            pass: [
                unaliased_import_exempt => r#"
                    use std::collections::VecDeque;
                "#,
                aliased_import_exempt => r#"
                    use std::collections::HashMap as my_map;
                "#,
                struct_exempt => r#"
                    struct UserList;
                "#,
                fn_exempt => r#"
                    fn process_arr() {}
                "#,
                unsuffixed_variable => r#"
                    fn run() {
                        let users = vec!["alice"];
                        let age = 30;
                    }
                "#,
                exact_suffix_exempt => r#"
                    fn run() {
                        let _list = vec![1];
                    }
                "#,
                trait_impl_const_exempt => r#"
                    impl ExternalTrait for MyStruct {
                        const DEFAULT_INT: i32 = 42;
                    }
                "#,
            ],
            fail: [
                const_binding => r#"
                    const MY_INT: i32 = 42;
                "# => "MY_INT",
                parameter_binding => r#"
                    fn run(account_map: std::collections::HashMap<String, i32>) {}
                "# => "account_map",
                let_binding => r#"
                    fn run() {
                        let user_list = vec!["alice"];
                    }
                "# => "user_list",
                loop_target_binding => r#"
                    fn run() {
                        for item_str in items {}
                    }
                "# => "item_str",
            ],
        },
    }
);
