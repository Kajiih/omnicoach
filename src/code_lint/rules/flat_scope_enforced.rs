//! Verifies that python functions are flat (no nested defs).

use crate::code_lint::CodeRule;
use crate::core::{Rule, RuleName};
use crate::diagnostic::{violation_template, Diagnostic, ViolationTemplate};
use crate::rules::Tag;
use ast_grep_core::{AstGrep, Doc, Node};
use ast_grep_language::SupportLang;
use std::path::Path;

/// Helper to check if a node is nested inside a `function_definition`.
fn has_function_ancestor<D: Doc>(node: &Node<'_, D>) -> bool {
    let mut parent = node.parent();
    while let Some(ancestor) = parent {
        if ancestor.kind() == "function_definition" {
            return true;
        }
        parent = ancestor.parent();
    }
    false
}

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
        let mut diagnostics = Vec::new();

        let root = grep.root();
        let matches_func = root.find_all("def $NAME($$$ARGS): $$$BODY");
        for matched_node in matches_func {
            if has_function_ancestor(&matched_node) {
                let func_name = matched_node
                    .field("name")
                    .map(|name_node| name_node.text())
                    .unwrap_or_default();

                diagnostics.push(self.diagnostic_at_node(
                    path,
                    &matched_node,
                    &[("func_name", &func_name)],
                ));
            }
        }
        diagnostics
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
