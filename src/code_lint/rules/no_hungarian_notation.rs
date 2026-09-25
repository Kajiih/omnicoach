//! Bans type suffixes (Hungarian notation) in variable names.

architecture_component!(CodeLintRules);

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::rule::CodeRule;
use crate::core::{FilterListDefaults, Rule, Tag};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
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
        file: &ParsedFile,
        config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        self.check_banned_suffixes(path, file, config, &DEFAULT_BANNED_SUFFIXES)
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    NoHungarianNotation,
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
