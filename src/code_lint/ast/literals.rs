//! Normalized literal values, roles, and literal occurrence extraction across languages.

use super::tree::{AstNode, ParsedFile, dispatch_lang};

/// A literal's normalized value: equal values are the same literal whatever their spelling.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LiteralValue {
    /// Decoded string content.
    Str(String),
    /// Decoded byte-string content.
    Bytes(String),
    /// Integer value, with digit separators, base prefix and type suffix resolved.
    Int(i128),
    /// Float value as [`f64::to_bits`], with `-0.0` stored as `0.0`.
    Float(u64),
}

impl LiteralValue {
    /// Returns the arithmetic negation of a number, or `None` for strings and overflow.
    pub(super) fn negated(&self) -> Option<Self> {
        match self {
            Self::Int(value) => value.checked_neg().map(Self::Int),
            Self::Float(bits) => Some(Self::float(-f64::from_bits(*bits))),
            Self::Str(_) | Self::Bytes(_) => None,
        }
    }

    /// Builds a float value, storing `-0.0` as `0.0` so both spellings compare equal.
    fn float(value: f64) -> Self {
        Self::Float(if value == 0.0 { 0.0_f64 } else { value }.to_bits())
    }
}

/// Whether a literal defines a named constant or is used inline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::code_lint) enum LiteralRole {
    /// The whole value of a named scalar constant (`MAX_RETRIES = 3`, `const JJ: &str = "jj";`).
    ConstantDefinition,
    /// Any other position.
    Inline,
}

/// A literal that could be replaced by a named constant.
pub struct LiteralOccurrence<'a> {
    /// The literal, or for a negative number the node spanning its sign and digits.
    pub(crate) node: AstNode<'a>,
    /// The normalized value.
    pub(crate) value: LiteralValue,
    /// Whether the literal defines a constant or is used inline.
    pub(in crate::code_lint) role: LiteralRole,
}

/// Collects the literals in `file` that could be replaced by a named constant, in source order.
///
/// Literals the language requires (annotations, attributes, ABI strings, format strings) and
/// the parts of composite constant initializers are not collected.
#[must_use]
pub fn collect_literal_occurrences(file: &ParsedFile) -> Vec<LiteralOccurrence<'_>> {
    dispatch_lang!(file.lang(), collect_literal_occurrences(file))
}

/// Parses an integer literal with digit separators and an optional `0x` / `0o` / `0b` prefix
/// (any case); `None` on overflow.
pub(super) fn parse_integer_literal(text: &str) -> Option<LiteralValue> {
    let digits = text.replace('_', "");
    let (radix, digits) = match digits.get(..2) {
        Some("0x" | "0X") => (16, &digits[2..]),
        Some("0o" | "0O") => (8, &digits[2..]),
        Some("0b" | "0B") => (2, &digits[2..]),
        _ => (10, digits.as_str()),
    };
    i128::from_str_radix(digits, radix)
        .ok()
        .map(LiteralValue::Int)
}

/// Parses a decimal float literal with digit separators (`1_000.5`, `1e3`, `.5`).
pub(super) fn parse_float_literal(text: &str) -> Option<LiteralValue> {
    text.replace('_', "").parse().ok().map(LiteralValue::float)
}
