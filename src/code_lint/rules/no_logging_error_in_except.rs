//! Verifies that `logging.error` is not used inside python except blocks.

use crate::code_lint::CodeRule;
use crate::core::{Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Banned use of `logging.error` inside except block.",
    rationale: "Logging errors inside except blocks using logging.error does not capture exception context automatically, which can hide root causes.",
    suggestion: "Use `logging.exception` instead of `logging.error` inside except blocks.",
};

/// Rule struct.
pub struct NoLoggingErrorInExcept;

impl Rule for NoLoggingErrorInExcept {
    fn name(&self) -> RuleName {
        RuleName("no-logging-error-in-except")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Logging, Tag::Exceptions]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeRule for NoLoggingErrorInExcept {
    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<crate::code_lint::SourceDoc>,
        _config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        grep.root()
            .find_all("logging.error($$$ARGS)")
            .filter(|call| {
                call.ancestors()
                    .any(|ancestor| ancestor.kind() == "except_clause")
            })
            .map(|call| self.diagnostic_at_node(path, &call, &[]))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn test_no_logging_error_in_except_rule() {
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
            &NoLoggingErrorInExcept,
            source_violating,
            "test.py",
        );
        insta::assert_snapshot!(output_violating, @"[no-logging-error-in-except] Line 4, Col 5: Banned use of `logging.error` inside except block.");

        let output_ok = crate::test_utils::assert_code_rule_snapshot(
            &NoLoggingErrorInExcept,
            source_ok,
            "test.py",
        );
        assert!(output_ok.is_empty());
    }
}
