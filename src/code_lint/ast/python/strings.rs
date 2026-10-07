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

/// Extracts the decoded text of a plain string literal or an implicit concatenation of plain
/// string literals, excluding f-strings and byte strings.
pub(super) fn static_string_text(expr: &Expr) -> Option<String> {
    let Expr::StringLiteral(string_literal) = expr else {
        return None;
    };
    let text = string_literal.value.to_str();
    (!text.is_empty()).then(|| text.to_owned())
}
