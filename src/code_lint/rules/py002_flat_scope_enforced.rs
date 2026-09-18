//! PY002: Verifies that python functions are flat (no nested defs).

use crate::code_lint::CodeRule;
use crate::core::Rule;
use crate::diagnostic::{
    Diagnostic, LocationContext, RuleCode, RuleName, SourceLocation, SourceSpan, ViolationMessage,
};
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

/// Rule struct.
pub struct FlatScopeEnforced;

/// Violation attributes for `FlatScopeEnforced` rule.
pub struct FlatScopeViolation {
    /// The name of the violating nested function.
    pub func_name: String,
}

impl FlatScopeEnforced {
    /// Formats the human-readable diagnostic message.
    #[must_use]
    pub fn format_message(violation: &FlatScopeViolation) -> ViolationMessage {
        ViolationMessage {
            summary: format!(
                "Nested function definition `{}` is discouraged.",
                violation.func_name
            ),
            rationale: "Nested functions increase cognitive complexity and reduce testability."
                .to_string(),
            suggestion: format!(
                "Move `{}` to the module level or convert to a private helper.",
                violation.func_name
            ),
        }
    }
}

impl Rule for FlatScopeEnforced {
    fn code(&self) -> RuleCode {
        RuleCode("SCOPE-001")
    }
    fn name(&self) -> RuleName {
        RuleName("flat-scope-enforced")
    }
    fn tags(&self) -> &'static [Tag] {
        &[Tag::Complexity, Tag::Style]
    }
    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
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
                let violation = FlatScopeViolation { func_name: func_name.to_string() };
                let message = Self::format_message(&violation);

                diagnostics.push(Diagnostic::new(
                    self.code(),
                    self.name(),
                    message,
                    SourceLocation {
                        context: LocationContext::File(path.to_path_buf()),
                        span: SourceSpan {
                            start: matched_node.range().start,
                            end: matched_node.range().end,
                        },
                    },
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
        insta::assert_snapshot!(output_violating, @r###"
        [SCOPE-001] Line 2, Col 5: Nested function definition `inner` is discouraged.
        "###);

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
        insta::assert_snapshot!(output, @r###"
        [SCOPE-001] Line 2, Col 5: Nested function definition `inner1` is discouraged.
        [SCOPE-001] Line 3, Col 9: Nested function definition `inner2` is discouraged.
        "###);
    }
}
