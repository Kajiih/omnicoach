//! Python literal occurrences.

use super::bindings::constant_assignment_value;
use super::classes::typed_dict_keys;
use super::names::written_name;
use crate::code_lint::ast::literals::{
    LiteralOccurrence, LiteralRole, LiteralValue, parse_float_literal, parse_integer_literal,
};
use crate::code_lint::ast::tree::{AstNode, ParsedFile, span_from_ruff_range};
use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, walk_expr, walk_pattern, walk_stmt,
};
use ruff_python_ast::{ExceptHandler, Expr, Stmt};
use ruff_text_size::Ranged as _;
use std::collections::HashSet;

/// Calls whose first argument is the declared type's name (`TypeVar("T")`, `cast("Node", x)`).
const TYPE_NAME_FIRST_ARGUMENT_CALLS: &[&str] = &[
    "TypeVar",
    "NewType",
    "ParamSpec",
    "TypeVarTuple",
    "NamedTuple",
    "TypedDict",
    "cast",
];

/// Dictionary methods whose first argument is a key lookup (`d.get("k")`, `d.pop("k")`).
const DICT_KEY_FIRST_ARGUMENT_METHODS: &[&str] = &["get", "pop", "setdefault"];

/// Collects Python literal occurrences (see [`crate::code_lint::ast::collect_literal_occurrences`]).
#[must_use]
pub fn collect_literal_occurrences(file: &ParsedFile) -> Vec<LiteralOccurrence<'_>> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut collector = LiteralOccurrenceCollector {
        file,
        typed_dict_keys: typed_dict_keys(file),
        in_constant_scope: true,
        out: Vec::new(),
    };
    collector.visit_body(&parsed.syntax().body);
    collector.out
}

struct LiteralOccurrenceCollector<'a> {
    file: &'a ParsedFile,
    typed_dict_keys: HashSet<String>,
    in_constant_scope: bool,
    out: Vec<LiteralOccurrence<'a>>,
}

impl<'a> LiteralOccurrenceCollector<'a> {
    fn record_constant_rhs(&mut self, value: &Expr) {
        self.out
            .extend(constant_definition_occurrence(value, self.file));
    }

    fn visit_non_constant_expr(&mut self, expr: &'a Expr) {
        let prev = self.in_constant_scope;
        self.in_constant_scope = false;
        self.visit_expr(expr);
        self.in_constant_scope = prev;
    }

    fn visit_call(&mut self, call: &'a ruff_python_ast::ExprCall) {
        self.visit_expr(&call.func);
        let mut args = call.arguments.args.iter();
        if let Some(first_arg) = args.next()
            && !TYPE_NAME_FIRST_ARGUMENT_CALLS.contains(
                &written_name(&call.func, &self.file.source)
                    .terminal
                    .as_str(),
            )
        {
            if let Expr::Attribute(attr) = call.func.as_ref()
                && DICT_KEY_FIRST_ARGUMENT_METHODS.contains(&attr.attr.as_str())
            {
                self.visit_dict_key(first_arg);
            } else {
                self.visit_expr(first_arg);
            }
        }
        for arg in args {
            self.visit_expr(arg);
        }
        for keyword in &call.arguments.keywords {
            self.visit_keyword(keyword);
        }
    }

    fn visit_dict_key(&mut self, expr: &'a Expr) {
        if let Expr::StringLiteral(str_lit) = expr
            && let [part] = str_lit.value.as_slice()
            && self.typed_dict_keys.contains(part.as_str())
        {
            return;
        }
        self.visit_expr(expr);
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
            Stmt::Assign(_) | Stmt::AnnAssign(_)
                if self.in_constant_scope
                    && let Some(value) = constant_assignment_value(statement, self.file) =>
            {
                self.record_constant_rhs(value);
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
            Expr::Dict(dict) => {
                for item in &dict.items {
                    if let Some(key) = &item.key {
                        self.visit_dict_key(key);
                    }
                    self.visit_expr(&item.value);
                }
                return;
            }
            Expr::Subscript(sub) => {
                if written_name(&sub.value, &self.file.source).terminal != "Literal" {
                    self.visit_expr(&sub.value);
                    self.visit_dict_key(&sub.slice);
                }
                return;
            }
            Expr::Compare(cmp) => {
                let mut operands = cmp.operands.iter();
                if let Some(left) = operands.next() {
                    if matches!(
                        cmp.ops.first(),
                        Some(ruff_python_ast::CmpOp::In | ruff_python_ast::CmpOp::NotIn)
                    ) {
                        self.visit_dict_key(left);
                    } else {
                        self.visit_expr(left);
                    }
                }
                for comparator in operands {
                    self.visit_expr(comparator);
                }
                return;
            }
            Expr::Call(call) => {
                self.visit_call(call);
                return;
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

    fn visit_pattern(&mut self, pattern: &'a ruff_python_ast::Pattern) {
        if let ruff_python_ast::Pattern::MatchMapping(mapping) = pattern {
            for key in &mapping.keys {
                self.visit_dict_key(key);
            }
            for sub_pattern in &mapping.patterns {
                self.visit_pattern(sub_pattern);
            }
            return;
        }
        walk_pattern(self, pattern);
    }
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

/// Returns the [`LiteralRole::ConstantDefinition`] occurrence of a scalar literal constant
/// RHS (`None` for composite values like lists or implicitly concatenated strings `'aa' 'bb'`).
fn constant_definition_occurrence<'a>(
    expr: &Expr,
    file: &'a ParsedFile,
) -> Option<LiteralOccurrence<'a>> {
    let (span, value) = match expr {
        Expr::NumberLiteral(_) => (
            span_from_ruff_range(expr.range()),
            number_literal_value(expr, &file.source)?,
        ),
        Expr::UnaryOp(unary) if unary.op == ruff_python_ast::UnaryOp::USub => {
            let negated = number_literal_value(&unary.operand, &file.source)?.negated()?;
            (span_from_ruff_range(unary.range()), negated)
        }
        Expr::StringLiteral(str_lit) => {
            let [part] = str_lit.value.as_slice() else {
                return None;
            };
            (
                span_from_ruff_range(part.range()),
                LiteralValue::Str(part.as_str().to_string()),
            )
        }
        Expr::BytesLiteral(bytes_lit) => {
            let [part] = bytes_lit.value.as_slice() else {
                return None;
            };
            (
                span_from_ruff_range(part.range()),
                LiteralValue::Bytes(String::from_utf8_lossy(part.as_slice()).into_owned()),
            )
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
            (span_from_ruff_range(fpart.range()), LiteralValue::Str(text))
        }
        _ => return None,
    };
    Some(LiteralOccurrence {
        node: AstNode::from_span(file, span),
        value,
        role: LiteralRole::ConstantDefinition,
    })
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;
    use LiteralRole::{ConstantDefinition, Inline};

    #[rstest::rstest]
    #[case::module_upper_name_defines("MAX = 30", &[("30", ConstantDefinition)])]
    #[case::class_final_lowercase_defines("class C:\n    limit: Final[int] = -5", &[("-5", ConstantDefinition)])]
    #[case::aliased_final_defines("import typing as t\ntimeout: t.Final = 30", &[("30", ConstantDefinition)])]
    #[case::class_lowercase_is_inline("class C:\n    name = 'cc'", &[("'cc'", Inline)])]
    #[case::function_level_upper_name_is_inline("def f():\n    MAX = 30", &[("30", Inline)])]
    #[case::module_if_body_defines(indoc::indoc! {r"
        if WIN:
            RETRIES = 3
        else:
            RETRIES = 5"}, &[("3", ConstantDefinition), ("5", ConstantDefinition)])]
    #[case::module_except_body_defines(indoc::indoc! {r"
        try:
            import x
        except ImportError:
            LIMIT = 9"}, &[("9", ConstantDefinition)])]
    #[case::function_if_body_is_inline(indoc::indoc! {r"
        def f():
            if a:
                MAX = 30"}, &[("30", Inline)])]
    #[case::parenthesized_constant_defines(indoc::indoc! {r"
        MSG = (
            'refused'
        )"}, &[("'refused'", ConstantDefinition)])]
    #[case::composite_constant_not_collected("URLS = ['u1', 'u2']\nPAIR = 'aa' 'bb'", &[])]
    #[case::negation_anchored_on_operator("f(-42, 7 - 42)", &[("-42", Inline), ("7", Inline), ("42", Inline)])]
    #[case::negative_case_pattern(indoc::indoc! {r"
        match m:
            case [-42, 'xx']:
                pass"}, &[("-42", Inline), ("'xx'", Inline)])]
    #[case::signed_numbers_in_patterns_collected(indoc::indoc! {r"
        match m:
            case {-404: _} | Resp(code=-404) | -7:
                pass"}, &[("-404", Inline), ("-404", Inline), ("-7", Inline)])]
    #[case::docstring_skipped(indoc::indoc! {r"
        def f():
            '''Doc.'''
            return 'rv'"}, &[("'rv'", Inline)])]
    #[case::annotations_skipped("def f(a: 'T' = 'dv') -> 'R':\n    v: 'V' = 'vv'", &[("'dv'", Inline), ("'vv'", Inline)])]
    #[case::literal_type_skipped("v = Literal['y']", &[])]
    #[case::cast_skips_type_argument_only(
        "v = f('rx').cast('Node', 'vv')",
        &[("'rx'", Inline), ("'vv'", Inline)]
    )]
    #[case::typed_dict_keys_skipped_only_in_dict_key_positions(
        indoc::indoc! {r"
            class Fill(TypedDict):
                symbol: str
                price: float
            f = {'symbol': 'symbol', 'other': 'val'}
            _ = f['price'], f.get('symbol', 'fb'), f.pop('price'), f.setdefault('symbol', 'df')
            _ = 'price' in f, 'symbol' not in f, 'price' == 'other', get('symbol')
            match f:
                case {'symbol': _, 'other': 'mv'}:
                    pass
        "},
        &[
            ("'symbol'", Inline),
            ("'other'", Inline),
            ("'val'", Inline),
            ("'fb'", Inline),
            ("'df'", Inline),
            ("'price'", Inline),
            ("'other'", Inline),
            ("'symbol'", Inline),
            ("'other'", Inline),
            ("'mv'", Inline),
        ]
    )]
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
    fn test_collect_literal_occurrences_python_skips_type_name_first_argument(
        #[values(
            "TypeVar",
            "NewType",
            "ParamSpec",
            "TypeVarTuple",
            "NamedTuple",
            "typing.TypedDict"
        )]
        callee: &str,
    ) {
        let file = ParsedFile::new(
            &format!("t = {callee}('Nm', 'A', bound='Bd')"),
            Language::Python,
        );
        let collected: Vec<String> = collect_literal_occurrences(&file)
            .into_iter()
            .map(|occurrence| occurrence.node.text().into_owned())
            .collect();
        assert_eq!(collected, vec!["'A'", "'Bd'"]);
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
