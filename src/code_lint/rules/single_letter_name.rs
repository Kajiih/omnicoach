//! Flags names made of a single letter.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::collections::HashSet;
use std::path::Path;

const ALLOWED: ListOption = ListOption {
    kind: ListKind::Allow,
    doc: "Single-letter names accepted as variable names.",
    default: FilterListDefaults {
        base: &["i", "j", "x", "f"],
        extend: &[(SupportLang::Rust, &["c"])],
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
        languages: &[SupportLang::Python, SupportLang::Rust],
        options: RuleOptions::code_rule(ALLOWED),
        classification: Classification {
            topics: &[Topic::ABBREVIATED_NAMES],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags names made of a single letter.",
            what_it_does: "Flags single-letter names the code defines: variables, \
                           parameters (including lambda and closure parameters), loop, \
                           comprehension, `except ... as`, walrus and pattern bindings, and \
                           function, class and constant names, except the allowed ones; `_` \
                           is never flagged. Imports (including aliased imports), members of a \
                           Rust `impl Trait for Type` block, Python methods marked \
                           `@override`, type parameters such as `T`, and references to \
                           existing names are not checked.",
            why_is_this_bad: "A single letter says nothing about what the value is, so the \
                              reader has to trace where it comes from, and the meaning gets \
                              lost as the scope grows. Single letters are also impossible to \
                              search for: a search for `d` matches almost every line.\n\n\
                              Use a noun that says what the value is (`index`, `user`, \
                              `error`). Keep the allowed letters for conventional cases such \
                              as loop counters or coordinates.",
            references: &[Reference {
                title: "Google Python Style Guide: Naming",
                url: "https://google.github.io/styleguide/pyguide.html#316-naming",
            }],
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
        if name.chars().count() == 1 && name != "_" && !allowed.contains(&*name) {
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
            ],
        },
    }
);
