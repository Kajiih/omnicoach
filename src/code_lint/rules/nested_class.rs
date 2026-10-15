//! Verifies that Python classes are not defined inside functions or methods.

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, RuleDoc,
    RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Class `{class}` is defined inside a function.",
    rationale: "A class defined inside a function is recreated on every call, captures the enclosing scope implicitly, and cannot be imported or referenced in module-level type annotations.",
    suggestion: "Move `{class}` to module level and pass any values it needs to `__init__`.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("nested-class"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::NESTING],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags Python classes defined inside functions.",
            what_it_does: indoc::indoc! {r"
                Flags a `class` defined inside the body of a Python function or method. Top-level
                classes and classes nested inside another class body are not flagged."},
            why_is_this_bad: indoc::indoc! {r"
                A class defined inside a function is rebuilt as a new class object on every call, so
                two calls return classes that are not equal and fail `isinstance` checks across calls.
                It also captures the enclosing function's local variables implicitly, cannot be
                imported or tested on its own, and cannot be referenced in module-level type
                annotations.

                Move the class to module level and pass the values it needs to `__init__`."},
            known_problems: None,
            references: &[],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r"
                    def make_counter(step: int):
                        class Counter:
                            def next_value(self, current: int) -> int:
                                return current + step
                        return Counter()
                "},
                flagged_span: indoc::indoc! {r"
                    class Counter:
                        def next_value(self, current: int) -> int:
                            return current + step
                "},
                fixed: indoc::indoc! {r"
                    class _Counter:
                        def __init__(self, step: int) -> None:
                            self._step = step

                        def next_value(self, current: int) -> int:
                            return current + self._step

                    def make_counter(step: int) -> _Counter:
                        return _Counter(step)
                "},
            }],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    ast::python::find_nested_classes(file)
        .into_iter()
        .map(|(class_node, class_name)| {
            rule.diagnostic_at_node(path, &class_node, &[("class", &class_name)])
        })
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                top_level_class_allowed => r#"
                    class Greeter:
                        def greet(self) -> str:
                            return "hello"
                "#,
                class_nested_in_class_allowed => r#"
                    class Outer:
                        class Inner:
                            value: int = 1
                "#,
            ],
            fail: [
                nested_in_function => r#"
                    def make_handler():
                        class Handler:
                            def handle(self, event):
                                return event
                        return Handler()
                "# => r#"
                    class Handler:
                        def handle(self, event):
                            return event
                "#,
                nested_in_method => r#"
                    class Service:
                        def build(self):
                            class Session:
                                active = True
                            return Session()
                "# => r#"
                    class Session:
                        active = True
                "#,
                nested_class_inside_local_class_flagged_once => r#"
                    def make_envelope():
                        class Envelope:
                            class Header:
                                version = 1
                        return Envelope()
                "# => r#"
                    class Envelope:
                        class Header:
                            version = 1
                "#,
            ],
        },
    }
);
