//! Verifies that python functions are flat (no nested defs).

use crate::code_lint::CodeRule;
use crate::core::{Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Function `{func_name}` is defined inside another function.",
    rationale: "Nested named functions bloat enclosing scopes and capture ambient state implicitly, increasing cognitive complexity and preventing isolated unit testing.",
    suggestion: "Extract `{func_name}` to a module-level private function (`_{func_name}`) or use an inline `lambda` for trivial callbacks.",
};

/// Rule enforcing flat function definitions (no nested named functions).
pub struct FlatScopeEnforced;

impl Rule for FlatScopeEnforced {
    fn name(&self) -> RuleName {
        RuleName("flat-scope-enforced")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Complexity, Tag::Style]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeRule for FlatScopeEnforced {
    fn target(&self) -> crate::code_lint::RuleTarget {
        crate::code_lint::RuleTarget::SourceOnly
    }

    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<crate::code_lint::SourceDoc>,
        _config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        grep.root()
            .find_all("def $NAME($$$ARGS): $$$BODY")
            .filter(|func| crate::code_lint::ast_python::is_nested_function(func))
            .map(|func| {
                let func_name = func
                    .field("name")
                    .map(|name_node| name_node.text())
                    .unwrap_or_default();
                self.diagnostic_at_node(path, &func, &[("func_name", &func_name)])
            })
            .collect()
    }
}

#[cfg(test)]
crate::rule_test!(
    FlatScopeEnforced,
    {
        Python => {
            pass: [
                top_level_functions_allowed => r#"
                    def first():
                        pass

                    def second():
                        pass
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
                module_level_private_helper_allowed => r#"
                    def _compute_checksum(payload):
                        return len(payload)

                    def process(payload):
                        return _compute_checksum(payload)
                "#,
            ],
            fail: [
                nested_function_in_function => r#"
                    def outer():
                        def inner():
                            return 1
                        return inner()
                "# => [r#"
                    def inner():
                        return 1
                "#],
                nested_function_in_method => r#"
                    class Processor:
                        def run(self, data):
                            def transform(item):
                                return item * 2
                            return [transform(val) for val in data]
                "# => [r#"
                    def transform(item):
                        return item * 2
                "#],
                multiple_nested_functions => r#"
                    def pipeline(value):
                        def step_one(input_val):
                            return input_val + 1

                        def step_two(input_val):
                            return input_val * 2

                        return step_two(step_one(value))
                "# => [
                    r#"
                        def step_one(input_val):
                            return input_val + 1
                    "#,
                    r#"
                        def step_two(input_val):
                            return input_val * 2
                    "#,
                ],
            ],
        },
    }
);
