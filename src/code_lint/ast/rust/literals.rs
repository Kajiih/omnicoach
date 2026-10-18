//! Literal occurrence and value extraction for Rust (`ra_ap_syntax`).

use super::{macro_call_terminal_name, resolve_preceding_macro_path};
use crate::code_lint::ast::{
    AstNode, LiteralOccurrence, LiteralRole, LiteralValue, ParsedFile, parse_float_literal,
    parse_integer_literal, span_from_rowan_range,
};
use ra_ap_syntax::ast;
use ra_ap_syntax::{
    AstNode as _, AstToken as _, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken,
};

/// Rust 32-bit float literal suffix.
const FLOAT_SUFFIX_32: &str = "f32";
/// Rust 64-bit float literal suffix.
const FLOAT_SUFFIX_64: &str = "f64";

/// Macros whose literal arguments the language or API requires as written: format strings,
/// assertion and logging messages, and compile-time inputs. Matched on the last path segment.
const LITERAL_EXEMPT_MACROS: &[&str] = &[
    "format",
    "print",
    "println",
    "eprint",
    "eprintln",
    "write",
    "writeln",
    "panic",
    "todo",
    "unimplemented",
    "unreachable",
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "bail",
    "ensure",
    "anyhow",
    "error",
    "warn",
    "info",
    "debug",
    "trace",
    "indoc",
    "concat",
    "env",
    "option_env",
    "include_str",
    "include_bytes",
];

/// Collects Rust literal occurrences (see [`super::collect_literal_occurrences`]).
#[must_use]
pub(in crate::code_lint::ast) fn collect_literal_occurrences(
    file: &ParsedFile,
) -> Vec<LiteralOccurrence<'_>> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_literal_occurrences_rec(parsed.tree().syntax(), file, &mut out);
    out
}

/// Walks `node`, pushing collectable literals to `out` (see [`super::collect_literal_occurrences`]).
fn collect_literal_occurrences_rec<'a>(
    node: &SyntaxNode,
    file: &'a ParsedFile,
    out: &mut Vec<LiteralOccurrence<'a>>,
) {
    let kind = node.kind();
    if ast::Attr::can_cast(kind) {
        return;
    }
    if let Some(macro_call) = ast::MacroCall::cast(node.clone())
        && is_literal_exempt_macro(&macro_call_terminal_name(&macro_call))
    {
        return;
    }
    if kind == SyntaxKind::TOKEN_TREE {
        if resolve_preceding_macro_path(node)
            .is_some_and(|macro_path| is_literal_exempt_macro(&macro_path.terminal))
        {
            return;
        }
        let mut follows_minus = false;
        let mut follows_dot = false;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Node(child_node) => {
                    follows_minus = false;
                    follows_dot = false;
                    collect_literal_occurrences_rec(&child_node, file, out);
                }
                SyntaxElement::Token(token) => {
                    if token.kind().is_trivia() {
                        continue;
                    }
                    let is_number = matches!(
                        token.kind(),
                        SyntaxKind::INT_NUMBER | SyntaxKind::FLOAT_NUMBER
                    );
                    let is_signed_number = follows_minus && is_number;
                    let is_tuple_field = follows_dot && is_number;
                    follows_minus = token.kind() == SyntaxKind::MINUS;
                    follows_dot = token.kind() == SyntaxKind::DOT;
                    if !is_signed_number
                        && !is_tuple_field
                        && let Some(value) = token_literal_value(&token)
                    {
                        out.push(LiteralOccurrence {
                            node: AstNode::from_span(
                                file,
                                span_from_rowan_range(token.text_range()),
                            ),
                            value,
                            role: LiteralRole::Inline,
                        });
                    }
                }
            }
        }
        return;
    }
    if let Some(ScalarConstant { initializer }) = constant_initializer_expr(node) {
        if let Some(expr_node) = initializer
            && let Some(value) = node_literal_value(expr_node.syntax())
        {
            out.push(LiteralOccurrence {
                node: AstNode::from_span(
                    file,
                    span_from_rowan_range(expr_node.syntax().text_range()),
                ),
                value,
                role: LiteralRole::ConstantDefinition,
            });
        }
        return;
    }
    if let Some(value) = node_literal_value(node) {
        out.push(LiteralOccurrence {
            node: AstNode::from_span(file, span_from_rowan_range(node.text_range())),
            value,
            role: LiteralRole::Inline,
        });
        return;
    }
    for child in node.children() {
        collect_literal_occurrences_rec(&child, file, out);
    }
}

/// Returns true if `name` (a macro's last path segment) is in [`LITERAL_EXEMPT_MACROS`].
fn is_literal_exempt_macro(name: &str) -> bool {
    LITERAL_EXEMPT_MACROS.contains(&name)
}

/// A scalar constant item (`const`, non-`mut` `static`, or `enum` variant).
struct ScalarConstant {
    /// The initializer expression node, absent when the item has none (`const N: usize;`).
    initializer: Option<ast::Expr>,
}

/// Returns `node` as a scalar constant (`const`, non-`mut` `static`, or `enum` variant).
fn constant_initializer_expr(node: &SyntaxNode) -> Option<ScalarConstant> {
    if let Some(const_item) = ast::Const::cast(node.clone()) {
        return Some(ScalarConstant {
            initializer: const_item.body(),
        });
    }
    if let Some(variant) = ast::Variant::cast(node.clone()) {
        return Some(ScalarConstant {
            initializer: variant
                .const_arg()
                .and_then(|const_arg| const_arg.expr())
                .or_else(|| variant.syntax().children().find_map(ast::Expr::cast)),
        });
    }
    if let Some(static_item) = ast::Static::cast(node.clone())
        && static_item.mut_token().is_none()
    {
        return Some(ScalarConstant {
            initializer: static_item.body(),
        });
    }
    None
}

/// Evaluates an [`ast::Literal`], negative [`ast::PrefixExpr`], or [`ast::LiteralPat`] node
/// to its [`LiteralValue`].
fn node_literal_value(node: &SyntaxNode) -> Option<LiteralValue> {
    if let Some(literal) = ast::Literal::cast(node.clone()) {
        let token = literal.token();
        if !matches!(
            token.kind(),
            SyntaxKind::STRING
                | SyntaxKind::BYTE_STRING
                | SyntaxKind::INT_NUMBER
                | SyntaxKind::FLOAT_NUMBER
        ) {
            return None;
        }
        let value = token_literal_value(&token)?;
        let has_leading_minus = node
            .children_with_tokens()
            .filter_map(SyntaxElement::into_token)
            .any(|child_tok| child_tok.kind() == SyntaxKind::MINUS);
        return if has_leading_minus {
            value.negated()
        } else {
            Some(value)
        };
    }
    if let Some(prefix_expr) = ast::PrefixExpr::cast(node.clone()) {
        if prefix_expr.op_kind() != Some(ast::UnaryOp::Neg) {
            return None;
        }
        let ast::Expr::Literal(inner_literal) = prefix_expr.expr()? else {
            return None;
        };
        let token = inner_literal.token();
        if !matches!(
            token.kind(),
            SyntaxKind::INT_NUMBER | SyntaxKind::FLOAT_NUMBER
        ) {
            return None;
        }
        return token_literal_value(&token)?.negated();
    }
    if let Some(literal_pat) = ast::LiteralPat::cast(node.clone()) {
        let inner_literal = literal_pat.literal()?;
        let token = inner_literal.token();
        let value = token_literal_value(&token)?;
        return if literal_pat.minus_token().is_some() {
            value.negated()
        } else {
            Some(value)
        };
    }
    None
}

/// Decodes a literal [`SyntaxToken`] (`STRING`, `BYTE_STRING`, `INT_NUMBER`, `FLOAT_NUMBER`)
/// into its [`LiteralValue`], using `ra_ap_syntax`'s native escape decoder for strings and byte strings.
fn token_literal_value(token: &SyntaxToken) -> Option<LiteralValue> {
    if let Some(string_token) = ast::String::cast(token.clone()) {
        let decoded = string_token.value().ok()?;
        return Some(LiteralValue::Str(decoded.into_owned()));
    }
    if let Some(byte_string_token) = ast::ByteString::cast(token.clone()) {
        let decoded = byte_string_token.value().ok()?;
        return Some(LiteralValue::Bytes(
            String::from_utf8_lossy(&decoded).into_owned(),
        ));
    }
    match token.kind() {
        SyntaxKind::INT_NUMBER => integer_literal_value(token.text()),
        SyntaxKind::FLOAT_NUMBER => {
            let text = token.text();
            let number = text
                .strip_suffix(FLOAT_SUFFIX_32)
                .or_else(|| text.strip_suffix(FLOAT_SUFFIX_64))
                .unwrap_or(text);
            parse_float_literal(number)
        }
        _ => None,
    }
}

/// Returns the value of a Rust integer literal, resolving its type suffix (`30u64`, `0xffu8`);
/// a decimal literal with an `f32` / `f64` suffix (`1f32`) is a float.
fn integer_literal_value(text: &str) -> Option<LiteralValue> {
    let is_hex = text.starts_with("0x") || text.starts_with("0X");
    let digits_start = if is_hex
        || text.starts_with("0o")
        || text.starts_with("0O")
        || text.starts_with("0b")
        || text.starts_with("0B")
    {
        2
    } else {
        0
    };
    let suffix_start = text[digits_start..]
        .find(|character: char| {
            if is_hex {
                matches!(character, 'u' | 'i')
            } else {
                character.is_ascii_alphabetic()
            }
        })
        .map_or(text.len(), |offset| digits_start + offset);
    let (number, suffix) = text.split_at(suffix_start);
    if matches!(suffix, FLOAT_SUFFIX_32 | FLOAT_SUFFIX_64) {
        parse_float_literal(number)
    } else {
        parse_integer_literal(number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use LiteralRole::{ConstantDefinition, Inline};

    #[rstest::rstest]
    #[case::const_defines("const MAX: u32 = 30;", &[("30", ConstantDefinition)])]
    #[case::static_defines("static MIN: i32 = -5;", &[("-5", ConstantDefinition)])]
    #[case::enum_discriminant_defines("enum E { V = 10 }", &[("10", ConstantDefinition)])]
    #[case::static_mut_is_inline("static mut LIMIT: u32 = 30;", &[("30", Inline)])]
    #[case::composite_constant_not_collected("const NAMES: &[&str] = &[\"n1\", \"n2\"];", &[])]
    #[case::negation_anchored_on_operator(
        "fn f() { g(-42, 7 - 42); match x { -42 => {} _ => {} } }",
        &[("-42", Inline), ("7", Inline), ("42", Inline), ("-42", Inline)]
    )]
    #[case::signed_number_in_token_tree_skipped("fn f() { vec![-42, 43]; }", &[("43", Inline)])]
    #[case::tuple_position_skipped("fn f() { g(pair.3, 30); }", &[("30", Inline)])]
    #[case::attribute_skipped("#[cfg(feature = \"ff\")]\nfn f() {}", &[])]
    #[case::inner_attribute_skipped("#![doc = \"dd\"]", &[])]
    #[case::extern_abi_skipped("extern \"C\" { fn g(); }", &[])]
    #[case::exempt_macro_by_path_skipped("fn f() { tracing::warn!(\"ww\"); }", &[])]
    #[case::exempt_macro_inside_collected_macro_skipped(
        "fn f() { vec![format!(\"{}\", 5), \"vv\"]; }",
        &[("\"vv\"", Inline)]
    )]
    #[case::matches_macro_collected("fn f() { matches!(k, \"ma\"); }", &[("\"ma\"", Inline)])]
    #[case::char_skipped("fn f() { g('c'); }", &[])]
    #[case::c_string_skipped("fn f() { g(c\"cs\"); }", &[])]
    fn test_collect_literal_occurrences_rust(
        #[case] source: &str,
        #[case] expected: &[(&str, LiteralRole)],
    ) {
        let file = ParsedFile::rust(source);
        let actual: Vec<(String, LiteralRole)> = collect_literal_occurrences(&file)
            .into_iter()
            .map(|occurrence| (occurrence.node.text().into_owned(), occurrence.role))
            .collect();
        let expected: Vec<(String, LiteralRole)> = expected
            .iter()
            .map(|(text, role)| ((*text).to_string(), *role))
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_collect_literal_occurrences_rust_skips_every_exempt_macro() {
        let collected: Vec<&str> = LITERAL_EXEMPT_MACROS
            .iter()
            .copied()
            .filter(|name| {
                let file = ParsedFile::rust(&format!("fn f() {{ {name}!(\"ee\"); }}"));
                !collect_literal_occurrences(&file).is_empty()
            })
            .collect();
        assert_eq!(collected, Vec::<&str>::new());
    }

    #[rstest::rstest]
    #[case::raw_string("r#\"ab\"#", LiteralValue::Str("ab".to_string()))]
    #[case::raw_backslash_decoded("r\"a\\tb\"", LiteralValue::Str("a\\tb".to_string()))]
    #[case::raw_quote_decoded("r#\"a\"b\"#", LiteralValue::Str("a\"b".to_string()))]
    #[case::empty_raw_string("r\"\"", LiteralValue::Str(String::new()))]
    #[case::byte_string("b\"ab\"", LiteralValue::Bytes("ab".to_string()))]
    #[case::raw_byte_string("br\"ab\"", LiteralValue::Bytes("ab".to_string()))]
    #[case::integer_suffix("1_000_u32", LiteralValue::Int(1000))]
    #[case::hex_suffix("0xffu8", LiteralValue::Int(255))]
    #[case::hex_digits_ending_in_f32("0x1f32", LiteralValue::Int(0x1f32))]
    #[case::octal_suffix("0o17u8", LiteralValue::Int(15))]
    #[case::binary("0b11", LiteralValue::Int(3))]
    #[case::float_suffix_on_integer("1f32", LiteralValue::Float(1.0_f64.to_bits()))]
    #[case::float_suffix_after_separator("1_f32", LiteralValue::Float(1.0_f64.to_bits()))]
    #[case::float_suffix("1.5f64", LiteralValue::Float(1.5_f64.to_bits()))]
    #[case::exponent_with_suffix("1e3f32", LiteralValue::Float(1000.0_f64.to_bits()))]
    #[case::negative_zero("-0.0", LiteralValue::Float(0.0_f64.to_bits()))]
    fn test_collect_literal_occurrences_rust_values(
        #[case] literal: &str,
        #[case] expected: LiteralValue,
    ) {
        let file = ParsedFile::rust(&format!("fn f() {{ g({literal}); }}"));
        let values: Vec<LiteralValue> = collect_literal_occurrences(&file)
            .into_iter()
            .map(|occurrence| occurrence.value)
            .collect();
        assert_eq!(values, vec![expected]);
    }

    #[test]
    fn test_collect_literal_occurrences_rust_skips_overflow() {
        let file = ParsedFile::rust("fn f() { g(0xffff_ffff_ffff_ffff_ffff_ffff_ffff_ffff_f); }");
        assert!(collect_literal_occurrences(&file).is_empty());
    }
}
