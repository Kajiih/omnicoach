//! Python format strings: f-strings, `str.format` templates and `%` (printf-style) templates,
//! split into literal text and replacement fields, plus PEP 3101 field-name parsing.

// omni:disable-file [repeated-literal] -- Tree-sitter node kinds and field names (see ROADMAP)

use super::{
    AstNode, ParsedFile, RawNode, delimited_string_parts, extract_logger_call,
    fstring_segments_and_interpolations, outermost_string_expression, positional_call_arguments,
    preceding_concatenated_literal_text, string_prefix_flags,
};

/// `%` conversion types accepted by printf-style formatting.
const PRINTF_CONVERSIONS: &[u8] = b"diouxXeEfFgGcrsa";

/// How a Python string literal is formatted at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PythonFormatStyle {
    /// An `f` / `F` prefixed literal (`f"... {x} ..."`).
    FString,
    /// The template of `.format(...)`, `.format_map(...)` or `str.format(...)`.
    StrFormat,
    /// The left operand of `%`, or a logger message followed by format arguments.
    Printf,
}

/// One replacement field of a format string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonFormatPlaceholder {
    /// The field as written in source (`{name!r:>8}`, `%(key)s`, `%5d`).
    pub text: String,
    /// The f-string expression, `str.format` field name or `%` mapping key; empty when absent
    /// (`{}`, `%s`).
    pub field: String,
    /// The conversion after `!` in braces (`r`, `s`, `a`), or the `%` conversion type (`s`, `d`).
    pub conversion: Option<String>,
    /// The format spec after `:` in braces, or the `%` flags, width, precision and length.
    pub format_spec: Option<String>,
    /// Whether an f-string field is self-documenting (`{value=}`).
    pub is_self_documenting: bool,
}

/// A formatted Python `string` literal, split into literal text and replacement fields.
///
/// All text is source text: escape sequences are not decoded.
pub struct PythonFormatString<'a> {
    /// The `string` node.
    pub node: AstNode<'a>,
    /// How the literal is formatted.
    pub style: PythonFormatStyle,
    /// Literal text of the strings before `node` in an implicit concatenation
    /// (`"Failed for " f"'{name}'"`), without f-string fields.
    pub preceding_text: String,
    /// Literal text around the fields of `node`: `literals[i]` precedes `placeholders[i]` and
    /// the last entry follows the last field, so there is one more literal than placeholders.
    pub literals: Vec<String>,
    /// The replacement fields of `node`, in source order.
    pub placeholders: Vec<PythonFormatPlaceholder>,
}

/// Collects the formatted string literals in `file`, in source order. Raw, byte and
/// unformatted string literals are skipped.
#[must_use]
pub fn collect_format_strings(file: &ParsedFile) -> Vec<PythonFormatString<'_>> {
    file.grep
        .root()
        .dfs()
        .filter(|node| node.kind() == "string")
        .filter_map(|node| format_string(&node))
        .collect()
}

fn format_string<'a>(string_node: &RawNode<'a>) -> Option<PythonFormatString<'a>> {
    let style = format_style(string_node)?;
    let (literals, placeholders) = match style {
        PythonFormatStyle::FString => {
            let (literals, interpolations) = fstring_segments_and_interpolations(string_node)?;
            (
                literals,
                interpolations.iter().map(fstring_placeholder).collect(),
            )
        }
        PythonFormatStyle::StrFormat => split_brace_fields(&delimited_string_parts(string_node).1),
        PythonFormatStyle::Printf => split_printf_fields(&delimited_string_parts(string_node).1),
    };
    Some(PythonFormatString {
        node: AstNode::from_raw(string_node.clone()),
        style,
        preceding_text: preceding_concatenated_literal_text(string_node),
        literals,
        placeholders,
    })
}

/// Returns how `string_node` is formatted, or `None` for raw, byte and unformatted literals.
fn format_style(string_node: &RawNode<'_>) -> Option<PythonFormatStyle> {
    let opening = string_node.child(0)?;
    let opening_text = opening.text();
    let prefix = string_prefix_flags(&opening_text);
    if prefix.contains(['r', 'R', 'b', 'B']) {
        return None;
    }
    if prefix.contains(['f', 'F']) {
        return Some(PythonFormatStyle::FString);
    }
    let context_root = outermost_string_expression(string_node);
    if is_str_format_target(&context_root) {
        Some(PythonFormatStyle::StrFormat)
    } else if is_printf_format_target(&context_root) {
        Some(PythonFormatStyle::Printf)
    } else {
        None
    }
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
/// logger call that `%`-formats it with at least one trailing argument.
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
    let Some(call) = extract_logger_call(&call_node) else {
        return false;
    };
    call.uses_printf
        && call.has_trailing_positional_args
        && call.message_node.range() == context_root.range()
}

/// Describes an f-string `interpolation` node.
fn fstring_placeholder(interpolation: &RawNode<'_>) -> PythonFormatPlaceholder {
    PythonFormatPlaceholder {
        text: interpolation.text().into_owned(),
        field: interpolation
            .field("expression")
            .map(|expression| expression.text().trim().to_owned())
            .unwrap_or_default(),
        conversion: interpolation.field("type_conversion").map(|conversion| {
            let text = conversion.text();
            text.strip_prefix('!').unwrap_or(&text).to_owned()
        }),
        format_spec: interpolation.field("format_specifier").map(|specifier| {
            let text = specifier.text();
            text.strip_prefix(':').unwrap_or(&text).to_owned()
        }),
        is_self_documenting: interpolation.children().any(|child| child.kind() == "="),
    }
}

/// Finds the closing `}` byte offset of a `{...}` field opening at `open_index`, returning
/// `None` if an inner `{` comes first.
fn find_brace_field_end(bytes: &[u8], open_index: usize) -> Option<usize> {
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

/// Describes a `{field!conversion:format_spec}` field written as `text`.
///
/// The field name ends at the first `:`, so a `:` inside an index (`{a[:]}`) starts the spec.
fn brace_placeholder(text: &str) -> PythonFormatPlaceholder {
    let inner = &text[1..text.len() - 1];
    let (header, format_spec) = match inner.split_once(':') {
        Some((header, format_spec)) => (header, Some(format_spec.to_owned())),
        None => (inner, None),
    };
    let (field, conversion) = match header.split_once('!') {
        Some((field, conversion)) => (field, Some(conversion.to_owned())),
        None => (header, None),
    };
    PythonFormatPlaceholder {
        text: text.to_owned(),
        field: field.to_owned(),
        conversion,
        format_spec,
        is_self_documenting: false,
    }
}

/// Splits `str.format` template text into literals and `{...}` fields. Escaped `{{` and `}}`
/// stay in the literals.
fn split_brace_fields(content: &str) -> (Vec<String>, Vec<PythonFormatPlaceholder>) {
    let bytes = content.as_bytes();
    let mut literals = Vec::new();
    let mut placeholders = Vec::new();
    let mut literal_start = 0;
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
            && let Some(close_index) = find_brace_field_end(bytes, index)
        {
            literals.push(content[literal_start..index].to_owned());
            placeholders.push(brace_placeholder(&content[index..=close_index]));
            index = close_index + 1;
            literal_start = index;
            continue;
        }
        index += 1;
    }
    literals.push(content[literal_start..].to_owned());
    (literals, placeholders)
}

/// Advances `cursor` past a printf width or precision (`*` or digits).
fn skip_printf_count(bytes: &[u8], cursor: &mut usize) {
    if bytes.get(*cursor) == Some(&b'*') {
        *cursor += 1;
        return;
    }
    while bytes.get(*cursor).is_some_and(u8::is_ascii_digit) {
        *cursor += 1;
    }
}

/// Parses the `%[(key)][flags][width][.precision][length]type` field starting at
/// `percent_index`, returning the index after it.
fn parse_printf_field(
    content: &str,
    percent_index: usize,
) -> Option<(usize, PythonFormatPlaceholder)> {
    let bytes = content.as_bytes();
    let mut cursor = percent_index + 1;
    let mut field = "";
    if bytes.get(cursor) == Some(&b'(') {
        let close_offset = content[cursor + 1..].find(')')?;
        field = &content[cursor + 1..cursor + 1 + close_offset];
        cursor += close_offset + 2;
    }
    let spec_start = cursor;
    while bytes
        .get(cursor)
        .is_some_and(|byte| b"#0- +".contains(byte))
    {
        cursor += 1;
    }
    skip_printf_count(bytes, &mut cursor);
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        skip_printf_count(bytes, &mut cursor);
    }
    while bytes.get(cursor).is_some_and(|byte| b"hlL".contains(byte)) {
        cursor += 1;
    }
    let conversion = *bytes.get(cursor)?;
    if !PRINTF_CONVERSIONS.contains(&conversion) {
        return None;
    }
    let end = cursor + 1;
    Some((
        end,
        PythonFormatPlaceholder {
            text: content[percent_index..end].to_owned(),
            field: field.to_owned(),
            conversion: Some(char::from(conversion).to_string()),
            format_spec: (cursor > spec_start).then(|| content[spec_start..cursor].to_owned()),
            is_self_documenting: false,
        },
    ))
}

/// Splits printf-style template text into literals and `%` fields. Escaped `%%` and `%`
/// signs that start no valid field stay in the literals.
fn split_printf_fields(content: &str) -> (Vec<String>, Vec<PythonFormatPlaceholder>) {
    let bytes = content.as_bytes();
    let mut literals = Vec::new();
    let mut placeholders = Vec::new();
    let mut literal_start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        if bytes.get(index + 1) == Some(&b'%') {
            index += 2;
            continue;
        }
        if let Some((end, placeholder)) = parse_printf_field(content, index) {
            literals.push(content[literal_start..index].to_owned());
            placeholders.push(placeholder);
            index = end;
            literal_start = index;
            continue;
        }
        index += 1;
    }
    literals.push(content[literal_start..].to_owned());
    (literals, placeholders)
}

/// Returns true if `name` is a valid Python identifier (`order_id`, `_item2`, `café`).
fn is_python_identifier(name: &str) -> bool {
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    let valid_start = first == '_' || first.is_alphabetic();
    valid_start
        && characters.all(|character| character == '_' || character.is_alphanumeric())
        && !first.is_ascii_digit()
}

/// Validates a PEP 3101 `field_name` (`arg_name(\".\" attribute | \"[\" index \"]\")*`) and returns
/// its root `arg_name` slice.
#[must_use]
pub fn extract_valid_field_root(field_name: &str) -> Option<&str> {
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
fn parse_replacement_field(message: &str, start: usize) -> Option<(usize, Vec<&str>)> {
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

/// Returns the identifier roots of the named PEP 3101 replacement fields in `message`.
///
/// Roots come in source order (`order` for `{order.id}`, `width` for `{:>{width}}`); the result
/// is `None` if `message` has unbalanced braces. Positional fields (`{}`, `{0}`) have no named root.
#[must_use]
pub fn named_format_field_roots(message: &str) -> Option<Vec<String>> {
    let mut cursor = 0;
    let mut roots = Vec::new();
    while cursor < message.len() {
        let rest = &message[cursor..];
        if rest.starts_with("{{") || rest.starts_with("}}") {
            cursor += 2;
        } else if rest.starts_with('}') {
            return None;
        } else if rest.starts_with('{') {
            let (next_cursor, field_roots) = parse_replacement_field(message, cursor + 1)?;
            roots.extend(
                field_roots
                    .into_iter()
                    .filter(|root| is_python_identifier(root))
                    .map(str::to_owned),
            );
            cursor = next_cursor;
        } else {
            cursor += rest.chars().next()?.len_utf8();
        }
    }
    Some(roots)
}
