//! Flags top-level Python statements placed after the `if __name__ == "__main__":` guard.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::collect_statements_after_main_guard;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Top-level statement appears after the `if __name__ == \"__main__\":` guard.",
    rationale: "Code inside the `__main__` block executes before any declarations below it are bound, so calling a function or referencing a binding defined below the guard raises `NameError` at runtime when the module is run as a script.",
    suggestion: "Move all module-level definitions above the `if __name__ == \"__main__\":` block, or move script-only cleanup inside the guard.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("statement-after-main-guard"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::DECLARATION_ORDER],
            precision: Precision::Exact,
            consensus: Consensus::Unopinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags top-level Python statements placed after the `if __name__ == \"__main__\":` guard.",
            what_it_does: indoc::indoc! {r#"
                Flags a top-level statement or definition placed after the `if __name__ ==
                "__main__":` block of a Python module. A further `__main__` guard below the first
                one is not flagged."#},
            why_is_this_bad: indoc::indoc! {r#"
                Python executes top-level module statements sequentially from top to bottom. When
                a module is executed as a script (`python module.py`), the `if __name__ ==
                "__main__":` block runs immediately upon being reached, before any functions,
                classes, or constants declared below it exist in the module namespace. Because a
                call inside `main()` to a helper defined below the guard is inside a deferred
                function scope, undefined-name linters (such as Ruff `F821`) do not flag it, yet
                running the script raises `NameError` at runtime.

                Keep the `if __name__ == "__main__":` block at the very bottom of the module and
                place all definitions above it."#},
            known_problems: None,
            references: &[Reference {
                title: "Python Documentation: __main__ — Top-level code environment",
                url: "https://docs.python.org/3/library/__main__.html",
            }],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r#"
                    if __name__ == "__main__":
                        print(default_port())

                    def default_port() -> int:
                        return 8080
                "#},
                flagged_span: indoc::indoc! {r"
                    def default_port() -> int:
                        return 8080
                "},
                fixed: indoc::indoc! {r#"
                    def default_port() -> int:
                        return 8080

                    if __name__ == "__main__":
                        print(default_port())
                "#},
            }],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    collect_statements_after_main_guard(file)
        .iter()
        .map(|node| rule.diagnostic_at_node(path, node, &[]))
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                main_guard_at_end_of_module => r#"
                    DEFAULT_PORT = 8080

                    def run_server() -> int:
                        return DEFAULT_PORT

                    if __name__ == "__main__":
                        run_server()
                "#,
                guard_else_branch_exempt => r#"
                    if __name__ == "__main__":
                        print("main")
                    else:
                        FALLBACK_PORT = 1
                "#,
                subsequent_main_guard_exempt => r#"
                    if __name__ == "__main__":
                        print("first")

                    if __name__ == "__main__":
                        print("second")
                "#,
                non_main_name_comparisons_exempt => r#"
                    if __name__ != "__main__":
                        IMPORTED = True

                    if __name__ == "custom_pkg":
                        CUSTOM = True

                    AFTER_CHECK = 1
                "#,
                nested_main_guard_inside_function_exempt => r#"
                    def check_entry() -> bool:
                        if __name__ == "__main__":
                            return True
                        return False

                    AFTER_HELPER = 1
                "#,
            ],
            fail: [
                function_after_main_guard => r#"
                    if __name__ == "__main__":
                        print(helper())

                    def helper() -> int:
                        return 1
                "# => "def helper() -> int:\n    return 1",
                reversed_main_guard_comparison => r#"
                    if "__main__" == __name__:
                        pass

                    EXTRA_FLAG = True
                "# => "EXTRA_FLAG = True",
            ],
        },
    }
);
