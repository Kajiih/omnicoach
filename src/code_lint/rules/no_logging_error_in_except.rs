//! Verifies that `logging.error` is not used inside python except blocks.

use crate::code_lint::CodeRule;
use crate::core::{Config, FilterListDefaults, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Static defaults for banned logging calls inside except blocks.
const DEFAULT_BANNED_CALLS: FilterListDefaults = FilterListDefaults {
    base: &["logging.error"],
    extend: &[],
    exempt: &[],
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Call to `{call}(...)` inside an `except` block.",
    rationale: "Logging inside an `except` block without exception context drops the active traceback, obscuring the root cause during debugging.",
    suggestion: "Replace with `logging.exception(...)` to capture and attach the active exception traceback automatically.",
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
        self.find_configured_banned_calls(grep, config, &DEFAULT_BANNED_CALLS)
            .into_iter()
            .filter(|matched| crate::code_lint::ast_python::is_inside_except_clause(&matched.node))
            .map(|matched| {
                self.diagnostic_at_node(path, &matched.node, &[("call", &matched.callee)])
            })
            .collect()
    }
}

#[cfg(test)]
crate::rule_test!(
    NoLoggingErrorInExcept,
    {
        Python => {
            pass: [
                logging_exception_in_except => r#"
                    import logging

                    try:
                        run_job()
                    except RuntimeError:
                        logging.exception("failed")
                "#,
                logging_error_outside_except => r#"
                    import logging

                    logging.error("failed")
                "#,
                logging_error_in_else_block => r#"
                    import logging

                    try:
                        run_job()
                    except RuntimeError:
                        pass
                    else:
                        logging.error("unexpected state")
                "#,
            ],
            fail: [
                logging_error_in_bare_except => r#"
                    import logging

                    try:
                        run_job()
                    except:
                        logging.error("failed")
                "# => [r#"logging.error("failed")"#],
                logging_error_in_typed_except => r#"
                    import logging

                    try:
                        run_job()
                    except ValueError as err:
                        logging.error("invalid value: %s", err)
                "# => [r#"logging.error("invalid value: %s", err)"#],
            ],
        },
    }
);
