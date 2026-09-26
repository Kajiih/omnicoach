//! Verifies that python functions are flat (no nested defs).

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::rule::CodeRule;
use crate::core::{Rule, Tag};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
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
    fn target(&self) -> crate::code_lint::rule::RuleTarget {
        crate::code_lint::rule::RuleTarget::SourceOnly
    }

    fn check_file(
        &self,
        path: &Path,
        file: &ParsedFile,
        _config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        ast::python::find_nested_functions(file)
            .into_iter()
            .map(|(func, func_name)| {
                self.diagnostic_at_node(path, &func, &[("func_name", &func_name)])
            })
            .collect()
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    FlatScopeEnforced,
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
