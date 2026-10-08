//! Python format strings: f-strings, `str.format` templates and `%` (printf-style) templates,
//! split into literal text and replacement fields, plus PEP 3101 field-name parsing.

use super::{AstNode, ParsedFile, extract_logger_call, fstring_segments_and_interpolations};
use crate::code_lint::ast::span_from_ruff_range;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_arguments, walk_expr};
use ruff_python_ast::{Expr, FStringPartRef, InterpolatedElement, Operator, StringLiteral};
use ruff_text_size::{Ranged as _, TextRange};

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
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = FormatStringVisitor {
        file,
        context_style: None,
        call_arg_target: None,
        out: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.out
}

struct FormatStringVisitor<'a> {
    file: &'a ParsedFile,
    context_style: Option<PythonFormatStyle>,
    call_arg_target: Option<(TextRange, PythonFormatStyle)>,
    out: Vec<PythonFormatString<'a>>,
}

impl FormatStringVisitor<'_> {
    fn visit_fstring(
        &mut self,
        expr_fstring: &ruff_python_ast::ExprFString,
        current_style: Option<PythonFormatStyle>,
    ) {
        let mut preceding_text = String::new();
        for part in &expr_fstring.value {
            match part {
                FStringPartRef::Literal(lit) => {
                    if let Some(style) = current_style {
                        self.push_plain_part(lit, style, &mut preceding_text);
                    } else {
                        let content_range = lit.content_range();
                        preceding_text.push_str(
                            &self.file.source[usize::from(content_range.start())
                                ..usize::from(content_range.end())],
                        );
                    }
                }
                FStringPartRef::FString(fstring) => {
                    let (literals, interpolations) =
                        fstring_segments_and_interpolations(fstring, &self.file.source);
                    if !fstring.flags.prefix().is_raw() {
                        let placeholders = interpolations
                            .iter()
                            .map(|interp| fstring_placeholder(interp, &self.file.source))
                            .collect();
                        self.out.push(PythonFormatString {
                            node: AstNode::from_span(
                                self.file,
                                span_from_ruff_range(fstring.range),
                            ),
                            style: PythonFormatStyle::FString,
                            preceding_text: preceding_text.clone(),
                            literals: literals.clone(),
                            placeholders,
                        });
                    }
                    for segment in &literals {
                        preceding_text.push_str(segment);
                    }
                    for interpolation in interpolations {
                        self.visit_expr(&interpolation.expression);
                        if let Some(spec) = &interpolation.format_spec {
                            for element in &spec.elements {
                                self.visit_interpolated_string_element(element);
                            }
                        }
                    }
                }
            }
        }
    }

    fn visit_call(&mut self, call: &ruff_python_ast::ExprCall) {
        if let Expr::Attribute(attr) = call.func.as_ref()
            && matches!(attr.attr.as_str(), "format" | "format_map")
        {
            self.context_style = Some(PythonFormatStyle::StrFormat);
            self.visit_expr(&attr.value);
            self.context_style = None;
        } else {
            self.visit_expr(&call.func);
        }

        let func_span = span_from_ruff_range(call.func.range());
        let func_text = &self.file.source[func_span.start..func_span.end];
        let target = if func_text == "str.format" {
            call.arguments
                .args
                .iter()
                .find(|arg| !matches!(arg, Expr::Starred(_)))
                .map(|arg| (arg.range(), PythonFormatStyle::StrFormat))
        } else {
            extract_logger_call(call, self.file)
                .filter(|lc| lc.uses_printf && lc.has_trailing_positional_args)
                .map(|lc| (lc.message_range, PythonFormatStyle::Printf))
        };

        let prev_target = self.call_arg_target;
        self.call_arg_target = target;
        walk_arguments(self, &call.arguments);
        self.call_arg_target = prev_target;
    }

    fn push_plain_part(
        &mut self,
        part: &StringLiteral,
        style: PythonFormatStyle,
        preceding_text: &mut String,
    ) {
        let content_range = part.content_range();
        let content =
            &self.file.source[usize::from(content_range.start())..usize::from(content_range.end())];
        if !part.flags.prefix().is_raw() {
            let (literals, placeholders) = match style {
                PythonFormatStyle::StrFormat => split_brace_fields(content),
                PythonFormatStyle::Printf => split_printf_fields(content),
                PythonFormatStyle::FString => unreachable!(),
            };
            self.out.push(PythonFormatString {
                node: AstNode::from_span(self.file, span_from_ruff_range(part.range)),
                style,
                preceding_text: preceding_text.clone(),
                literals,
                placeholders,
            });
        }
        preceding_text.push_str(content);
    }
}

impl SourceOrderVisitor<'_> for FormatStringVisitor<'_> {
    fn visit_expr(&mut self, expr: &Expr) {
        let current_style = self.context_style.take().or_else(|| {
            self.call_arg_target
                .filter(|(range, _)| *range == expr.range())
                .map(|(_, style)| style)
        });

        match expr {
            Expr::StringLiteral(string_literal) => {
                if let Some(style) = current_style {
                    let mut preceding_text = String::new();
                    for part in string_literal.value.as_slice() {
                        self.push_plain_part(part, style, &mut preceding_text);
                    }
                }
            }
            Expr::FString(expr_fstring) => self.visit_fstring(expr_fstring, current_style),
            Expr::Call(call) => self.visit_call(call),
            Expr::BinOp(bin_op) => {
                self.context_style =
                    (bin_op.op == Operator::Mod).then_some(PythonFormatStyle::Printf);
                self.visit_expr(&bin_op.left);
                self.context_style = None;
                self.visit_expr(&bin_op.right);
            }
            _ => {
                walk_expr(self, expr);
            }
        }
    }
}

/// Describes an f-string `InterpolatedElement` node.
fn fstring_placeholder(
    interpolation: &InterpolatedElement,
    source: &str,
) -> PythonFormatPlaceholder {
    let text_span = span_from_ruff_range(interpolation.range);
    let expr_span = span_from_ruff_range(interpolation.expression.range());
    PythonFormatPlaceholder {
        text: source[text_span.start..text_span.end].to_owned(),
        field: source[expr_span.start..expr_span.end].trim().to_owned(),
        conversion: interpolation.conversion.to_char().map(String::from),
        format_spec: interpolation.format_spec.as_ref().map(|spec| {
            let spec_span = span_from_ruff_range(spec.range);
            let spec_text = &source[spec_span.start..spec_span.end];
            spec_text.strip_prefix(':').unwrap_or(spec_text).to_owned()
        }),
        is_self_documenting: interpolation.debug_text.is_some(),
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

/// Validates a PEP 3101 `field_name` (`arg_name("." attribute | "[" index "]")*`) and returns
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
        } else {
            let after_bracket = tail.strip_prefix('[')?;
            let close = after_bracket.find(']')?;
            if close == 0 {
                return None;
            }
            tail = &after_bracket[close + 1..];
        }
    }
    Some(root)
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
