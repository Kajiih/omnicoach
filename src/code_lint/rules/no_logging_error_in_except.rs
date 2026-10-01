//! Verifies that `logging.error` is not used inside python except blocks.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::rule::CodeDetector;
use crate::code_lint::semantic::calls;
use crate::core::{
    Detector, FilterListDefaults, ListKind, ListOption, OptionSpec, ResolvedOptions, RuleOptions,
};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::Rule;
use crate::rule_documentation::{Reference, RuleDoc};
use crate::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Topic};
use ast_grep_language::SupportLang;
use std::path::Path;

const BANNED_CALLS: ListOption = ListOption {
    kind: ListKind::Deny,
    doc: "Logging calls flagged inside an `except` block.",
    default: FilterListDefaults {
        base: &["logging.error"],
        extend: &[],
        exempt: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Call to `{call}(...)` inside an `except` block.",
    rationale: "Logging inside an `except` block without exception context drops the active traceback, obscuring the root cause during debugging.",
    suggestion: "Replace with `logging.exception(...)` to capture and attach the active exception traceback automatically.",
};

/// Rule struct.
struct NoLoggingErrorInExcept;

/// The rule's declaration.
pub const RULE: Rule<dyn CodeDetector> = Rule {
    detector: &NoLoggingErrorInExcept,
    classification: Classification {
        topics: &[Topic::LOGGING, Topic::ERROR_HANDLING],
        precision: Precision::Exact,
        consensus: Consensus::Unopinionated,
        impacted_quality: ImpactedQuality::Reliability,
    },
    doc: RuleDoc {
        summary: "Flags `logging.error` calls inside Python `except` blocks.",
        what_it_does: "Flags calls to `logging.error(...)` anywhere inside an `except` \
                       block, including bare `except:` and nested blocks such as an `if` \
                       within the handler. Calls in the `try`, `else` and `finally` blocks, \
                       or outside any `try`, are not flagged. By default only the \
                       module-level `logging.error` is matched: a logger instance call such \
                       as `logger.error(...)` is not flagged. A call passing \
                       `exc_info=True` is flagged too.",
        why_is_this_bad: "Inside an `except` block, the exception being handled is the \
                          most useful thing to log. `logging.error` records only the \
                          message, so the log loses the traceback: the exception type and \
                          where it was raised. Whoever reads the log later has to guess the \
                          root cause.\n\n\
                          Use `logging.exception(...)`, which logs at error level and \
                          attaches the active traceback.",
        references: &[Reference {
            title: "Python docs: logging.exception",
            url: "https://docs.python.org/3/library/logging.html#logging.exception",
        }],
    },
    options: RuleOptions {
        options: &[OptionSpec::List(&BANNED_CALLS)],
        ..RuleOptions::CODE_RULE
    },
};

impl Detector for NoLoggingErrorInExcept {
    fn name(&self) -> RuleName {
        RuleName("no-logging-error-in-except")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeDetector for NoLoggingErrorInExcept {
    fn check_file(
        &self,
        path: &Path,
        file: &ParsedFile,
        options: &ResolvedOptions<'_>,
    ) -> Vec<Diagnostic> {
        calls::find_banned_calls(file, &options.list(&BANNED_CALLS))
            .into_iter()
            .filter(|matched| crate::code_lint::ast::python::is_inside_except_clause(&matched.node))
            .map(|matched| {
                self.diagnostic_at_node(path, &matched.node, &[("call", &matched.callee)])
            })
            .collect()
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
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
                logging_error_in_finally_block => r#"
                    import logging

                    try:
                        run_job()
                    except RuntimeError:
                        pass
                    finally:
                        logging.error("cleanup failed")
                "#,
                logger_instance_error_not_in_banned_calls => r#"
                    import logging

                    logger = logging.getLogger(__name__)
                    try:
                        run_job()
                    except RuntimeError:
                        logger.error("failed")
                "#,
            ],
            fail: [
                logging_error_in_typed_except => r#"
                    import logging

                    try:
                        run_job()
                    except ValueError as err:
                        logging.error("invalid value: %s", err)
                "# => r#"logging.error("invalid value: %s", err)"#,
                logging_error_in_bare_except => r#"
                    import logging

                    try:
                        run_job()
                    except:
                        logging.error("failed")
                "# => r#"logging.error("failed")"#,
                logging_error_in_nested_block_inside_except => r#"
                    import logging

                    try:
                        run_job()
                    except ValueError as err:
                        if err.args:
                            logging.error("invalid value: %s", err)
                "# => r#"logging.error("invalid value: %s", err)"#,
                logging_error_with_exc_info_in_except => r#"
                    import logging

                    try:
                        run_job()
                    except RuntimeError:
                        logging.error("failed", exc_info=True)
                "# => r#"logging.error("failed", exc_info=True)"#,
            ],
        },
    }
);
