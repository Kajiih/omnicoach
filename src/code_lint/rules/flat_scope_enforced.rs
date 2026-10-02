//! Verifies that python functions are flat (no nested defs).

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::rule::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, ImpactedQuality, Precision, RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Function `{func_name}` is defined inside another function.",
    rationale: "Nested named functions bloat enclosing scopes and capture ambient state implicitly, increasing cognitive complexity and preventing isolated unit testing.",
    suggestion: "Extract `{func_name}` to a module-level private function (`_{func_name}`) or use an inline `lambda` for trivial callbacks.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("flat-scope-enforced"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::COMPLEXITY],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags Python functions defined inside other functions.",
            what_it_does: "Flags every `def` that appears inside the body of another \
                           function or method in Python source files; test files are not \
                           checked. This includes the methods of a class declared inside a \
                           function. Top-level functions, methods of top-level classes and \
                           `lambda` expressions are not flagged.",
            why_is_this_bad: "A nested function captures the enclosing function's local \
                              variables implicitly, so its real inputs are not visible in its \
                              signature. It cannot be imported, tested or reused on its own, \
                              and it makes the enclosing function longer and harder to \
                              follow.\n\n\
                              Move the function to module level, conventionally with a \
                              leading underscore, and pass what it needs as parameters. A \
                              `lambda` remains fine for a trivial callback such as a sort \
                              key.",
            references: &[],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    ast::python::find_nested_functions(file)
        .into_iter()
        .map(|(func, func_name)| rule.diagnostic_at_node(path, &func, &[("func_name", &func_name)]))
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                top_level_functions_allowed => r#"
                    def _compute_checksum(payload):
                        return len(payload)

                    def process(payload):
                        return _compute_checksum(payload)
                "#,
                class_methods_allowed => r#"
                    class Greeter:
                        def greet(self):
                            return "hello"
                "#,
                lambda_callbacks_allowed => r#"
                    def sort_items(items):
                        return sorted(items, key=lambda item: item.id)
                "#,
            ],
            fail: [
                nested_in_function => r#"
                    def outer():
                        def inner():
                            return 1
                        return inner()
                "# => r#"
                    def inner():
                        return 1
                "#,
                nested_with_return_annotation => r#"
                    def outer() -> int:
                        def inner() -> int:
                            return 1
                        return inner()
                "# => r#"
                    def inner() -> int:
                        return 1
                "#,
                nested_in_class_method => r#"
                    class Processor:
                        def run(self, data):
                            def transform(item):
                                return item * 2
                            return [transform(val) for val in data]
                "# => r#"
                    def transform(item):
                        return item * 2
                "#,
            ],
        },
    }
);
