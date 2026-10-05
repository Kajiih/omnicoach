//! Python string literal prefix, concatenation, segment extraction, and PEP 3101 field parsing.

// omni:disable-file [repeated-literal] -- Tree-sitter node kinds and field names (see ROADMAP)

use crate::code_lint::ast::{RawNode, delimited_string_parts};
use std::collections::HashSet;

/// Splits the prefix flags (`f`, `r`, `b`, `u`, `t`) from a `string_start` delimiter token.
pub(super) fn string_prefix_flags(opening_delimiter: &str) -> &str {
    opening_delimiter
        .find(['\'', '"'])
        .map_or("", |quote_index| &opening_delimiter[..quote_index])
}

/// Checks if a Python string node is triple-quoted (`"""` or `'''`), ignoring any prefix flags.
pub(super) fn is_triple_quoted(node: &RawNode<'_>) -> bool {
    let text = node.text();
    let prefix = string_prefix_flags(&text);
    let unprefixed = &text[prefix.len()..];
    unprefixed.starts_with("\"\"\"") || unprefixed.starts_with("'''")
}

/// Walks upward from a `string` node through enclosing `concatenated_string` and
/// `parenthesized_expression` nodes to find the expression node bound to its surrounding context.
pub(super) fn outermost_string_expression<'a>(string_node: &RawNode<'a>) -> RawNode<'a> {
    let mut current = string_node.clone();
    while let Some(parent) = current.parent() {
        let is_wrapper = matches!(
            parent.kind().as_ref(),
            "concatenated_string" | "parenthesized_expression"
        );
        if !is_wrapper {
            break;
        }
        current = parent;
    }
    current
}

/// Returns the positional (non-keyword, non-splat) argument nodes of an `argument_list`.
pub(super) fn positional_call_arguments<'a>(arguments: &RawNode<'a>) -> Vec<RawNode<'a>> {
    arguments
        .children()
        .filter(|child| {
            child.is_named()
                && !child.is_extra()
                && !matches!(
                    child.kind().as_ref(),
                    "keyword_argument" | "list_splat" | "dictionary_splat"
                )
        })
        .collect()
}

/// Appends the literal text segments (excluding `{...}` interpolations) of a single `string` node
/// to `buffer`, using node-relative byte offsets into `string_node.text()`.
pub(super) fn append_string_literal_segments(string_node: &RawNode<'_>, buffer: &mut String) {
    let Some((segments, _)) = fstring_segments_and_interpolations(string_node) else {
        return;
    };
    for segment in segments {
        buffer.push_str(&segment);
    }
}

/// Splits a `string` node into its literal text segments (between delimiters and `interpolation`
/// children) and its `interpolation` child nodes, using node-relative byte offsets so leading
/// file whitespace never shifts slice indices.
pub(super) fn fstring_segments_and_interpolations<'a>(
    string_node: &RawNode<'a>,
) -> Option<(Vec<String>, Vec<RawNode<'a>>)> {
    let opening = string_node.child(0)?;
    let closing = string_node.children().last()?;
    let node_text = string_node.text();
    let base_offset = string_node.range().start;
    let content_start = opening.range().end.saturating_sub(base_offset);
    let content_end = closing.range().start.saturating_sub(base_offset);
    if content_start > content_end || content_end > node_text.len() {
        return None;
    }

    let interpolations: Vec<RawNode<'a>> = string_node
        .children()
        .filter(|child| child.kind() == "interpolation")
        .collect();

    let mut segments = Vec::with_capacity(interpolations.len() + 1);
    let mut cursor = content_start;
    for interpolation in &interpolations {
        let range = interpolation.range();
        let relative_start = range.start.saturating_sub(base_offset);
        let relative_end = range.end.saturating_sub(base_offset);
        segments.push(
            node_text
                .get(cursor..relative_start)
                .unwrap_or_default()
                .to_owned(),
        );
        cursor = relative_end;
    }
    segments.push(
        node_text
            .get(cursor..content_end)
            .unwrap_or_default()
            .to_owned(),
    );
    Some((segments, interpolations))
}

/// Returns the combined literal text of all sibling `string` nodes preceding `string_node` in an
/// enclosing `concatenated_string`.
pub(super) fn preceding_concatenated_literal_text(string_node: &RawNode<'_>) -> String {
    let mut prefix = String::new();
    if let Some(parent) = string_node
        .parent()
        .filter(|node| node.kind() == "concatenated_string")
    {
        for child in parent.children().filter(|child| child.kind() == "string") {
            if child.range().start >= string_node.range().start {
                break;
            }
            append_string_literal_segments(&child, &mut prefix);
        }
    }
    prefix
}

/// Replaces unescaped `\N{...}` named Unicode character escapes with a space in non-raw strings
/// so `{...}` inside `\N{NAME}` is not mistaken for a PEP 3101 format field.
fn strip_named_unicode_escapes(content: &str) -> String {
    let mut cleaned = String::with_capacity(content.len());
    let mut cursor = 0;
    while cursor < content.len() {
        let rest = &content[cursor..];
        if rest.starts_with(r"\\") {
            cleaned.push_str(r"\\");
            cursor += 2;
        } else if let Some(after_prefix) = rest.strip_prefix(r"\N{")
            && let Some(close_offset) = after_prefix.find('}')
            && !after_prefix[..close_offset].contains('{')
        {
            cleaned.push(' ');
            cursor += 3 + close_offset + 1;
        } else if let Some(character) = rest.chars().next() {
            cleaned.push(character);
            cursor += character.len_utf8();
        } else {
            break;
        }
    }
    cleaned
}

/// Returns the inner text of a single plain Python `string` node (with `\N{...}` escapes stripped
/// in non-raw strings), excluding f-strings, byte strings, and template strings.
pub(super) fn extract_plain_string_node(node: &RawNode<'_>) -> Option<String> {
    if node.kind() != "string" || node.children().any(|child| child.kind() == "interpolation") {
        return None;
    }
    let (opening, content) = delimited_string_parts(node);
    let prefix = string_prefix_flags(&opening);
    if prefix.contains(['f', 'F', 'b', 'B', 't', 'T']) {
        return None;
    }
    if prefix.contains(['r', 'R']) {
        Some(content)
    } else {
        Some(strip_named_unicode_escapes(&content))
    }
}

/// Extracts the static text of a plain string literal or an implicit `concatenated_string` of
/// plain string literals.
pub(super) fn extract_logger_message_literal(node: &RawNode<'_>) -> Option<String> {
    match node.kind().as_ref() {
        "string" => extract_plain_string_node(node),
        "concatenated_string" => {
            let mut combined = String::new();
            for child in node
                .children()
                .filter(|child| child.is_named() && !child.is_extra())
            {
                combined.push_str(&extract_plain_string_node(&child)?);
            }
            (!combined.is_empty()).then_some(combined)
        }
        _ => None,
    }
}

/// Returns true if `name` is a valid Python identifier (`order_id`, `_item2`, `café`).
pub(super) fn is_python_identifier(name: &str) -> bool {
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    let valid_start = first == '_' || first.is_alphabetic();
    valid_start
        && characters.all(|character| character == '_' || character.is_alphanumeric())
        && !first.is_ascii_digit()
}

/// Validates a PEP 3101 `field_name` (`arg_name("." attribute | "[" index "]")*`) and returns
/// its root `arg_name` slice.
pub(super) fn extract_valid_field_root(field_name: &str) -> Option<&str> {
    let split_at = field_name.find(['.', '[']).unwrap_or(field_name.len());
    let root = &field_name[..split_at];
    let valid_root = root.is_empty()
        || root.chars().all(|character| character.is_ascii_digit())
        || is_python_identifier(root);
    if !valid_root {
        return None;
    }

    let mut tail = &field_name[split_at..];
    while !tail.is_empty() {
        if let Some(after_dot) = tail.strip_prefix('.') {
            let end = after_dot.find(['.', '[']).unwrap_or(after_dot.len());
            if !is_python_identifier(&after_dot[..end]) {
                return None;
            }
            tail = &after_dot[end..];
        } else if let Some(after_bracket) = tail.strip_prefix('[') {
            let close = after_bracket.find(']')?;
            if close == 0 {
                return None;
            }
            tail = &after_bracket[close + 1..];
        } else {
            return None;
        }
    }
    Some(root)
}

/// Parses the `:format_spec` portion of a PEP 3101 replacement field starting at `start`,
/// appending any nested `{nested_field}` root names to `roots` and returning the byte index
/// immediately after the outer closing `}`.
fn parse_format_spec_section<'a>(
    message: &'a str,
    start: usize,
    roots: &mut Vec<&'a str>,
) -> Option<usize> {
    let mut cursor = start;
    while cursor < message.len() {
        let rest = &message[cursor..];
        if rest.starts_with('}') {
            return Some(cursor + 1);
        }
        if rest.starts_with('{') {
            let after_open = &message[cursor + 1..];
            let close_offset = after_open.find('}')?;
            let nested_body = &after_open[..close_offset];
            if nested_body.contains('{') {
                return None;
            }
            let nested_field = nested_body
                .split_once('!')
                .map_or(nested_body, |(before, _)| before);
            if let Some(nested_root) = extract_valid_field_root(nested_field) {
                roots.push(nested_root);
            }
            cursor += 1 + close_offset + 1;
        } else {
            cursor += rest.chars().next()?.len_utf8();
        }
    }
    None
}

/// Parses one `{...}` replacement field whose body starts at `start` (immediately after `{`),
/// returning the byte index after the matching `}` and the root `arg_name`s found inside it.
pub(super) fn parse_replacement_field(message: &str, start: usize) -> Option<(usize, Vec<&str>)> {
    let mut cursor = start;
    let mut in_brackets = false;
    let mut delimiter = None;

    while cursor < message.len() {
        let character = message[cursor..].chars().next()?;
        match character {
            '[' if !in_brackets => in_brackets = true,
            ']' if in_brackets => in_brackets = false,
            '{' if !in_brackets => return None,
            ':' | '}' if !in_brackets => {
                delimiter = Some((cursor, character));
                break;
            }
            _ => {}
        }
        cursor += character.len_utf8();
    }

    let (delimiter_index, delimiter_char) = delimiter?;
    let header = &message[start..delimiter_index];
    let field_name = match header.split_once('!') {
        Some((before, "r" | "s" | "a")) => Some(before),
        Some(_) => None,
        None => Some(header),
    };

    let mut roots = Vec::new();
    if let Some(root) = field_name.and_then(extract_valid_field_root) {
        roots.push(root);
    }

    if delimiter_char == '}' {
        return Some((delimiter_index + 1, roots));
    }
    let next_cursor = parse_format_spec_section(message, delimiter_index + 1, &mut roots)?;
    Some((next_cursor, roots))
}

/// Returns the first named PEP 3101 placeholder root identifier in `message` that is not
/// present in `keyword_names`, or `None` if `message` has unbalanced braces or all named
/// placeholders are satisfied.
pub(super) fn first_unmatched_named_placeholder(
    message: &str,
    keyword_names: &HashSet<String>,
) -> Option<String> {
    let mut cursor = 0;
    let mut first_unmatched: Option<String> = None;

    while cursor < message.len() {
        let rest = &message[cursor..];
        if rest.starts_with("{{") || rest.starts_with("}}") {
            cursor += 2;
        } else if rest.starts_with('}') {
            return None;
        } else if rest.starts_with('{') {
            let (next_cursor, roots) = parse_replacement_field(message, cursor + 1)?;
            for root in roots {
                if first_unmatched.is_none()
                    && is_python_identifier(root)
                    && !keyword_names.contains(root)
                {
                    first_unmatched = Some(root.to_owned());
                }
            }
            cursor = next_cursor;
        } else {
            cursor += rest.chars().next()?.len_utf8();
        }
    }
    first_unmatched
}
