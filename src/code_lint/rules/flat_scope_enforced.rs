//! Verifies that python functions are flat (no nested defs).

use crate::code_lint::CodeRule;
use crate::core::{Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Nested function definition `{func_name}` is discouraged.",
    rationale: "Nested functions increase cognitive complexity and reduce testability.",
    suggestion: "Move `{func_name}` to the module level or convert to a private helper.",
};

/// Rule struct.
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
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn test_flat_scope_enforced_rule() {
        let source_violating = indoc! {r"
            def outer():
                def inner():
                    pass
        "};
        let source_ok = indoc! {r"
            def first():
                pass

            def second():
                pass
        "};

        let output_violating = crate::test_utils::assert_code_rule_snapshot(
            &FlatScopeEnforced,
            source_violating,
            "test.py",
        );
        insta::assert_snapshot!(output_violating, @"[flat-scope-enforced] Line 2, Col 5: Nested function definition `inner` is discouraged.");

        let output_ok =
            crate::test_utils::assert_code_rule_snapshot(&FlatScopeEnforced, source_ok, "test.py");
        assert!(output_ok.is_empty());
    }

    #[test]
    fn test_flat_scope_multiple_nested() {
        let source = indoc! {r"
            def outer():
                def inner1():
                    def inner2():
                        pass
        "};
        let output =
            crate::test_utils::assert_code_rule_snapshot(&FlatScopeEnforced, source, "test.py");
        insta::assert_snapshot!(output, @"
        [flat-scope-enforced] Line 2, Col 5: Nested function definition `inner1` is discouraged.
        [flat-scope-enforced] Line 3, Col 9: Nested function definition `inner2` is discouraged.
        ");
    }
}
