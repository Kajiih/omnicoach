//! Collector and prose/quote heuristics for `quote-wrapped-placeholder`.

// omni:disable-file [repeated-literal] -- Tree-sitter node kinds and field names (see ROADMAP)

use super::{
    AstNode, ParsedFile, RawNode, append_string_literal_segments, delimited_string_parts,
    extract_logger_call, extract_valid_field_root, fstring_segments_and_interpolations,
    outermost_string_expression, positional_call_arguments, preceding_concatenated_literal_text,
    string_prefix_flags,
};

/// Uppercase SQL statement keywords that mark a string as a SQL query rather than prose.
const SQL_STATEMENT_KEYWORDS: &[&str] = &[
    "SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "ALTER", "DROP", "WITH", "REPLACE", "MERGE",
    "PRAGMA", "EXPLAIN", "TRUNCATE", "GRANT", "REVOKE", "WHERE",
];

/// A quote-wrapped format placeholder found in a Python f-string, `.format()`, or `%`-formatted
/// string literal.
#[derive(Clone)]
pub struct PythonQuoteWrappedPlaceholder<'a> {
    /// The `string` AST node containing the quote-wrapped placeholder.
    pub node: AstNode<'a>,
    /// The quote-wrapped placeholder as written in source (such as `'{x}'`, `"{}"`, `'%s'`).
    pub expression: String,
    /// The canonical `repr`-formatted replacement (such as `{x!r}`, `{!r}`, `%r`, `%(name)r`).
    pub replacement: String,
}

/// Active string-formatting mechanism for a Python `string` node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PythonFormatContext {
    /// An `f` / `F` prefixed string literal (`f"... {x} ..."`).
    FString,
    /// A string literal formatted via `.format(...)`, `.format_map(...)`, or `str.format(...)`.
    StrFormat,
    /// A string literal formatted via `%` or passed with format arguments to a `logging` call.
    Printf,
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

/// Returns true if `literal_text` contains at least one prose word ($\ge 2$ consecutive ASCII
/// letters) and does not start with an uppercase SQL statement keyword.
fn is_prose_message_text(literal_text: &str) -> bool {
    let cleaned = strip_escape_sequences(literal_text);
    let has_word = cleaned
        .split(|character: char| !character.is_ascii_alphabetic())
        .any(|word| word.len() >= 2);
    if !has_word {
        return false;
    }
    let first_word = cleaned
        .trim_start_matches(|character: char| character.is_ascii_whitespace() || character == '(')
        .split(|character: char| !character.is_ascii_alphabetic())
        .next()
        .unwrap_or_default();
    !SQL_STATEMENT_KEYWORDS.contains(&first_word)
}

/// Finds the closing `}` byte offset of an unescaped `{...}` format placeholder starting at
/// `open_index`, returning `None` if an inner `{` or `}}` is encountered first.
fn find_brace_placeholder_end(bytes: &[u8], open_index: usize) -> Option<usize> {
    let mut scan = open_index + 1;
    while scan < bytes.len() {
        let current = bytes[scan];
        if current == b'}' && bytes.get(scan + 1) != Some(&b'}') {
            return Some(scan);
        }
        if current == b'{' {
            return None;
        }
        scan += 1;
    }
    None
}

/// Strips unescaped `{...}` format placeholders from `text` while preserving escaped `{{` and `}}`
/// so placeholder identifiers and braces are not mistaken for prose words or literal containers.
fn strip_brace_placeholders(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut stripped = String::with_capacity(text.len());
    let mut cursor = 0;
    let mut index = 0;
    while index < bytes.len() {
        let current = bytes[index];
        let is_escaped = (current == b'{' && bytes.get(index + 1) == Some(&b'{'))
            || (current == b'}' && bytes.get(index + 1) == Some(&b'}'));
        if is_escaped {
            index += 2;
            continue;
        }
        if current == b'{'
            && let Some(close_index) = find_brace_placeholder_end(bytes, index)
        {
            stripped.push_str(&text[cursor..index]);
            index = close_index + 1;
            cursor = index;
            continue;
        }
        index += 1;
    }
    stripped.push_str(&text[cursor..]);
    stripped
}

/// Surrounding literal text slices for a single placeholder candidate.
struct PlaceholderNeighbors<'a> {
    /// Immediate literal text slice preceding the placeholder.
    before: &'a str,
    /// Cumulative literal text from the start of the (possibly concatenated) string up to the placeholder.
    full_before: &'a str,
    /// Immediate literal text slice following the placeholder.
    after: &'a str,
    /// True when `after` is immediately followed by another interpolation.
    followed_by_interpolation: bool,
    /// True when unescaped `{...}` placeholders in `full_before` should be stripped before
    /// checking for unclosed structured delimiters (`.format()` in Python).
    strip_brace_placeholders: bool,
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

/// Returns true if `text` (after optionally stripping `{field}` placeholders) has any unclosed
/// `{` / `{{` or `[` delimiter, such as inside a JSON or list literal (`f'{{"key": "{value}"}}'`).
fn has_unclosed_structured_delimiter(text: &str, strip_braces: bool) -> bool {
    let stripped;
    let literal_text = if strip_braces {
        stripped = strip_brace_placeholders(text);
        stripped.as_str()
    } else {
        text
    };
    let mut brace_depth = 0_i32;
    let mut bracket_depth = 0_i32;
    let bytes = literal_text.as_bytes();
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

/// Returns true if `prefix_before_quote` and `full_prefix_before_quote` satisfy the left prose
/// boundary rules.
fn has_valid_left_prose_boundary(
    prefix_before_quote: &str,
    full_prefix_before_quote: &str,
    strip_braces: bool,
) -> bool {
    let backtick_count = full_prefix_before_quote
        .bytes()
        .filter(|&byte| byte == b'`')
        .count();
    if backtick_count % 2 != 0
        || has_unclosed_structured_delimiter(full_prefix_before_quote, strip_braces)
    {
        return false;
    }

    let starts_at_beginning = prefix_before_quote.is_empty() && full_prefix_before_quote.is_empty();
    let preceded_by_space_or_paren = prefix_before_quote.ends_with([' ', '\t', '\n', '\r', '('])
        || prefix_before_quote.ends_with("\\n")
        || prefix_before_quote.ends_with("\\t")
        || prefix_before_quote.ends_with("\\r");
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

/// Returns true if `character` is a sentence punctuation mark allowed immediately after a closing
/// prose quote.
const fn is_prose_punctuation(character: char) -> bool {
    matches!(character, '.' | ',' | ';' | ':' | '!' | '?' | ')')
}

/// Returns true if `suffix_after_quote` satisfies the right prose boundary rules.
fn has_valid_right_prose_boundary(
    suffix_after_quote: &str,
    followed_by_interpolation: bool,
) -> bool {
    if suffix_after_quote.is_empty() {
        return !followed_by_interpolation;
    }
    if suffix_after_quote.starts_with("\\n")
        || suffix_after_quote.starts_with("\\t")
        || suffix_after_quote.starts_with("\\r")
    {
        return true;
    }
    let mut characters = suffix_after_quote.chars();
    let Some(first_char) = characters.next() else {
        return !followed_by_interpolation;
    };
    if first_char.is_ascii_whitespace() {
        return true;
    }
    if !is_prose_punctuation(first_char) {
        return false;
    }
    characters
        .next()
        .map_or(!followed_by_interpolation, |second_char| {
            second_char.is_ascii_whitespace()
                || is_prose_punctuation(second_char)
                || matches!(second_char, '\'' | '"')
        })
}

/// Checks whether a candidate placeholder surrounded by `neighbors` is quote-wrapped in a prose
/// context, returning the quote-wrapped expression if so.
fn match_prose_quoted_placeholder(
    neighbors: &PlaceholderNeighbors<'_>,
    placeholder_body: &str,
) -> Option<String> {
    let matched = extract_matching_quote_pair(neighbors.before, neighbors.after)?;
    let full_prefix_before_quote =
        &neighbors.full_before[..neighbors.full_before.len() - matched.opening.len()];
    if !has_valid_left_prose_boundary(
        matched.prefix_before,
        full_prefix_before_quote,
        neighbors.strip_brace_placeholders,
    ) {
        return None;
    }
    if !has_valid_right_prose_boundary(matched.suffix_after, neighbors.followed_by_interpolation) {
        return None;
    }
    Some(format!(
        "{open}{placeholder_body}{close}",
        open = matched.opening,
        close = matched.closing,
    ))
}

/// Returns true if `context_root` is the receiver of `.format(...)` / `.format_map(...)` or the
/// first positional argument of `str.format(...)`.
fn is_str_format_target(context_root: &RawNode<'_>) -> bool {
    let Some(parent) = context_root.parent() else {
        return false;
    };
    if parent.kind() == "attribute"
        && parent
            .field("object")
            .is_some_and(|object| object.range() == context_root.range())
        && parent
            .field("attribute")
            .is_some_and(|attribute| matches!(attribute.text().as_ref(), "format" | "format_map"))
        && parent.parent().is_some_and(|grandparent| {
            grandparent.kind() == "call"
                && grandparent
                    .field("function")
                    .is_some_and(|function| function.range() == parent.range())
        })
    {
        return true;
    }

    if parent.kind() == "argument_list"
        && let Some(call_node) = parent.parent()
        && call_node.kind() == "call"
        && call_node
            .field("function")
            .is_some_and(|function| function.text() == "str.format")
    {
        return positional_call_arguments(&parent)
            .first()
            .is_some_and(|first| first.range() == context_root.range());
    }

    false
}

/// Returns true if `context_root` is the left operand of `%` or the message argument of a
/// `logging` call that passes at least one trailing format argument.
fn is_printf_format_target(context_root: &RawNode<'_>) -> bool {
    let Some(parent) = context_root.parent() else {
        return false;
    };
    if parent.kind() == "binary_operator"
        && parent
            .field("operator")
            .is_some_and(|operator| operator.text() == "%")
        && parent
            .field("left")
            .is_some_and(|left| left.range() == context_root.range())
    {
        return true;
    }

    if parent.kind() != "argument_list" {
        return false;
    }
    let Some(call_node) = parent.parent() else {
        return false;
    };
    let Some(info) = extract_logger_call(&call_node) else {
        return false;
    };
    info.uses_printf
        && info.has_trailing_positional_args
        && info.message_node.range() == context_root.range()
}

/// Determines the active formatting context of a Python `string` node, returning `None` for
/// raw strings, byte strings, or unformatted string literals.
fn classify_string_format_context(string_node: &RawNode<'_>) -> Option<PythonFormatContext> {
    let opening = string_node.child(0)?;
    let opening_text = opening.text();
    let prefix = string_prefix_flags(&opening_text);
    if prefix.contains(['r', 'R', 'b', 'B']) {
        return None;
    }
    if prefix.contains(['f', 'F']) {
        return Some(PythonFormatContext::FString);
    }
    let context_root = outermost_string_expression(string_node);
    if is_str_format_target(&context_root) {
        Some(PythonFormatContext::StrFormat)
    } else if is_printf_format_target(&context_root) {
        Some(PythonFormatContext::Printf)
    } else {
        None
    }
}

/// Strips `.format()` (`{field}`) or `printf` (`%(key)s`) placeholder bodies from `text` so
/// placeholder identifiers and braces are not mistaken for prose words or literal containers.
fn strip_non_fstring_placeholders(text: &str, format_context: PythonFormatContext) -> String {
    match format_context {
        PythonFormatContext::FString => text.to_owned(),
        PythonFormatContext::StrFormat => strip_brace_placeholders(text),
        PythonFormatContext::Printf => {
            let bytes = text.as_bytes();
            let mut stripped = String::with_capacity(text.len());
            let mut cursor = 0;
            let mut index = 0;
            while index < bytes.len() {
                let current = bytes[index];
                if current == b'%' && bytes.get(index + 1) == Some(&b'%') {
                    index += 2;
                    continue;
                }
                if current == b'%'
                    && bytes.get(index + 1) == Some(&b'(')
                    && let Some(close_offset) = text[index + 2..].find(')')
                {
                    stripped.push_str(&text[cursor..index]);
                    let after_paren = index + 2 + close_offset + 1;
                    let next_index = text[after_paren..]
                        .chars()
                        .next()
                        .map_or(after_paren, |conv| after_paren + conv.len_utf8());
                    index = next_index;
                    cursor = index;
                    continue;
                }
                index += 1;
            }
            stripped.push_str(&text[cursor..]);
            stripped
        }
    }
}

/// Returns the combined literal text of `string_node` (or all sibling `string` parts when
/// enclosed in a `concatenated_string`), with format placeholders excluded.
fn combined_message_literal_text(
    string_node: &RawNode<'_>,
    format_context: PythonFormatContext,
) -> String {
    let mut combined = String::new();
    if let Some(parent) = string_node
        .parent()
        .filter(|node| node.kind() == "concatenated_string")
    {
        for child in parent.children().filter(|child| child.kind() == "string") {
            append_string_literal_segments(&child, &mut combined);
        }
    } else {
        append_string_literal_segments(string_node, &mut combined);
    }
    strip_non_fstring_placeholders(&combined, format_context)
}

/// Returns the bare expression text inside an f-string `interpolation` node if it has no
/// `type_conversion` (`!r`, `!s`, `!a`), no `format_specifier` (`:...`), no debug `=`, and is not
/// a nested string literal.
fn bare_fstring_interpolation_expression(interpolation: &RawNode<'_>) -> Option<String> {
    if interpolation.field("type_conversion").is_some()
        || interpolation.field("format_specifier").is_some()
        || interpolation.children().any(|child| child.kind() == "=")
    {
        return None;
    }
    let expression = interpolation.field("expression")?;
    if expression.kind() == "string" {
        return None;
    }
    let text = expression.text().trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// Collects quote-wrapped placeholders inside an f-string `string` node.
fn collect_fstring_quote_wrapped<'a>(
    string_node: &RawNode<'a>,
    out: &mut Vec<PythonQuoteWrappedPlaceholder<'a>>,
) {
    let Some((segments, interpolations)) = fstring_segments_and_interpolations(string_node) else {
        return;
    };
    if interpolations.is_empty() {
        return;
    }

    let mut cumulative_before = preceding_concatenated_literal_text(string_node);
    for (index, interpolation) in interpolations.iter().enumerate() {
        let segment_before = segments[index].as_str();
        let after = segments[index + 1].as_str();
        cumulative_before.push_str(segment_before);
        let before = if index == 0 {
            cumulative_before.as_str()
        } else {
            segment_before
        };
        let Some(inner_expression) = bare_fstring_interpolation_expression(interpolation) else {
            continue;
        };
        let neighbors = PlaceholderNeighbors {
            before,
            full_before: &cumulative_before,
            after,
            followed_by_interpolation: index + 1 < interpolations.len(),
            strip_brace_placeholders: false,
        };
        let placeholder_body = format!("{{{inner_expression}}}");
        if let Some(expression) = match_prose_quoted_placeholder(&neighbors, &placeholder_body) {
            out.push(PythonQuoteWrappedPlaceholder {
                node: AstNode::from_raw(string_node.clone()),
                expression,
                replacement: format!("{{{inner_expression}!r}}"),
            });
        }
    }
}

/// Evaluates a single `{...}` span inside a `.format()` string and returns a
/// [`PythonQuoteWrappedPlaceholder`] when it is a bare field wrapped in prose quotes.
fn evaluate_str_format_brace_span<'a>(
    string_node: &RawNode<'a>,
    concatenated_prefix: &str,
    content: &str,
    open_index: usize,
    close_index: usize,
) -> Option<PythonQuoteWrappedPlaceholder<'a>> {
    let field = &content[open_index + 1..close_index];
    if field.contains(['!', ':', '{', '}']) || extract_valid_field_root(field).is_none() {
        return None;
    }
    let full_before = format!("{concatenated_prefix}{}", &content[..open_index]);
    let after = &content[close_index + 1..];
    let neighbors = PlaceholderNeighbors {
        before: &full_before,
        full_before: &full_before,
        after,
        followed_by_interpolation: false,
        strip_brace_placeholders: true,
    };
    let placeholder_body = format!("{{{field}}}");
    let expression = match_prose_quoted_placeholder(&neighbors, &placeholder_body)?;
    Some(PythonQuoteWrappedPlaceholder {
        node: AstNode::from_raw(string_node.clone()),
        expression,
        replacement: format!("{{{field}!r}}"),
    })
}

/// Collects quote-wrapped placeholders inside a `.format()` or `.format_map()` string literal.
fn collect_str_format_quote_wrapped<'a>(
    string_node: &RawNode<'a>,
    content: &str,
    out: &mut Vec<PythonQuoteWrappedPlaceholder<'a>>,
) {
    let concatenated_prefix = preceding_concatenated_literal_text(string_node);
    let bytes = content.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let current = bytes[index];
        let is_escaped = (current == b'{' && bytes.get(index + 1) == Some(&b'{'))
            || (current == b'}' && bytes.get(index + 1) == Some(&b'}'));
        if is_escaped {
            index += 2;
            continue;
        }
        if current == b'{'
            && let Some(close_index) = find_brace_placeholder_end(bytes, index)
        {
            if let Some(finding) = evaluate_str_format_brace_span(
                string_node,
                &concatenated_prefix,
                content,
                index,
                close_index,
            ) {
                out.push(finding);
            }
            index = close_index + 1;
            continue;
        }
        index += 1;
    }
}

/// Parses a bare `%s` or `%(name)s` printf placeholder starting at `&content[percent_index..]`,
/// returning `(end_index, raw_specifier, replacement)`.
fn parse_bare_printf_s_placeholder(
    content: &str,
    percent_index: usize,
) -> Option<(usize, &str, String)> {
    let rest = &content[percent_index + 1..];
    if rest.starts_with('s') {
        let end_index = percent_index + 2;
        return Some((
            end_index,
            &content[percent_index..end_index],
            "%r".to_owned(),
        ));
    }
    let after_open_paren = rest.strip_prefix('(')?;
    let close_offset = after_open_paren.find(")s")?;
    let key = &after_open_paren[..close_offset];
    let is_valid_key = !key.is_empty()
        && key
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_');
    if !is_valid_key {
        return None;
    }
    let end_index = percent_index + 2 + close_offset + 2;
    let raw_specifier = &content[percent_index..end_index];
    Some((end_index, raw_specifier, format!("%({key})r")))
}

/// Collects quote-wrapped `%s` and `%(name)s` placeholders inside a printf-formatted string.
fn collect_printf_quote_wrapped<'a>(
    string_node: &RawNode<'a>,
    content: &str,
    out: &mut Vec<PythonQuoteWrappedPlaceholder<'a>>,
) {
    let concatenated_prefix = preceding_concatenated_literal_text(string_node);
    let bytes = content.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let current = bytes[index];
        if current == b'%' && bytes.get(index + 1) == Some(&b'%') {
            index += 2;
            continue;
        }
        if current == b'%'
            && let Some((end_index, raw_specifier, replacement)) =
                parse_bare_printf_s_placeholder(content, index)
        {
            let full_before = format!("{concatenated_prefix}{}", &content[..index]);
            let after = &content[end_index..];
            let neighbors = PlaceholderNeighbors {
                before: &full_before,
                full_before: &full_before,
                after,
                followed_by_interpolation: false,
                strip_brace_placeholders: false,
            };
            if let Some(expression) = match_prose_quoted_placeholder(&neighbors, raw_specifier) {
                out.push(PythonQuoteWrappedPlaceholder {
                    node: AstNode::from_raw(string_node.clone()),
                    expression,
                    replacement,
                });
            }
            index = end_index;
            continue;
        }
        index += 1;
    }
}

/// Collects quote-wrapped placeholders in Python f-strings, `.format()` / `.format_map()`
/// calls, `%`-formatted strings, and multi-argument `logging` calls in `file`, in source order.
#[must_use]
pub fn collect_quote_wrapped_placeholders(
    file: &ParsedFile,
) -> Vec<PythonQuoteWrappedPlaceholder<'_>> {
    let mut out = Vec::new();
    for node in file.grep.root().dfs() {
        if node.kind() != "string" {
            continue;
        }
        let Some(format_context) = classify_string_format_context(&node) else {
            continue;
        };
        let combined_literal = combined_message_literal_text(&node, format_context);
        if !is_prose_message_text(&combined_literal) {
            continue;
        }
        match format_context {
            PythonFormatContext::FString => {
                collect_fstring_quote_wrapped(&node, &mut out);
            }
            PythonFormatContext::StrFormat => {
                let (_, content) = delimited_string_parts(&node);
                collect_str_format_quote_wrapped(&node, &content, &mut out);
            }
            PythonFormatContext::Printf => {
                let (_, content) = delimited_string_parts(&node);
                collect_printf_quote_wrapped(&node, &content, &mut out);
            }
        }
    }
    out
}
