//! Verifies that `logging.error` is not used inside python except blocks.

use crate::code_lint::CodeRule;
use crate::code_lint::calls::find_banned_calls;
use crate::core::{Config, DenyListConfig, DynamicRuleConfig, FilterListDefaults, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Configuration for the `NoLoggingErrorInExcept` rule.
pub type NoLoggingErrorInExceptConfig = DynamicRuleConfig<DenyListConfig>;

/// Static defaults for banned logging calls inside except blocks.
const DEFAULT_BANNED_CALLS: FilterListDefaults = FilterListDefaults {
    base: &["logging.error"],
    extend: &[],
    exempt: &[],
};

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
        config: &Config,
    ) -> Vec<Diagnostic> {
        let rule_config: NoLoggingErrorInExceptConfig = config.get_rule_config(self.name().0);
        let effective_banned =
            rule_config.effective_banned_for_lang(*grep.lang(), &DEFAULT_BANNED_CALLS);

        find_banned_calls(grep, &effective_banned)
            .into_iter()
            .filter(|matched| {
                matched
                    .node
                    .ancestors()
                    .any(|ancestor| ancestor.kind() == "except_clause")
            })
            .map(|matched| self.diagnostic_at_node(path, &matched.node, &[]))
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
