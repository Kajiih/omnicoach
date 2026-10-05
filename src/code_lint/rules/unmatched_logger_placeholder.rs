//! Flags Python logger calls that pass positional arguments to a message with an unmatched named
//! placeholder.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{collect_logger_calls, named_format_field_roots};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "`{callee}()` passes positional arguments to a message with unmatched named placeholder `{{name}}`.",
    rationale: "Positional format arguments do not bind to named placeholders, so formatting raises `KeyError` or `TypeError` at runtime.",
    suggestion: "Pass `{name}=...` as a keyword argument, or replace `{{name}}` with `{}` (or `%s` for `logging`).",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("unmatched-logger-placeholder"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::LOGGING],
            precision: Precision::Exact,
            consensus: Consensus::Unopinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags logger calls that pass positional arguments to a message with an unmatched named placeholder.",
            what_it_does: "Flags calls to `logger.<level>(...)`, `log.<level>(...)`, \
                           `_logger.<level>(...)`, `_log.<level>(...)`, `logging.<level>(...)`, \
                           `<expr>.logger.<level>(...)`, `<expr>.log.<level>(...)`, \
                           `<expr>._logger.<level>(...)` and `<expr>._log.<level>(...)` (across \
                           `trace`, `debug`, `info`, `success`, `warning`, `warn`, `error`, \
                           `critical`, `fatal`, `exception` and `log`) when the message string \
                           literal contains a named replacement field such as `{order_id}`, at \
                           least one positional format argument (or `*args` unpacking) is passed \
                           after the message, no `**kwargs` unpacking is present, and no \
                           matching `order_id=...` keyword argument is provided. For \
                           `.log(level, message, ...)`, the second positional argument is \
                           inspected as the message.",
            why_is_this_bad: "When refactoring an f-string log call such as \
                              `logger.info(f\"Order {order_id} filled\")` to use lazy logger \
                              formatting, stripping the `f` prefix and appending `order_id` \
                              positionally leaves `{order_id}` in the format string. Positional \
                              arguments never bind to named placeholders: `loguru` and \
                              `str.format` raise `KeyError: 'order_id'` at runtime, while \
                              standard library `logging` raises `TypeError` because the string \
                              has no `%` specifiers.\n\n\
                              Either pass `order_id=order_id` as a keyword argument (which \
                              `loguru` formats and captures into `record[\"extra\"]`), or \
                              replace `{order_id}` with positional `{}` (for `loguru`) or `%s` \
                              (for standard library `logging`).",
            references: &[
                Reference {
                    title: "Loguru documentation: formatting and extra context",
                    url: "https://loguru.readthedocs.io/en/stable/api/logger.html",
                },
                Reference {
                    title: "Python docs: Format String Syntax (PEP 3101)",
                    url: "https://docs.python.org/3/library/string.html#formatstrings",
                },
            ],
            examples: &[Example {
                language: SupportLang::Python,
                flagged: indoc::indoc! {r#"
                    logger.info("Order {order_id} filled", order_id)
                "#},
                flagged_span: r#"logger.info("Order {order_id} filled", order_id)"#,
                fixed: indoc::indoc! {r#"
                    logger.info("Order {} filled", order_id)
                "#},
            }],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    collect_logger_calls(file)
        .into_iter()
        .filter(|call| call.has_trailing_positional_args && !call.has_keyword_splat)
        .filter_map(|call| {
            let placeholder = named_format_field_roots(call.message.as_deref()?)?
                .into_iter()
                .find(|root| !call.keyword_names.contains(root))?;
            Some(rule.diagnostic_at_node(
                path,
                &call.node,
                &[("callee", &call.callee), ("name", &placeholder)],
            ))
        })
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                positional_empty_braces_with_positional_arg => r#"
                    logger.info("Order {} filled", order_id)
                "#,
                positional_numbered_braces_with_positional_arg => r#"
                    logger.info("Order {0} filled for {1}", order_id, user_id)
                "#,
                positional_compound_attribute_and_subscript => r#"
                    logger.info("Order {0.id} item {0[sku]}", order)
                "#,
                positional_with_conversion_and_format_spec => r#"
                    logger.info("Latency {:.2f} for {!r}", elapsed, request)
                "#,
                named_placeholder_with_matching_keyword_arg => r#"
                    logger.info("Order {order_id} filled", order_id=order_id)
                "#,
                named_compound_placeholder_with_matching_keyword_arg => r#"
                    logger.info("Order {order.id} item {items[0]}", order=order, items=items)
                "#,
                mixed_positional_and_matched_named_placeholder => r#"
                    logger.info("Order {} for {user_id}", order_id, user_id=user_id)
                "#,
                zero_format_args_with_literal_braces => r#"
                    logger.info("Registered FastAPI route /orders/{order_id}")
                "#,
                zero_positional_format_args_with_structlog_kwargs => r#"
                    logger.info("GET /orders/{order_id}", status_code=200, elapsed=12)
                "#,
                zero_positional_format_args_with_exc_info_kwarg => r#"
                    logger.error("Failed on route /orders/{order_id}", exc_info=True)
                "#,
                escaped_double_braces_with_positional_arg => r#"
                    logger.info("Literal {{order_id}} for {}", order_id)
                "#,
                f_string_message_not_flagged => r#"
                    logger.info(f"Order {order_id} filled: {}", status)
                "#,
                dictionary_splat_kwargs_exempt => r#"
                    logger.info("Order {order_id} filled", extra_positional, **context)
                "#,
                non_identifier_braces_json_or_set_with_positional_arg => r#"
                    logger.info("Payload {\"order_id\": 1} and {a, b}: %s", status)
                "#,
                malformed_unclosed_brace_ignored => r#"
                    logger.info("Malformed {order_id in input: %s", status)
                "#,
                logger_log_level_first_arg_with_valid_message => r#"
                    logger.log("INFO", "Order {} filled", order_id)
                    logger.log("{named_level}", "Order {} filled", order_id)
                "#,
                non_logger_call_ignored => r#"
                    formatter.info("Order {order_id} filled", order_id)
                    template.format("Order {order_id}", order_id)
                "#,
                unicode_named_character_escape_not_flagged => r#"
                    logger.info("Item \N{BULLET} %s", item)
                    logger.info("Arrow \N{RIGHTWARDS ARROW} {}", target)
                "#,
            ],
            fail: [
                named_placeholder_with_positional_arg_on_logger => r#"
                    logger.info("Order {order_id} filled", order_id)
                "# => r#"logger.info("Order {order_id} filled", order_id)"#,
                named_placeholder_with_positional_arg_on_log => r#"
                    log.warning("Retry {attempt} failed", attempt)
                "# => r#"log.warning("Retry {attempt} failed", attempt)"#,
                named_placeholder_with_positional_arg_on_private_logger => r#"
                    _logger.info("Order {order_id} filled", order_id)
                "# => r#"_logger.info("Order {order_id} filled", order_id)"#,
                named_placeholder_with_star_args_unpacking => r#"
                    logger.info("Order {order_id} filled", *args)
                "# => r#"logger.info("Order {order_id} filled", *args)"#,
                raw_string_backslash_n_is_not_unicode_escape => r#"
                    logger.info(r"Path \N{order_id}", order_id)
                "# => r#"logger.info(r"Path \N{order_id}", order_id)"#,
                named_placeholder_with_positional_arg_on_logging_module => r#"
                    logging.error("Request {request_id} failed", request_id)
                "# => r#"logging.error("Request {request_id} failed", request_id)"#,
                named_placeholder_with_positional_arg_on_self_logger => r#"
                    self.logger.debug("Peer {peer_id} connected", peer_id)
                "# => r#"self.logger.debug("Peer {peer_id} connected", peer_id)"#,
                named_placeholder_with_conversion_and_format_spec => r#"
                    logger.error("Order {order_id!r} amount {amount:.2f}", order_id, amount)
                "# => r#"logger.error("Order {order_id!r} amount {amount:.2f}", order_id, amount)"#,
                named_placeholder_with_attribute_or_subscript_access => r#"
                    logger.info("Order {order.id} filled", order)
                "# => r#"logger.info("Order {order.id} filled", order)"#,
                partially_matched_named_placeholders_flags_unmatched => r#"
                    logger.info("Order {order_id} for {user_id}", user_id, order_id=123)
                "# => r#"logger.info("Order {order_id} for {user_id}", user_id, order_id=123)"#,
                logger_log_method_checks_second_positional_arg => r#"
                    logger.log(20, "Order {order_id} filled", order_id)
                "# => r#"logger.log(20, "Order {order_id} filled", order_id)"#,
                implicitly_concatenated_message_string => r#"
                    logger.info(
                        "Order {order_id} filled "
                        "for account",
                        order_id,
                    )
                "# => r#"logger.info(
                        "Order {order_id} filled "
                        "for account",
                        order_id,
                    )"#,
                nested_format_spec_named_placeholder_unmatched => r#"
                    logger.info("Value {:>{width}}", value, width)
                "# => r#"logger.info("Value {:>{width}}", value, width)"#,
                escaped_braces_before_unmatched_named_placeholder => r#"
                    logger.info("Literal {{escaped}} for {order_id}", order_id)
                "# => r#"logger.info("Literal {{escaped}} for {order_id}", order_id)"#,
            ],
        },
    }
);
