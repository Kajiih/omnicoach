//! Flags names made of a single letter.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::collections::HashSet;
use std::path::Path;

const ALLOWED: ListOption = ListOption {
    kind: ListKind::Allow,
    doc: "Single-letter names accepted as variable names.",
    default: FilterListDefaults {
        base: &["i", "j", "x", "f"],
        extend: &[(Language::Rust, &["c"])],
        remove: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Name `{name}` is a single letter.",
    rationale: "A single-letter name says nothing about the role of the value and matches almost everything in a search.",
    suggestion: "Rename `{name}` to a noun that states its role.",
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("single-letter-name"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(ALLOWED),
        classification: Classification {
            topics: &[Topic::ABBREVIATED_NAMES],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags names made of a single letter.",
            what_it_does: indoc::indoc! {r"
                Flags single-letter names the code defines, such as a loop variable `u` or a
                parameter `d`, unless the letter is allowed. `_`, type parameters such as `T`,
                import aliases and names imposed by a contract (Python `@override` methods, members
                of a Rust `impl Trait for Type` block) are skipped, but the parameters of those
                methods are still checked."},
            why_is_this_bad: indoc::indoc! {r"
                A single letter says nothing about what the value is, so the reader has to trace
                where it comes from, and the meaning gets lost as the scope grows. Single letters
                are also impossible to search for: a search for `d` matches almost every line.

                Use a noun that says what the value is (`index`, `user`, `error`). Keep the allowed
                letters for conventional cases such as loop counters or coordinates."},
            known_problems: None,
            references: &[Reference {
                title: "Google Python Style Guide: Naming",
                url: "https://google.github.io/styleguide/pyguide.html#316-naming",
            }],
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r"
                        def welcome_new_users(new_users):
                            for u in new_users:
                                send_welcome_email(u)
                    "},
                    flagged_span: "u",
                    fixed: indoc::indoc! {r"
                        def welcome_new_users(new_users):
                            for user in new_users:
                                send_welcome_email(user)
                    "},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r"
                        fn welcome_new_users(new_users: &[User]) {
                            for u in new_users {
                                send_welcome_email(u);
                            }
                        }
                    "},
                    flagged_span: "u",
                    fixed: indoc::indoc! {r"
                        fn welcome_new_users(new_users: &[User]) {
                            for user in new_users {
                                send_welcome_email(user);
                            }
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
    allowed: &HashSet<String>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for node in crate::code_lint::semantic::bindings::collect_renameable_bindings(file) {
        let name = node.text();
        if name.chars().count() == 1 && !allowed.contains(&*name) {
            diagnostics.push(rule.diagnostic_at_node(path, &node, &[("name", &name)]));
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
                allowed_names => r#"
                    x = 1
                    i = 0
                    j = 0
                    f = open("file.txt")
                "#,
                descriptive_variable_names => r#"
                    index = 0
                    item = "data"
                    user = "alice"
                    error = None
                "#,
                parameter_allowed => r#"
                    def foo(i: int = 1):
                        pass
                "#,
                wildcard_ignored => r#"
                    _ = 1
                "#,
                aliased_import_exempt => r#"
                    import os as a
                "#,
                attribute_writes_outside_declarations_not_checked => r#"
                    class Worker:
                        def update(self, items, index) -> None:
                            self.b = 1
                            items[index] = 2
                "#,
            ],
            fail: [
                rust_only_allowed_char_flagged_in_python => r#"
                    c = 2
                "# => "c",
                non_ascii_single_letter => r#"
                    é = 1
                "# => "é",
                parameter_annotation => r#"
                    def foo(b: int = 1):
                        pass
                "# => "b",
                typed_varargs_parameter => r#"
                    def total(*a: int) -> int:
                        return sum(a)
                "# => "a",
                multi_assignment => r#"
                    x, d = 3, 4
                "# => "d",
                comprehension => r#"
                    [y for y in range(10)]
                "# => "y",
                exception_alias => r#"
                    try:
                        pass
                    except Exception as g:
                        pass
                "# => "g",
                walrus_expression => r#"
                    (v := 1)
                "# => "v",
                init_attribute_declaration => r#"
                    class Worker:
                        def __init__(self, count) -> None:
                            self.b = count
                            self.b = 2
                "# => "b",
                init_tuple_target_declaration => r#"
                    class Worker:
                        def __init__(self) -> None:
                            self.total, self.d = 1, 2
                "# => "d",
                class_body_attribute_declaration => r#"
                    class Worker:
                        b: int

                        def __init__(self, count: int) -> None:
                            self.b = count
                "# => "b",
            ],
        },
        Rust => {
            pass: [
                allowed_bindings => r#"
                    fn main() {
                        let i = 0;
                        let j = 0;
                        let x = 1;
                        let f = 2;
                    }
                "#,
                rust_char_binding_allowed => r#"
                    fn main() {
                        let c = 'a';
                    }
                "#,
                descriptive_variable_names => r#"
                    fn main() {
                        let index = 0;
                        let item = "data";
                        let user = "alice";
                        let error = None::<()>;
                    }
                "#,
                closure_parameter_allowed => r#"
                    fn main() {
                        let f = |x: i32| x + 1;
                    }
                "#,
                fn_parameter_allowed => r#"
                    fn test(i: i32, j: i32) {}
                "#,
                wildcard_ignored => r#"
                    fn main() {
                        let _ = 1;
                    }
                "#,
                single_letter_reference_not_flagged => r#"
                    fn main() {
                        let total = q + 1;
                    }
                "#,
            ],
            fail: [
                let_binding => r#"
                    fn main() {
                        let a = 1;
                    }
                "# => "a",
                mutable_binding => r#"
                    fn main() {
                        let mut b = 2;
                    }
                "# => "b",
                tuple_destructuring => r#"
                    fn main() {
                        let (c, d) = (1, 2);
                    }
                "# => "d",
                struct_destructuring_explicit => r#"
                    fn main() {
                        let Point { x: e, y: _ } = p;
                    }
                "# => "e",
                struct_destructuring_shorthand => r#"
                    fn main() {
                        let Point { f, g } = p;
                    }
                "# => "g",
                loop_target => r#"
                    fn main() {
                        for h in 0..10 {}
                    }
                "# => "h",
                match_pattern_variants => r#"
                    fn main() {
                        match val {
                            Some(k) => {}
                            None => {}
                        }
                    }
                "# => "k",
                if_let_pattern => r#"
                    fn main() {
                        if let Some(a) = y {}
                    }
                "# => "a",
                while_let_pattern => r#"
                    fn main() {
                        while let Some(z) = y {}
                    }
                "# => "z",
                match_pattern_guard => r#"
                    fn main() {
                        match val {
                            Some(z) if z > 0 => {}
                        }
                    }
                "# => "z",
                struct_field => r#"
                    struct Sample {
                        q: i32,
                        label: String,
                    }
                "# => "q",
            ],
        },
    }
);
