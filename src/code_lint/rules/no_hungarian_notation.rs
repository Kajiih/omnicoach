//! Bans type suffixes (Hungarian notation) in variable names.

use crate::code_lint::CodeRule;
use crate::core::{FilterListDefaults, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

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
    summary: "Identifier `{name}` ends with type suffix `{actual_suffix}`.",
    rationale: "Encoding container or primitive types in variable names duplicates static type annotations and becomes misleading when the underlying type changes.",
    suggestion: "Rename `{name}` to a semantic or domain-plural noun such as `{base_name}` (e.g., `users`, `name`).",
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
        crate::code_lint::bindings::check_banned_suffixes(
            self,
            path,
            grep,
            config,
            &DEFAULT_BANNED_SUFFIXES,
        )
    }
}

#[cfg(test)]
crate::rule_test!(
    NoHungarianNotation,
    {
        Python => {
            pass: [
                import_and_alias_exempt => r#"
                    import os_path
                    from sys import stderr as err_file
                "#,
                class_and_method_exempt => r#"
                    class ItemsArr:
                        def handle_dict(self):
                            pass
                "#,
                unsuffixed_variables => r#"
                    users = ["alice"]
                    data = None
                "#,
            ],
            fail: [
                variable_with_dict_suffix => r#"
                    users_dict = {}
                "# => ["users_dict"],
                variable_with_arr_suffix => r#"
                    items_arr = []
                "# => ["items_arr"],
                variable_with_int_suffix => r#"
                    value_int = 42
                "# => ["value_int"],
                multiple_suffixed_variables => r#"
                    users_dict = {}
                    items_arr = []
                    value_int = 42
                "# => ["users_dict", "items_arr", "value_int"],
            ],
        },
        Rust => {
            pass: [
                import_and_alias_exempt => r#"
                    use std::collections::VecDeque;
                    use std::collections::HashMap as my_map;
                "#,
                struct_and_fn_exempt => r#"
                    struct UserList;
                    fn process_arr() {}
                "#,
                unsuffixed_variables => r#"
                    fn run() {
                        let users = vec!["alice"];
                        let age = 30;
                    }
                "#,
            ],
            fail: [
                let_binding_list => r#"
                    fn run() {
                        let user_list = vec!["alice"];
                    }
                "# => ["user_list"],
                let_binding_set => r#"
                    fn run() {
                        let id_set = std::collections::HashSet::new();
                    }
                "# => ["id_set"],
                let_binding_str => r#"
                    fn run() {
                        let name_str = "bob";
                    }
                "# => ["name_str"],
                const_binding_int => r#"
                    const MY_INT: i32 = 42;
                "# => ["MY_INT"],
                multiple_suffixed_variables => r#"
                    fn run() {
                        let user_list = vec!["alice"];
                        let id_set = std::collections::HashSet::new();
                        let name_str = "bob";
                        const MY_INT: i32 = 42;
                    }
                "# => ["user_list", "id_set", "name_str", "MY_INT"],
            ],
        },
    }
);
