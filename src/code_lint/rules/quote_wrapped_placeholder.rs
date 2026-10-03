//! Flags Python format placeholders wrapped in literal single or double quotes.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::collect_quote_wrapped_placeholders;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Format placeholder `{expression}` is wrapped in literal quotes.",
    rationale: "Manual quotes around a formatted value do not escape embedded quotes or control characters and make non-string values such as `None` or numbers indistinguishable from strings.",
    suggestion: "Replace the quoted placeholder with `{replacement}`, or wrap it in backticks when formatting a code identifier.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("quote-wrapped-placeholder"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::LITERALS],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags Python format placeholders wrapped in literal single or double quotes.",
            what_it_does: "Flags bare string-formatted placeholders wrapped in matching single or \
                           double quotes (`'{x}'`, `\"{x}\"`, `'%s'`, `'%(name)s'`) inside \
                           human-readable Python format strings across all files, tests included. \
                           Three formatting contexts are inspected: f-strings (`f\"...\"`), \
                           strings formatted via `.format(...)` or `.format_map(...)` (or \
                           `str.format(...)`), and strings formatted via the `%` operator or \
                           passed with format arguments to a logger call (`debug`, `info`, \
                           `warning`, `warn`, `error`, `exception`, `critical`, `fatal`, `log`). \
                           Plain unformatted strings, docstrings, raw strings (`r\"...\"`), byte \
                           strings (`b\"...\"`), placeholders that already carry a conversion \
                           flag (`!r`, `!s`, `!a`), format specifier (`:...`), or debug `=`, \
                           non-`%s` printf specifiers (`%r`, `%d`, `%.2f`), isolated quoted \
                           placeholders without surrounding prose (`f\"'{value}'\"`), and \
                           structured syntax (HTML attributes, SQL queries, JSON or TOML \
                           fragments, `key=\"value\"` flags, and backtick code spans) are not \
                           flagged.",
            why_is_this_bad: "Wrapping a default string placeholder in manual quotes fails when \
                              the runtime value contains the same quote character (`Invalid \
                              value 'can't'`) or control characters such as newlines and tabs, \
                              and it turns `None`, booleans, and numbers into quoted strings \
                              (`'None'`, `'42'`) that look identical to actual strings in logs \
                              and error messages.\n\n\
                              Use representation formatting (`{x!r}` in f-strings and `.format()`, \
                              `%r` or `%(name)r` in printf and logger format strings) so Python \
                              quotes and escapes strings automatically while preserving the \
                              representation of non-string types, or wrap the placeholder in \
                              backticks (`` `{x}` ``) when formatting a code identifier.",
            references: &[
                Reference {
                    title: "PEP 3101: Advanced String Formatting (conversion flags)",
                    url: "https://peps.python.org/pep-3101/",
                },
                Reference {
                    title: "PEP 498: Literal String Interpolation",
                    url: "https://peps.python.org/pep-0498/",
                },
            ],
            examples: &[Example {
                language: SupportLang::Python,
                flagged: indoc::indoc! {r#"
                    def parse_port(raw_port: str) -> int:
                        if not raw_port.isdigit():
                            raise ValueError(f"Invalid port '{raw_port}' in configuration")
                        return int(raw_port)
                "#},
                flagged_span: r#"f"Invalid port '{raw_port}' in configuration""#,
                fixed: indoc::indoc! {r#"
                    def parse_port(raw_port: str) -> int:
                        if not raw_port.isdigit():
                            raise ValueError(f"Invalid port {raw_port!r} in configuration")
                        return int(raw_port)
                "#},
            }],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    collect_quote_wrapped_placeholders(file)
        .into_iter()
        .map(|placeholder| {
            rule.diagnostic_at_node(
                path,
                &placeholder.node,
                &[
                    ("expression", &placeholder.expression),
                    ("replacement", &placeholder.replacement),
                ],
            )
        })
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                fstring_repr_conversion_allowed => r#"
                    message = f"Invalid value {value!r} in input"
                "#,
                fstring_backticks_allowed => r#"
                    message = f"Invalid identifier `{name}` in module"
                "#,
                fstring_already_has_repr_inside_quotes => r#"
                    message = f"Invalid value '{value!r}' in input"
                "#,
                fstring_explicit_str_or_ascii_conversion => r#"
                    first = f"Invalid value '{value!s}' in input"
                    second = f"Invalid value '{value!a}' in input"
                "#,
                fstring_with_format_specifier => r#"
                    price_text = f"Calculated price is '{price:.2f}' dollars"
                    date_text = f"Scheduled date is '{timestamp:%Y-%m-%d}' today"
                "#,
                fstring_with_debug_equals => r#"
                    debug_text = f"Observed result '{value=}' during run"
                "#,
                plain_unformatted_string_and_docstring => r#"
                    """Use '{name}' or '%s' as a template placeholder."""
                    template = "Invalid value '{name}' or '%s' in config"
                "#,
                awk_and_shell_strings => r#"
                    command = "awk '{print $1}' input.txt"
                    formatted_awk = "awk '{print $1}'".format()
                "#,
                zero_arg_logger_call => r#"
                    logger.info("Found literal '%s' token in input")
                "#,
                raw_and_byte_strings => r#"
                    raw_fstring = rf"Invalid pattern '{pattern}' in regex"
                    raw_format = r"Invalid pattern '{pattern}'".format(pattern=value)
                    byte_printf = b"Invalid token '%s' in stream" % raw_bytes
                "#,
                printf_already_percent_r_or_numeric => r#"
                    repr_msg = "Invalid value '%r' in input" % value
                    int_msg = "Processed '%d' items in batch" % count
                    float_msg = "Observed ratio '%.2f' in run" % ratio
                    escaped_pct = "Literal '%%s' in %s output" % name
                "#,
                str_format_escaped_braces_or_conversion => r#"
                    escaped = "Literal '{{name}}' and '{other!r}' in output".format(other=value)
                    formatted = "Measured '{score:.2f}' points".format(score=1.5)
                "#,
                html_and_xml_attributes => r#"
                    link = f'<a href="{url}" title=\'{title}\'>Click here</a>'
                "#,
                sql_queries => r#"
                    select_query = f"SELECT * FROM accounts WHERE username = '{username}'"
                    insert_query = f"INSERT INTO accounts VALUES ('{username}')"
                    like_query = f"SELECT * FROM accounts WHERE username LIKE '{pattern}'"
                "#,
                json_toml_and_key_value_syntax => r#"
                    json_text = f'{{"username": "{username}"}}'
                    toml_text = f'mode = "{mode}"'
                    cli_flag = f'--output="{output_path}"'
                "#,
                isolated_quote_wrapping => r#"
                    double_quoted = f'"{value}"'
                    single_quoted = f"'{value}'"
                "#,
                placeholder_inside_markdown_backticks => r#"
                    hint = f"Set `mode = '{mode}'` in the configuration file"
                    token_hint = f"Expected `'{token}'` in the input stream"
                "#,
                file_extension_and_host_port => r#"
                    filename = f"Loading module '{stem}'.py from disk"
                    endpoint = f"Connecting to '{host}':{port} now"
                "#,
                mismatched_quotes => r#"
                    mismatched = f"Invalid value '{value}\" in input"
                "#,
            ],
            fail: [
                fstring_single_quotes => r#"
                    message = f"Invalid value '{value}' in input"
                "# => r#"f"Invalid value '{value}' in input""#,
                fstring_double_quotes => r#"
                    message = f'Invalid value "{value}" in input'
                "# => r#"f'Invalid value "{value}" in input'"#,
                fstring_escaped_double_quotes => r#"
                    message = f"Invalid value \"{value}\" in input"
                "# => r#"f"Invalid value \"{value}\" in input""#,
                fstring_at_start_of_message => r#"
                    message = f"'{plugin_name}' is not a registered plugin"
                "# => r#"f"'{plugin_name}' is not a registered plugin""#,
                fstring_in_parentheses_and_before_period => r#"
                    message = f"Unknown plugin ('{plugin_name}')."
                "# => r#"f"Unknown plugin ('{plugin_name}').""#,
                fstring_complex_expression => r#"
                    message = f"Failed to load '{path.name}' from disk"
                "# => r#"f"Failed to load '{path.name}' from disk""#,
                str_format_named_placeholder => r#"
                    message = "Invalid value '{name}' in input".format(name=value)
                "# => r#""Invalid value '{name}' in input""#,
                str_format_empty_placeholder => r#"
                    message = "Invalid value '{}' in input".format(value)
                "# => r#""Invalid value '{}' in input""#,
                str_format_positional_placeholder => r#"
                    message = "Invalid value '{0}' in input".format(value)
                "# => r#""Invalid value '{0}' in input""#,
                str_format_map_call => r#"
                    message = "Invalid value '{key}' in input".format_map(mapping)
                "# => r#""Invalid value '{key}' in input""#,
                printf_percent_s => r#"
                    message = "Invalid value '%s' in input" % value
                "# => r#""Invalid value '%s' in input""#,
                printf_named_percent_s => r#"
                    message = "Invalid value '%(name)s' in input" % {"name": value}
                "# => r#""Invalid value '%(name)s' in input""#,
                logger_warning_percent_s => r#"
                    logger.warning("Failed to connect to '%s' on port %d", host, port)
                "# => r#""Failed to connect to '%s' on port %d""#,
                logger_log_level_percent_s => r#"
                    logging.log(logging.ERROR, "Failed to connect to '%s'", host)
                "# => r#""Failed to connect to '%s'""#,
            ],
        },
    }
);
