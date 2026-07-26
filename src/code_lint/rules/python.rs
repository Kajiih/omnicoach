//! Python-specific static analysis rules.

use crate::core::Rule;
use crate::diagnostic::{
    Diagnostic, LocationContext, SourceLocation, SourceSpan, ViolationMessage,
};
use ast_grep_core::{AstGrep, Doc, Node};
use ast_grep_language::SupportLang;
use std::path::Path;

/// Helper to check if a node is nested inside an `except_clause`.
fn has_except_ancestor<D: Doc>(node: &Node<'_, D>) -> bool {
    let mut parent = node.parent();
    while let Some(p) = parent {
        if p.kind() == "except_clause" {
            return true;
        }
        parent = p.parent();
    }
    false
}

/// Helper to check if a node is nested inside a `function_definition`.
fn has_function_ancestor<D: Doc>(node: &Node<'_, D>) -> bool {
    let mut parent = node.parent();
    while let Some(p) = parent {
        if p.kind() == "function_definition" {
            return true;
        }
        parent = p.parent();
    }
    false
}

use crate::diagnostic::{RuleCode, RuleName};
use crate::rules::Tag;

/// PY001: Verifies that `logging.error` is not used inside python except blocks.
pub struct NoLoggingInExcept;

impl Rule for NoLoggingInExcept {
    fn code(&self) -> RuleCode {
        RuleCode("PY001")
    }
    fn name(&self) -> RuleName {
        RuleName("no-logging-in-except")
    }
    fn tags(&self) -> &'static [Tag] {
        &[Tag::Logging, Tag::Exceptions, Tag::Python]
    }
}

impl crate::code_lint::CodeRule for NoLoggingInExcept {
    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<ast_grep_core::source::StrDoc<SupportLang>>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let matches = grep.root().find_all("logging.error($$$ARGS)");
        for m in matches {
            if has_except_ancestor(&m) {
                diagnostics.push(Diagnostic::new(
                    self.code(),
                    self.name(),
                    ViolationMessage {
                        summary: "Banned use of `logging.error` inside except block.".to_string(),
                        rationale: "Logging errors inside except blocks using logging.error does not capture exception context automatically, which can hide root causes.".to_string(),
                        suggestion: "Use `logging.exception` instead of `logging.error` inside except blocks.".to_string(),
                    },
                    SourceLocation {
                        context: LocationContext::File(path.to_path_buf()),
                        span: SourceSpan {
                            start: m.range().start,
                            end: m.range().end,
                        },
                    },
                ));
            }
        }
        diagnostics
    }
}

/// PY002: Verifies that python functions are flat (no nested defs).
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
        RuleCode("PY002")
    }
    fn name(&self) -> RuleName {
        RuleName("flat-scope-enforced")
    }
    fn tags(&self) -> &'static [Tag] {
        &[Tag::Style, Tag::Python]
    }
}

impl crate::code_lint::CodeRule for FlatScopeEnforced {
    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<ast_grep_core::source::StrDoc<SupportLang>>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let matches_func = grep.root().find_all("def $NAME($$$ARGS): $$$BODY");
        for m in matches_func {
            if has_function_ancestor(&m) {
                let func_name = m.field("name").map(|n| n.text()).unwrap_or_default();
                let violation = FlatScopeViolation {
                    func_name: func_name.to_string(),
                };
                let message = Self::format_message(&violation);

                diagnostics.push(Diagnostic::new(
                    self.code(),
                    self.name(),
                    message,
                    SourceLocation {
                        context: LocationContext::File(path.to_path_buf()),
                        span: SourceSpan {
                            start: m.range().start,
                            end: m.range().end,
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
    fn test_no_logging_in_except_rule() {
        let source_violating = indoc! {r#"
            try:
                x = 1 / 0
            except Exception as e:
                logging.error("division failed")
        "#};
        let source_ok = indoc! {r#"
            try:
                x = 1 / 0
            except Exception as e:
                logging.exception("division failed")
        "#};

        let output_violating = crate::test_utils::assert_code_rule_snapshot(
            &NoLoggingInExcept,
            source_violating,
            "test.py",
        );
        insta::assert_snapshot!(output_violating, @r###"
        [PY001] Line 4, Col 5: Banned use of `logging.error` inside except block.
        "###);

        let output_ok = crate::test_utils::assert_code_rule_snapshot(
            &NoLoggingInExcept,
            source_ok,
            "test.py",
        );
        assert!(output_ok.is_empty());
    }

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
        [PY002] Line 2, Col 5: Nested function definition `inner` is discouraged.
        "###);

        let output_ok = crate::test_utils::assert_code_rule_snapshot(
            &FlatScopeEnforced,
            source_ok,
            "test.py",
        );
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
        let output = crate::test_utils::assert_code_rule_snapshot(
            &FlatScopeEnforced,
            source,
            "test.py",
        );
        insta::assert_snapshot!(output, @r###"
        [PY002] Line 2, Col 5: Nested function definition `inner1` is discouraged.
        [PY002] Line 3, Col 9: Nested function definition `inner2` is discouraged.
        "###);
    }
}

