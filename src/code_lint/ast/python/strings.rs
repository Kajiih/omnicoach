//! Python string literals: prefixes, implicit concatenation and literal segments.

use ruff_python_ast::{
    Expr, FString, InterpolatedElement, InterpolatedStringElement, StringFlags as _,
};

/// Splits an `FString` node into its literal text segments (between delimiters and
/// `InterpolatedElement` children) and its `InterpolatedElement` nodes, using raw source byte
/// offsets so escape sequences are not decoded.
pub(super) fn fstring_segments_and_interpolations<'a>(
    fstring: &'a FString,
    source: &str,
) -> (Vec<String>, Vec<&'a InterpolatedElement>) {
    let content_start = usize::from(fstring.range.start() + fstring.flags.opener_len());
    let content_end = usize::from(fstring.range.end() - fstring.flags.closer_len());

    let interpolations: Vec<&'a InterpolatedElement> = fstring
        .elements
        .iter()
        .filter_map(|element| match element {
            InterpolatedStringElement::Interpolation(interpolation) => Some(interpolation),
            InterpolatedStringElement::Literal(_) => None,
        })
        .collect();

    let mut segments = Vec::with_capacity(interpolations.len() + 1);
    let mut cursor = content_start;
    for interpolation in &interpolations {
        let interp_start = usize::from(interpolation.range.start());
        let interp_end = usize::from(interpolation.range.end());
        segments.push(
            source
                .get(cursor..interp_start)
                .unwrap_or_default()
                .to_owned(),
        );
        cursor = interp_end;
    }
    segments.push(
        source
            .get(cursor..content_end)
            .unwrap_or_default()
            .to_owned(),
    );
    (segments, interpolations)
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

/// Extracts the static text of a plain string literal or an implicit concatenation of plain
/// string literals (with `\N{...}` escapes stripped in non-raw strings), excluding f-strings and
/// byte strings.
pub(super) fn static_string_text(expr: &Expr, source: &str) -> Option<String> {
    let Expr::StringLiteral(string_literal) = expr else {
        return None;
    };
    let mut combined = String::new();
    for part in string_literal.value.as_slice() {
        let range = part.content_range();
        let content = &source[usize::from(range.start())..usize::from(range.end())];
        if part.flags.prefix().is_raw() {
            combined.push_str(content);
        } else {
            combined.push_str(&strip_named_unicode_escapes(content));
        }
    }
    (!combined.is_empty()).then_some(combined)
}
