//! Verifies that `logging.error` is not used inside python except blocks.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
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
    summary: "`{callee}()` is called inside an `except` block.",
    rationale: "Logging at error level inside an `except` block drops the active traceback unless `exc_info=True` is repeated at every call, so the root cause is lost.",
    suggestion: "Replace the call with `logging.exception(...)`, which attaches the active traceback automatically.",
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("error-log-in-except"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(BANNED),
        classification: Classification {
            topics: &[Topic::LOGGING, Topic::ERROR_HANDLING],
            precision: Precision::Exact,
            consensus: Consensus::Unopinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags error-level logging calls inside Python `except` blocks.",
            what_it_does: indoc::indoc! {r"
                Flags calls to `logging.error(...)` anywhere inside an `except` block, including
                bare `except:` and nested blocks such as an `if` within the handler. Calls in the
                `try`, `else` and `finally` blocks, inside a function, `lambda` or class defined
                within the handler, or outside any `try`, are not flagged. By default only the
                module-level `logging.error` is matched: a logger instance call such as
                `logger.error(...)` is not flagged. A call passing `exc_info=True` is flagged too."},
            why_is_this_bad: indoc::indoc! {r"
                Inside an `except` block, the exception being handled is the most useful thing to
                log. By default `logging.error` records only the message and drops the traceback,
                while passing `exc_info=True` duplicates what `logging.exception` already expresses
                directly.

                Use `logging.exception(...)`, which logs at error level and attaches the active
                traceback by default."},
            references: &[Reference {
                title: "Python docs: logging.exception",
                url: "https://docs.python.org/3/library/logging.html#logging.exception",
            }],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r#"
                    try:
                        sync_inventory()
                    except ConnectionError:
                        logging.error("Inventory sync failed")
                "#},
                flagged_span: r#"logging.error("Inventory sync failed")"#,
                fixed: indoc::indoc! {r#"
                    try:
                        sync_inventory()
                    except ConnectionError:
                        logging.exception("Inventory sync failed")
                "#},
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
    rule.check_banned_calls_where(path, file, banned, |matched| matched.is_in_except_clause)
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
                "#,
                logging_error_in_lambda_inside_except => r#"
                    import logging

                    try:
                        run_job()
                    except RuntimeError:
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
