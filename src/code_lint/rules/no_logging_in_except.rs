//! Verifies that `logging.error` is not used inside python except blocks.

use crate::code_lint::CodeRule;
use crate::core::Rule;
use crate::diagnostic::{Diagnostic, RuleName, SourceLocation, ViolationMessage};
use crate::rules::Tag;
use ast_grep_core::{AstGrep, Doc, Node};
use ast_grep_language::SupportLang;
use std::path::Path;

/// Helper to check if a node is nested inside an `except_clause`.
fn has_except_ancestor<D: Doc>(node: &Node<'_, D>) -> bool {
    let mut parent = node.parent();
    while let Some(ancestor) = parent {
        if ancestor.kind() == "except_clause" {
            return true;
        }
        parent = ancestor.parent();
    }
    false
}

/// Rule struct.
pub struct NoLoggingInExcept;

impl Rule for NoLoggingInExcept {
    fn name(&self) -> RuleName {
        RuleName("no-logging-in-except")
    }
    fn tags(&self) -> &'static [Tag] {
        &[Tag::Logging, Tag::Exceptions]
    }
    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }
}

impl CodeRule for NoLoggingInExcept {
    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<crate::code_lint::SourceDoc>,
        _config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let root = grep.root();
        let matches = root.find_all("logging.error($$$ARGS)");
        for matched_node in matches {
            if has_except_ancestor(&matched_node) {
                diagnostics.push(Diagnostic::new(
                    self.name(),
                    ViolationMessage {
                        summary: "Banned use of `logging.error` inside except block.".to_string(),
                        rationale: "Logging errors inside except blocks using logging.error does not capture exception context automatically, which can hide root causes.".to_string(),
                        suggestion: "Use `logging.exception` instead of `logging.error` inside except blocks.".to_string(),
                    },
                    SourceLocation::from_node(path, &matched_node),
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
        insta::assert_snapshot!(output_violating, @"[no-logging-in-except] Line 4, Col 5: Banned use of `logging.error` inside except block.");

        let output_ok =
            crate::test_utils::assert_code_rule_snapshot(&NoLoggingInExcept, source_ok, "test.py");
        assert!(output_ok.is_empty());
    }
}
