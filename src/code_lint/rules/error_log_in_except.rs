//! Verifies that `logging.error` is not used inside python except blocks.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::rule::{CodeRule, RuleTarget};
use crate::code_lint::semantic::calls;
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::collections::HashSet;
use std::path::Path;

const BANNED: ListOption = ListOption {
    kind: ListKind::Deny,
    doc: "Logging calls flagged inside an `except` block.",
    default: FilterListDefaults {
        base: &["logging.error"],
        extend: &[],
        remove: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Call to `{call}(...)` inside an `except` block.",
    rationale: "Calling `logging.error` inside an `except` block either drops the active traceback (obscuring the root cause) or requires redundant `exc_info=True` boilerplate instead of the canonical `logging.exception`.",
    suggestion: "Replace with `logging.exception(...)` to capture and attach the active exception traceback automatically.",
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("error-log-in-except"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python],
        options: RuleOptions::code_rule(BANNED),
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
                           inside a function, `lambda` or class defined within the handler, or \
                           outside any `try`, are not flagged. By default only the \
                           module-level `logging.error` is matched: a logger instance call such \
                           as `logger.error(...)` is not flagged. A call passing \
                           `exc_info=True` is flagged too.",
            why_is_this_bad: "Inside an `except` block, the exception being handled is the \
                              most useful thing to log. By default `logging.error` records only \
                              the message and drops the traceback, while passing \
                              `exc_info=True` duplicates what `logging.exception` already \
                              expresses directly.\n\n\
                              Use `logging.exception(...)`, which logs at error level and \
                              attaches the active traceback by default.",
            references: &[Reference {
                title: "Python docs: logging.exception",
                url: "https://docs.python.org/3/library/logging.html#logging.exception",
            }],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

fn check_file(
    rule: &CodeRule<ListOption>,
    path: &Path,
    file: &ParsedFile,
    banned: &HashSet<String>,
) -> Vec<Diagnostic> {
    calls::find_banned_calls(file, banned)
        .into_iter()
        .filter(|matched| crate::code_lint::ast::python::is_inside_except_clause(&matched.node))
        .map(|matched| rule.diagnostic_at_node(path, &matched.node, &[("call", &matched.callee)]))
        .collect()
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
                logging_error_in_nested_function_inside_except => r#"
                    import logging

                    try:
                        run_job()
                    except RuntimeError:
                        def on_retry_failure():
                            logging.error("retry failed")
                        callback = lambda: logging.error("callback failed")
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
