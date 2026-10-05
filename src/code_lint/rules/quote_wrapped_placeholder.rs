//! Flags Python format placeholders wrapped in literal single or double quotes.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    PythonFormatPlaceholder, PythonFormatString, PythonFormatStyle, collect_format_strings,
    extract_valid_field_root,
};
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
    rationale: "Manual quotes around a string-formatted value do not escape embedded quotes or control characters and make non-string values such as `None` or numbers indistinguishable from strings.",
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
                           double quotes (`'{x}'`, `\"{x}\"`, `'{}'`, `'{0}'`, `'%s'`, `'%(name)s'`) \
                           inside Python format strings across all files, tests \
                           included. Three formatting contexts are inspected: f-strings \
                           (`f\"...\"`), strings formatted via `.format(...)` or \
                           `.format_map(...)` (or `str.format(...)`), and strings formatted via \
                           the `%` operator or passed with format arguments to a logger call \
                           (`debug`, `info`, `warning`, `warn`, `error`, `exception`, `critical`, \
                           `fatal`, `log`). Plain unformatted strings, docstrings, raw strings \
                           (`r\"...\"`), byte strings (`b\"...\"`), placeholders that already \
                           carry a conversion flag (`!r`, `!s`, `!a`), format specifier (`:...`), \
                           or debug `=`, non-`%s` printf specifiers (`%r`, `%d`, `%.2f`), \
                           and structured syntax (HTML attributes, JSON or TOML fragments, \
                           `key=\"value\"` flags, and backtick code spans) are not flagged.",
            why_is_this_bad: "Wrapping a default string placeholder in manual quotes fails when \
                              the runtime value contains the same quote character (`Invalid \
                              value 'can't'`) or control characters such as newlines and tabs, \
                              and it turns `None`, booleans, and numbers into quoted strings \
                              (`'None'`, `'42'`) that look identical to actual strings in logs \
                              and error messages.\n\n\
                              Use `repr` formatting (`{x!r}` in f-strings and `.format()`, `%r` \
                              or `%(name)r` in printf and logger format strings) so strings are \
                              quoted and escaped automatically via `repr()` while preserving the \
                              representation of non-string types, or wrap the placeholder in \
                              backticks (`` `{x}` ``) when formatting a code identifier.",
            references: &[
                Reference {
                    title: "PEP 3101: Advanced String Formatting (conversion flags)",
                    url: "https://peps.python.org/pep-3101/",
                },
                Reference {
                    title: "Python Documentation: Built-in Functions — repr()",
                    url: "https://docs.python.org/3/library/functions.html#repr",
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
    let mut diagnostics = Vec::new();
    for format_string in collect_format_strings(file) {
        for (index, placeholder) in format_string.placeholders.iter().enumerate() {
            let Some((body, replacement)) = repr_replacement(format_string.style, placeholder)
            else {
                continue;
            };
            let neighbors = placeholder_neighbors(&format_string, index);
            if let Some(expression) = match_prose_quoted_placeholder(&neighbors, &body) {
                diagnostics.push(rule.diagnostic_at_node(
                    path,
                    &format_string.node,
                    &[("expression", &expression), ("replacement", &replacement)],
                ));
            }
        }
    }
    diagnostics
}

/// Returns the placeholder as matched inside quotes and its `repr` replacement, or `None` when
/// the placeholder already sets a conversion or format spec, or is not a bare field.
fn repr_replacement(
    style: PythonFormatStyle,
    placeholder: &PythonFormatPlaceholder,
) -> Option<(String, String)> {
    let field = &placeholder.field;
    match style {
        PythonFormatStyle::FString => (placeholder.conversion.is_none()
            && placeholder.format_spec.is_none()
            && !placeholder.is_self_documenting
            && !field.is_empty())
        .then(|| (format!("{{{field}}}"), format!("{{{field}!r}}"))),
        PythonFormatStyle::StrFormat => (placeholder.conversion.is_none()
            && placeholder.format_spec.is_none()
            && extract_valid_field_root(field).is_some())
        .then(|| (placeholder.text.clone(), format!("{{{field}!r}}"))),
        PythonFormatStyle::Printf => {
            if placeholder.conversion.as_deref() != Some("s") || placeholder.format_spec.is_some() {
                return None;
            }
            if field.is_empty() {
                Some((placeholder.text.clone(), "%r".to_owned()))
            } else if field
                .chars()
                .all(|character| character.is_alphanumeric() || character == '_')
            {
                Some((placeholder.text.clone(), format!("%({field})r")))
            } else {
                None
            }
        }
    }
}

/// Text around one placeholder, as the prose and quote heuristics read it.
struct PlaceholderNeighbors {
    /// Text immediately preceding the placeholder.
    before: String,
    /// Text from the start of the (possibly concatenated) string up to the placeholder.
    full_before: String,
    /// The part of `full_before` checked for unclosed `{` / `[` delimiters.
    structure_before: String,
    /// Text immediately following the placeholder.
    after: String,
    /// True when `after` is immediately followed by another placeholder.
    followed_by_placeholder: bool,
}

/// Builds the neighbors of `format_string.placeholders[index]`.
///
/// An f-string's neighbors are its literal segments, since its fields are expressions. A
/// `.format()` or `%` template is plain text, so its neighbors include the other fields as
/// written; `.format()` fields are left out of `structure_before` so their braces do not count
/// as delimiters.
fn placeholder_neighbors(
    format_string: &PythonFormatString<'_>,
    index: usize,
) -> PlaceholderNeighbors {
    let literals = &format_string.literals;
    let preceding = &format_string.preceding_text;
    let literal_before = format!("{preceding}{}", literals[..=index].concat());
    if format_string.style == PythonFormatStyle::FString {
        return PlaceholderNeighbors {
            before: if index == 0 {
                literal_before.clone()
            } else {
                literals[index].clone()
            },
            full_before: literal_before.clone(),
            structure_before: literal_before,
            after: literals[index + 1].clone(),
            followed_by_placeholder: index + 1 < format_string.placeholders.len(),
        };
    }
    let mut template = String::new();
    let mut placeholder_span = (0, 0);
    for (position, (literal, placeholder)) in
        literals.iter().zip(&format_string.placeholders).enumerate()
    {
        template.push_str(literal);
        let start = template.len();
        template.push_str(&placeholder.text);
        if position == index {
            placeholder_span = (start, template.len());
        }
    }
    template.push_str(literals.last().map_or("", String::as_str));
    let full_before = format!("{preceding}{}", &template[..placeholder_span.0]);
    PlaceholderNeighbors {
        before: full_before.clone(),
        structure_before: if format_string.style == PythonFormatStyle::StrFormat {
            literal_before
        } else {
            full_before.clone()
        },
        full_before,
        after: template[placeholder_span.1..].to_owned(),
        followed_by_placeholder: false,
    }
}

/// Replaces escape sequences (`\n`, `\t`, `\"`, etc.) with spaces so escape letters are not
/// mistaken for prose words.
fn strip_escape_sequences(text: &str) -> String {
    let mut cleaned = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            characters.next();
            cleaned.push(' ');
        } else {
            cleaned.push(character);
        }
    }
    cleaned
}

/// Extracted quote pair surrounding a placeholder, together with the text before the opening
/// quote and after the closing quote.
struct MatchedQuotePair<'a> {
    opening: &'a str,
    closing: &'a str,
    prefix_before: &'a str,
    suffix_after: &'a str,
}

/// Checks whether `before` ends with a single or double quote (unescaped or single-backslash
/// escaped) and `after` starts with the matching quote character.
fn extract_matching_quote_pair<'a>(
    before: &'a str,
    after: &'a str,
) -> Option<MatchedQuotePair<'a>> {
    let quote = *before.as_bytes().last()?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    let quote_char = char::from(quote);
    let without_quote = &before[..before.len() - 1];
    let trailing_backslashes = without_quote
        .bytes()
        .rev()
        .take_while(|&byte| byte == b'\\')
        .count();
    let (opening, prefix_before) = match trailing_backslashes {
        0 => (&before[before.len() - 1..], without_quote),
        1 => (
            &before[before.len() - 2..],
            &without_quote[..without_quote.len() - 1],
        ),
        _ => return None,
    };
    if prefix_before.ends_with(quote_char) {
        return None;
    }

    let (closing, suffix_after) = if after.starts_with(quote_char) {
        (&after[..1], &after[1..])
    } else if after.as_bytes().first() == Some(&b'\\') && after.as_bytes().get(1) == Some(&quote) {
        (&after[..2], &after[2..])
    } else {
        return None;
    };
    if suffix_after.starts_with(quote_char) {
        return None;
    }

    Some(MatchedQuotePair {
        opening,
        closing,
        prefix_before,
        suffix_after,
    })
}

/// Returns true if `text` has any unclosed `{` / `{{` or `[` delimiter, such as inside a JSON or
/// list literal (`f'{{"key": "{value}"}}'`).
fn has_unclosed_structured_delimiter(text: &str) -> bool {
    let mut brace_depth = 0_i32;
    let mut bracket_depth = 0_i32;
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let current = bytes[index];
        if current == b'{' && bytes.get(index + 1) == Some(&b'{') {
            brace_depth += 1;
            index += 2;
            continue;
        }
        if current == b'}' && bytes.get(index + 1) == Some(&b'}') {
            brace_depth = (brace_depth - 1).max(0);
            index += 2;
            continue;
        }
        match current {
            b'{' => brace_depth += 1,
            b'}' => brace_depth = (brace_depth - 1).max(0),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = (bracket_depth - 1).max(0),
            _ => {}
        }
        index += 1;
    }
    brace_depth > 0 || bracket_depth > 0
}

/// Returns true if the text before the opening quote satisfies the left prose boundary rules.
fn has_valid_left_prose_boundary(
    prefix_before_quote: &str,
    full_prefix_before_quote: &str,
    structure_prefix_before_quote: &str,
) -> bool {
    let backtick_count = full_prefix_before_quote
        .bytes()
        .filter(|&byte| byte == b'`')
        .count();
    if backtick_count % 2 != 0 || has_unclosed_structured_delimiter(structure_prefix_before_quote) {
        return false;
    }

    let starts_at_beginning = prefix_before_quote.is_empty() && full_prefix_before_quote.is_empty();
    let preceded_by_space_or_paren = prefix_before_quote.ends_with([' ', '\t', '\n', '\r', '('])
        || WHITESPACE_ESCAPES
            .iter()
            .any(|escape| prefix_before_quote.ends_with(escape));
    if !starts_at_beginning && !preceded_by_space_or_paren {
        return false;
    }

    let cleaned_prefix = strip_escape_sequences(full_prefix_before_quote);
    let trimmed = cleaned_prefix.trim_end();
    let Some(last_char) = trimmed.chars().next_back() else {
        return true;
    };
    last_char.is_ascii_alphanumeric() || matches!(last_char, ':' | ',' | '(' | '.' | '!' | '?')
}

/// Whitespace escape sequences as they appear in undecoded source text.
const WHITESPACE_ESCAPES: [&str; 3] = ["\\n", "\\t", "\\r"];

/// Returns true if `character` is a sentence punctuation mark allowed immediately after a closing
/// prose quote.
const fn is_prose_punctuation(character: char) -> bool {
    matches!(character, '.' | ',' | ';' | ':' | '!' | '?' | ')')
}

/// Returns true if `suffix_after_quote` satisfies the right prose boundary rules.
fn has_valid_right_prose_boundary(suffix_after_quote: &str, followed_by_placeholder: bool) -> bool {
    if suffix_after_quote.is_empty() {
        return !followed_by_placeholder;
    }
    if WHITESPACE_ESCAPES
        .iter()
        .any(|escape| suffix_after_quote.starts_with(escape))
    {
        return true;
    }
    let mut characters = suffix_after_quote.chars();
    let Some(first_char) = characters.next() else {
        return !followed_by_placeholder;
    };
    if first_char.is_ascii_whitespace() {
        return true;
    }
    if !is_prose_punctuation(first_char) {
        return false;
    }
    characters
        .next()
        .map_or(!followed_by_placeholder, |second_char| {
            second_char.is_ascii_whitespace()
                || is_prose_punctuation(second_char)
                || matches!(second_char, '\'' | '"')
        })
}

/// Checks whether a placeholder surrounded by `neighbors` is quote-wrapped in a prose context,
/// returning the quote-wrapped expression if so.
fn match_prose_quoted_placeholder(
    neighbors: &PlaceholderNeighbors,
    placeholder_body: &str,
) -> Option<String> {
    let matched = extract_matching_quote_pair(&neighbors.before, &neighbors.after)?;
    let opening_length = matched.opening.len();
    let full_prefix_before_quote =
        &neighbors.full_before[..neighbors.full_before.len() - opening_length];
    let structure_prefix_before_quote =
        &neighbors.structure_before[..neighbors.structure_before.len() - opening_length];
    if !has_valid_left_prose_boundary(
        matched.prefix_before,
        full_prefix_before_quote,
        structure_prefix_before_quote,
    ) {
        return None;
    }
    if !has_valid_right_prose_boundary(matched.suffix_after, neighbors.followed_by_placeholder) {
        return None;
    }
    Some(format!(
        "{open}{placeholder_body}{close}",
        open = matched.opening,
        close = matched.closing,
    ))
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
                json_toml_and_key_value_syntax => r#"
                    json_text = f'{{"username": "{username}"}}'
                    toml_text = f'mode = "{mode}"'
                    cli_flag = f'--output="{output_path}"'
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
                escaped_braces_json_in_fstring_and_str_format => r#"
                    fstring_json = f'{{"key": "{value}", "mode": 1}}'
                    format_json = '{{"key": "{}"}}'.format(value)
                "#,
                concatenated_structured_prefix_in_str_format_and_logger => r#"
                    format_flag = ("Pass --output=" "'{path}'").format(path=output_path)
                    logger.info("Set `mode = " "'%s'` in config", mode)
                "#,
                spaced_non_identifier_braces_in_str_format => r#"
                    spaced = "Invalid '{ name }' in input".format()
                "#,
                concatenated_flag_and_backtick_prefix_in_fstring => r#"
                    flag = "Pass --output=" f"'{output_path}'"
                    code = "Run `mode = " f"'{mode}'` in config"
                "#,
            ],
            fail: [
                isolated_fstring_single_quotes => r#"
                    labels = ", ".join(f"'{name}'" for name in names)
                "# => r#"f"'{name}'""#,
                isolated_fstring_double_quotes => r#"
                    label = f'"{value}"'
                "# => r#"f'"{value}"'"#,
                isolated_str_format => r#"
                    label = "'{value}'".format(value=x)
                "# => r#""'{value}'""#,
                isolated_printf => r#"
                    label = "'%(name)s'" % {"name": x}
                "# => r#""'%(name)s'""#,
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
                prose_starting_with_capitalized_english_verb => r#"
                    message = "Update of '%s' failed" % item
                "# => r#""Update of '%s' failed""#,
                prose_with_preposition_before_quoted_placeholder => r#"
                    message = f"Missing key in '{section}'"
                "# => r#"f"Missing key in '{section}'""#,
                prose_with_from_preposition_before_quoted_placeholder => r#"
                    message = f"Cannot read config from '{path}'"
                "# => r#"f"Cannot read config from '{path}'""#,
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
                compound_logger_receiver_percent_s => r#"
                    app.logger.info("Failed to connect to '%s'", host)
                "# => r#""Failed to connect to '%s'""#,
                logger_with_star_args_percent_s => r#"
                    logger.info("Failed to connect to '%s'", *args)
                "# => r#""Failed to connect to '%s'""#,
                fstring_second_placeholder => r#"
                    message = f"Copied {source} to '{target}'"
                "# => r#"f"Copied {source} to '{target}'""#,
                str_format_second_placeholder => r#"
                    message = "Copied {} to '{target}'".format(source, target=dest)
                "# => r#""Copied {} to '{target}'""#,
                printf_second_placeholder => r#"
                    message = "Copied %(source)s to '%(target)s'" % values
                "# => r#""Copied %(source)s to '%(target)s'""#,
                concatenated_fstring_shares_prose_context => r#"
                    message = (
                        "Failed to load configuration for "
                        f"'{service_name}'"
                    )
                "# => r#"f"'{service_name}'""#,
                sql_query_not_special_cased => r#"
                    query = f"SELECT * FROM accounts WHERE username LIKE '{pattern}'"
                "# => r#"f"SELECT * FROM accounts WHERE username LIKE '{pattern}'""#,
            ],
        },
    }
);
