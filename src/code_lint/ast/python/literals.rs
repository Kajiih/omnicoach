//! Python literal occurrences: scalar literals with their role (constant definition or inline
//! use) and parsed value.

use super::annotations::has_final_annotation_expr;
use super::resolve_path_and_terminal_expr;
use crate::code_lint::ast::{
    AstNode, LiteralOccurrence, LiteralRole, LiteralValue, ParsedFile, parse_float_literal,
    parse_integer_literal, span_from_ruff_range,
};
use crate::diagnostic::SourceSpan;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{ExceptHandler, Expr, Stmt};
use ruff_text_size::Ranged as _;

/// Calls whose first argument is a name or type the language requires as a string
/// (`TypeVar("T")`, `cast("Node", value)`).
const TYPE_NAME_FIRST_ARGUMENT_CALLS: &[&str] = &[
    "TypeVar",
    "NewType",
    "ParamSpec",
    "TypeVarTuple",
    "NamedTuple",
    "TypedDict",
    "cast",
];

/// Parses a non-complex Python `Expr::NumberLiteral` into a [`LiteralValue`].
fn number_literal_value(expr: &Expr, source: &str) -> Option<LiteralValue> {
    let Expr::NumberLiteral(number) = expr else {
        return None;
    };
    let text = &source[number.range().start().to_usize()..number.range().end().to_usize()];
    match &number.value {
        ruff_python_ast::Number::Int(_) => parse_integer_literal(text),
        ruff_python_ast::Number::Float(_) => parse_float_literal(text),
        ruff_python_ast::Number::Complex { .. } => None,
    }
}

/// Returns the `(span, LiteralValue)` of a scalar literal expression suitable as a
/// [`LiteralRole::ConstantDefinition`] RHS (`None` for composite values like lists or
/// implicitly concatenated strings `'aa' 'bb'`).
fn scalar_literal_value(expr: &Expr, source: &str) -> Option<(SourceSpan, LiteralValue)> {
    match expr {
        Expr::NumberLiteral(_) => Some((
            span_from_ruff_range(expr.range()),
            number_literal_value(expr, source)?,
        )),
        Expr::UnaryOp(unary) if unary.op == ruff_python_ast::UnaryOp::USub => {
            let negated = number_literal_value(&unary.operand, source)?.negated()?;
            Some((span_from_ruff_range(unary.range()), negated))
        }
        Expr::StringLiteral(str_lit) => {
            let [part] = str_lit.value.as_slice() else {
                return None;
            };
            Some((
                span_from_ruff_range(part.range()),
                LiteralValue::Str(part.as_str().to_string()),
            ))
        }
        Expr::BytesLiteral(bytes_lit) => {
            let [part] = bytes_lit.value.as_slice() else {
                return None;
            };
            Some((
                span_from_ruff_range(part.range()),
                LiteralValue::Bytes(String::from_utf8_lossy(part.as_slice()).into_owned()),
            ))
        }
        Expr::FString(fstr) => {
            let mut parts = fstr.value.iter();
            let (Some(ruff_python_ast::FStringPartRef::FString(fpart)), None) =
                (parts.next(), parts.next())
            else {
                return None;
            };
            if fpart
                .elements
                .iter()
                .any(ruff_python_ast::InterpolatedStringElement::is_interpolation)
            {
                return None;
            }
            let text: String = fpart
                .elements
                .iter()
                .filter_map(|elt| elt.as_literal().map(|lit| lit.value.as_ref()))
                .collect();
            Some((span_from_ruff_range(fpart.range()), LiteralValue::Str(text)))
        }
        _ => None,
    }
}

/// Returns true if `name` is spelled as a constant (`MAX_RETRIES`, `_TIMEOUT_S`).
pub(super) fn is_constant_name(name: &str) -> bool {
    name.chars().any(|character| character.is_ascii_uppercase())
        && name.chars().all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
}

/// Returns true if `expr` is a standalone string/bytes literal expression (docstring candidate).
fn is_standalone_string_expr(expr: &Expr) -> bool {
    match expr {
        Expr::StringLiteral(_) | Expr::BytesLiteral(_) => true,
        Expr::FString(fstr) => !fstr.value.iter().any(|part| match part {
            ruff_python_ast::FStringPartRef::Literal(_) => false,
            ruff_python_ast::FStringPartRef::FString(fpart) => fpart
                .elements
                .iter()
                .any(ruff_python_ast::InterpolatedStringElement::is_interpolation),
        }),
        _ => false,
    }
}

struct LiteralOccurrenceCollector<'a> {
    file: &'a ParsedFile,
    in_constant_scope: bool,
    out: Vec<LiteralOccurrence<'a>>,
}

impl<'a> LiteralOccurrenceCollector<'a> {
    fn record_constant_rhs(&mut self, value: &Expr) {
        if let Some((span, lit_val)) = scalar_literal_value(value, &self.file.source) {
            self.out.push(LiteralOccurrence {
                node: AstNode::from_span(self.file, span),
                value: lit_val,
                role: LiteralRole::ConstantDefinition,
            });
        }
    }

    fn visit_non_constant_expr(&mut self, expr: &'a Expr) {
        let prev = self.in_constant_scope;
        self.in_constant_scope = false;
        self.visit_expr(expr);
        self.in_constant_scope = prev;
    }

    fn visit_fstring(&mut self, fstr: &'a ruff_python_ast::ExprFString) {
        for part in &fstr.value {
            match part {
                ruff_python_ast::FStringPartRef::Literal(lit) => {
                    self.out.push(LiteralOccurrence {
                        node: AstNode::from_span(self.file, span_from_ruff_range(lit.range())),
                        value: LiteralValue::Str(lit.as_str().to_string()),
                        role: LiteralRole::Inline,
                    });
                }
                ruff_python_ast::FStringPartRef::FString(fpart) => {
                    if fpart
                        .elements
                        .iter()
                        .any(ruff_python_ast::InterpolatedStringElement::is_interpolation)
                    {
                        for elt in &fpart.elements {
                            self.visit_interpolated_string_element(elt);
                        }
                    } else {
                        let text: String = fpart
                            .elements
                            .iter()
                            .filter_map(|elt| elt.as_literal().map(|lit| lit.value.as_ref()))
                            .collect();
                        self.out.push(LiteralOccurrence {
                            node: AstNode::from_span(
                                self.file,
                                span_from_ruff_range(fpart.range()),
                            ),
                            value: LiteralValue::Str(text),
                            role: LiteralRole::Inline,
                        });
                    }
                }
            }
        }
    }
}

impl<'a> SourceOrderVisitor<'a> for LiteralOccurrenceCollector<'a> {
    fn visit_annotation(&mut self, _expr: &'a Expr) {}

    fn visit_stmt(&mut self, statement: &'a Stmt) {
        match statement {
            Stmt::TypeAlias(_) => {}
            Stmt::Expr(expr_statement) if is_standalone_string_expr(&expr_statement.value) => {}
            Stmt::Assign(assign)
                if self.in_constant_scope
                    && matches!(
                        assign.targets.as_slice(),
                        [Expr::Name(name)] if is_constant_name(name.id.as_str())
                    ) =>
            {
                self.record_constant_rhs(&assign.value);
            }
            Stmt::AnnAssign(ann)
                if self.in_constant_scope
                    && (matches!(
                        ann.target.as_ref(),
                        Expr::Name(name) if is_constant_name(name.id.as_str())
                    ) || has_final_annotation_expr(&ann.annotation, self.file)) =>
            {
                if let Some(value) = &ann.value {
                    self.record_constant_rhs(value);
                }
            }
            Stmt::ClassDef(cls) => {
                let prev = self.in_constant_scope;
                self.in_constant_scope = false;
                for dec in &cls.decorator_list {
                    self.visit_decorator(dec);
                }
                if let Some(args) = &cls.arguments {
                    self.visit_arguments(args);
                }
                self.in_constant_scope = true;
                self.visit_body(&cls.body);
                self.in_constant_scope = prev;
            }
            Stmt::If(if_statement) => {
                self.visit_non_constant_expr(&if_statement.test);
                self.visit_body(&if_statement.body);
                for clause in &if_statement.elif_else_clauses {
                    if let Some(test) = &clause.test {
                        self.visit_non_constant_expr(test);
                    }
                    self.visit_body(&clause.body);
                }
            }
            Stmt::Try(try_statement) => {
                self.visit_body(&try_statement.body);
                for handler in &try_statement.handlers {
                    let ExceptHandler::ExceptHandler(except_handler) = handler;
                    if let Some(type_expr) = &except_handler.type_ {
                        self.visit_non_constant_expr(type_expr);
                    }
                    self.visit_body(&except_handler.body);
                }
                self.visit_body(&try_statement.orelse);
                self.visit_body(&try_statement.finalbody);
            }
            Stmt::With(with_statement) => {
                for item in &with_statement.items {
                    self.visit_non_constant_expr(&item.context_expr);
                    if let Some(vars) = &item.optional_vars {
                        self.visit_non_constant_expr(vars);
                    }
                }
                self.visit_body(&with_statement.body);
            }
            Stmt::FunctionDef(_) | Stmt::For(_) | Stmt::While(_) | Stmt::Match(_) => {
                let prev = self.in_constant_scope;
                self.in_constant_scope = false;
                walk_stmt(self, statement);
                self.in_constant_scope = prev;
            }
            _ => walk_stmt(self, statement),
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Subscript(sub)
                if resolve_path_and_terminal_expr(&sub.value, &self.file.source).1 == "Literal" =>
            {
                return;
            }
            Expr::Call(call) => {
                let (_, terminal) = resolve_path_and_terminal_expr(&call.func, &self.file.source);
                if TYPE_NAME_FIRST_ARGUMENT_CALLS.contains(&terminal.as_str()) {
                    self.visit_expr(&call.func);
                    if call.arguments.args.is_empty() {
                        for kw in call.arguments.keywords.iter().skip(1) {
                            self.visit_keyword(kw);
                        }
                    } else {
                        for arg in call.arguments.args.iter().skip(1) {
                            self.visit_expr(arg);
                        }
                        for kw in &call.arguments.keywords {
                            self.visit_keyword(kw);
                        }
                    }
                    return;
                }
            }
            Expr::UnaryOp(unary) if unary.op == ruff_python_ast::UnaryOp::USub => {
                if let Some(val) = number_literal_value(&unary.operand, &self.file.source)
                    && let Some(negated) = val.negated()
                {
                    self.out.push(LiteralOccurrence {
                        node: AstNode::from_span(self.file, span_from_ruff_range(unary.range())),
                        value: negated,
                        role: LiteralRole::Inline,
                    });
                    return;
                }
            }
            Expr::NumberLiteral(number) => {
                if let Some(val) = number_literal_value(expr, &self.file.source) {
                    self.out.push(LiteralOccurrence {
                        node: AstNode::from_span(self.file, span_from_ruff_range(number.range())),
                        value: val,
                        role: LiteralRole::Inline,
                    });
                }
                return;
            }
            Expr::StringLiteral(str_lit) => {
                for part in str_lit.value.as_slice() {
                    self.out.push(LiteralOccurrence {
                        node: AstNode::from_span(self.file, span_from_ruff_range(part.range())),
                        value: LiteralValue::Str(part.as_str().to_string()),
                        role: LiteralRole::Inline,
                    });
                }
                return;
            }
            Expr::BytesLiteral(bytes_lit) => {
                for part in bytes_lit.value.as_slice() {
                    self.out.push(LiteralOccurrence {
                        node: AstNode::from_span(self.file, span_from_ruff_range(part.range())),
                        value: LiteralValue::Bytes(
                            String::from_utf8_lossy(part.as_slice()).into_owned(),
                        ),
                        role: LiteralRole::Inline,
                    });
                }
                return;
            }
            Expr::FString(fstr) => {
                self.visit_fstring(fstr);
                return;
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

/// Collects Python literal occurrences (see [`crate::code_lint::ast::collect_literal_occurrences`]).
#[must_use]
pub(in crate::code_lint::ast) fn collect_literal_occurrences(
    file: &ParsedFile,
) -> Vec<LiteralOccurrence<'_>> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut collector = LiteralOccurrenceCollector {
        file,
        in_constant_scope: true,
        out: Vec::new(),
    };
    collector.visit_body(&parsed.syntax().body);
    collector.out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;
    use LiteralRole::{ConstantDefinition, Inline};

    #[rstest::rstest]
    #[case::module_upper_name_defines("MAX = 30", &[("30", ConstantDefinition)])]
    #[case::class_final_lowercase_defines("class C:\n    limit: Final[int] = -5", &[("-5", ConstantDefinition)])]
    #[case::aliased_final_defines("timeout: t.Final = 30", &[("30", ConstantDefinition)])]
    #[case::class_lowercase_is_inline("class C:\n    name = 'cc'", &[("'cc'", Inline)])]
    #[case::function_level_upper_name_is_inline("def f():\n    MAX = 30", &[("30", Inline)])]
    #[case::module_if_body_defines("if WIN:\n    RETRIES = 3\nelse:\n    RETRIES = 5", &[("3", ConstantDefinition), ("5", ConstantDefinition)])]
    #[case::module_except_body_defines("try:\n    import x\nexcept ImportError:\n    LIMIT = 9", &[("9", ConstantDefinition)])]
    #[case::function_if_body_is_inline("def f():\n    if a:\n        MAX = 30", &[("30", Inline)])]
    #[case::parenthesized_constant_defines("MSG = (\n    'refused'\n)", &[("'refused'", ConstantDefinition)])]
    #[case::composite_constant_not_collected("URLS = ['u1', 'u2']\nPAIR = 'aa' 'bb'", &[])]
    #[case::negation_anchored_on_operator("f(-42, 7 - 42)", &[("-42", Inline), ("7", Inline), ("42", Inline)])]
    #[case::negative_case_pattern("match m:\n    case [-42, 'xx']:\n        pass", &[("-42", Inline), ("'xx'", Inline)])]
    #[case::signed_numbers_in_patterns_collected("match m:\n    case {-404: _} | Resp(code=-404) | -7:\n        pass", &[("-404", Inline), ("-404", Inline), ("-7", Inline)])]
    #[case::docstring_skipped("def f():\n    '''Doc.'''\n    return 'rv'", &[("'rv'", Inline)])]
    #[case::annotations_skipped("def f(a: 'T' = 'dv') -> 'R':\n    v: 'V' = 'vv'", &[("'dv'", Inline), ("'vv'", Inline)])]
    #[case::literal_type_skipped("v = Literal['y']", &[])]
    #[case::interpolated_fstring_walked("print(f\"{row['st']} and\", f'plain')", &[("'st'", Inline), ("f'plain'", Inline)])]
    #[case::imaginary_skipped("z = 2j", &[])]
    fn test_collect_literal_occurrences_python(
        #[case] source: &str,
        #[case] expected: &[(&str, LiteralRole)],
    ) {
        let file = ParsedFile::new(source, Language::Python);
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

    #[rstest::rstest]
    fn test_collect_literal_occurrences_python_skips_type_name_argument(
        #[values(
            "TypeVar",
            "NewType",
            "ParamSpec",
            "TypeVarTuple",
            "NamedTuple",
            "TypedDict",
            "typing.cast"
        )]
        callee: &str,
    ) {
        let file = ParsedFile::new(&format!("t = {callee}('Nm', 'vv')"), Language::Python);
        let texts: Vec<String> = collect_literal_occurrences(&file)
            .into_iter()
            .map(|occurrence| occurrence.node.text().into_owned())
            .collect();
        assert_eq!(texts, vec!["'vv'"]);
    }

    #[rstest::rstest]
    #[case::quote_style_ignored("'ab'", LiteralValue::Str("ab".to_string()))]
    #[case::triple_quoted("'''ab'''", LiteralValue::Str("ab".to_string()))]
    #[case::unicode_prefix("u'ab'", LiteralValue::Str("ab".to_string()))]
    #[case::bytes("b\"ab\"", LiteralValue::Bytes("ab".to_string()))]
    #[case::raw_bytes("Rb'ab'", LiteralValue::Bytes("ab".to_string()))]
    #[case::escapes_decoded("'a\\nb'", LiteralValue::Str("a\nb".to_string()))]
    #[case::raw_backslash_kept("r'a\\nb'", LiteralValue::Str("a\\nb".to_string()))]
    #[case::hex("0x1F", LiteralValue::Int(31))]
    #[case::separators("1_000", LiteralValue::Int(1000))]
    #[case::exponent("1e3", LiteralValue::Float(1000.0_f64.to_bits()))]
    #[case::leading_dot(".5", LiteralValue::Float(0.5_f64.to_bits()))]
    #[case::trailing_dot("3.", LiteralValue::Float(3.0_f64.to_bits()))]
    #[case::negative_float("-2.5", LiteralValue::Float((-2.5_f64).to_bits()))]
    #[case::negative_zero("-0.0", LiteralValue::Float(0.0_f64.to_bits()))]
    fn test_collect_literal_occurrences_python_values(
        #[case] literal: &str,
        #[case] expected: LiteralValue,
    ) {
        let file = ParsedFile::new(&format!("value = {literal}"), Language::Python);
        let values: Vec<LiteralValue> = collect_literal_occurrences(&file)
            .into_iter()
            .map(|occurrence| occurrence.value)
            .collect();
        assert_eq!(values, vec![expected]);
    }
}
