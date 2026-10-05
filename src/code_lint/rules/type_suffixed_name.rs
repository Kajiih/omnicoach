//! Bans type suffixes (Hungarian notation) in variable names.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::code_lint::semantic::bindings;
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
    doc: "Type suffixes flagged at the end of an identifier.",
    default: FilterListDefaults {
        base: &[
            "_list", "_arr", "_dict", "_map", "_vec", "_str", "_int", "_bool", "_set", "_ptr",
            "_num", "_float", "_byte",
        ],
        extend: &[],
        remove: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Name `{name}` ends with the type suffix `{suffix}`.",
    rationale: "A type encoded in a name duplicates the annotation and turns misleading when the type changes.",
    suggestion: "Rename `{name}` to a domain noun such as `{stem}`.",
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("type-suffixed-name"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python, SupportLang::Rust],
        options: RuleOptions::code_rule(BANNED),
        classification: Classification {
            topics: &[Topic::TYPE_ENCODED_NAMES],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags names that end with a type suffix such as `_list` or `_str`.",
            what_it_does: indoc::indoc! {r"
                Flags variables, parameters, loop and pattern bindings, and constants whose name
                ends, ignoring case, with a type suffix: `_list`, `_arr`, `_dict`, `_map`, `_vec`,
                `_str`, `_int`, `_bool`, `_set`, `_ptr`, `_num`, `_float` or `_byte` by default
                (`users_dict`, `MY_INT`). A name that is only the suffix, such as `_list`, is not
                flagged, nor is a boolean predicate name starting with `is_` or `has_` (`is_dict`),
                whose type word names what is tested. Functions, classes, structs, enums, traits,
                type aliases, imports (aliased or not) and members of a Rust `impl Trait for Type`
                block are not checked; neither are attributes (`self.users_dict = ...`) or struct
                fields."},
            why_is_this_bad: indoc::indoc! {r"
                The suffix repeats what the type annotation or the compiler already knows, and it
                lies as soon as the type changes: a `user_list` that becomes a set or a generator
                keeps its old name unless every use is renamed. It also takes the place of what the
                name should say: what the value means.

                Name the value by its role, using a plural for collections (`users`, `name`,
                `scores_by_player`), and leave the type to the annotation."},
            references: &[Reference {
                title: "Making Wrong Code Look Wrong (Joel Spolsky)",
                url: "https://www.joelonsoftware.com/2005/05/11/making-wrong-code-look-wrong/",
            }],
            examples: &[
                Example {
                    language: SupportLang::Python,
                    flagged: indoc::indoc! {r"
                        def rank_players(matches):
                            scores_dict = tally_scores(matches)
                            return sorted(scores_dict, key=scores_dict.get, reverse=True)
                    "},
                    flagged_span: "scores_dict",
                    fixed: indoc::indoc! {r"
                        def rank_players(matches):
                            scores = tally_scores(matches)
                            return sorted(scores, key=scores.get, reverse=True)
                    "},
                },
                Example {
                    language: SupportLang::Rust,
                    flagged: indoc::indoc! {r"
                        fn rank_players(matches: &[Match]) -> Vec<PlayerId> {
                            let scores_map = tally_scores(matches);
                            rank_by_score(&scores_map)
                        }
                    "},
                    flagged_span: "scores_map",
                    fixed: indoc::indoc! {r"
                        fn rank_players(matches: &[Match]) -> Vec<PlayerId> {
                            let scores = tally_scores(matches);
                            rank_by_score(&scores)
                        }
                    "},
                },
            ],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

/// Prefixes of boolean predicate names (`is_dict`, `has_str`), whose trailing type word names
/// what is tested rather than the variable's own type.
const PREDICATE_PREFIXES: [&str; 2] = ["is_", "has_"];

fn check_file(
    rule: &CodeRule<ListOption>,
    path: &Path,
    file: &ParsedFile,
    banned: &HashSet<String>,
) -> Vec<Diagnostic> {
    bindings::find_suffixed_bindings(file, banned)
        .into_iter()
        .filter(|matched| {
            let name = matched.name.to_ascii_lowercase();
            !PREDICATE_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix))
        })
        .map(|matched| {
            rule.diagnostic_at_node(
                path,
                &matched.node,
                &[
                    ("name", &matched.name),
                    ("suffix", &matched.actual_suffix),
                    ("stem", &matched.base_name),
                ],
            )
        })
        .collect()
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
                predicate_name_exempt => r#"
                    def summarize(base):
                        is_typed_dict = base.name == "TypedDict"
                        has_str = any(isinstance(arg, str) for arg in base.args)
                        return is_typed_dict, has_str
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
                predicate_name_exempt => r#"
                    fn run(node: &Node) {
                        let is_vec = node.kind() == "vec";
                        let has_map = node.children().any(|child| child.kind() == "map");
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
