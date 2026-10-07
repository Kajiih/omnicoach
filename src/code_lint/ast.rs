//! Dedicated AST and CST parser encapsulation.
//!
//! Encapsulates `ruff_python_parser` / `ruff_python_ast` (Python) and `ra_ap_syntax` (Rust) behind
//! [`ParsedFile`] and an opaque [`AstNode`], and provides language-dispatched and language-specific
//! syntax extractors. No module outside `crate::code_lint::ast` imports parser crates directly.

architecture_component!(CodeLintAst);

/// Dispatches `$func(args...)` to `ast::python` or `ast::rust` by `$lang`.
macro_rules! dispatch_lang {
    ($lang:expr, $func:ident ( $($arg:expr),* $(,)? )) => {
        match $lang {
            $crate::diagnostic::Language::Python => {
                $crate::code_lint::ast::python::$func($($arg),*)
            }
            $crate::diagnostic::Language::Rust => {
                $crate::code_lint::ast::rust::$func($($arg),*)
            }
        }
    };
}

mod imports;
pub mod python;
pub mod rust;
pub mod statements;

pub use imports::{ResolvedName, resolve_name};

use crate::diagnostic::{Language, LineColumn, LineIndex, SourceLocation, SourceSpan};
use ra_ap_syntax::AstNode as _;
use ruff_text_size::Ranged;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

/// A function enclosing a collected node, recorded by the walk that collected the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnclosingFunction {
    /// The function's name.
    pub name: String,
    /// True if the function is defined at file scope: a statement of the Python module body or
    /// an item of the Rust source file (not nested in a class, `impl`, `mod`, or block).
    pub is_top_level: bool,
}

/// Returns `outer` (innermost first) with `function` prepended as the new innermost function.
fn with_innermost_function(
    function: EnclosingFunction,
    outer: &[EnclosingFunction],
) -> Arc<[EnclosingFunction]> {
    std::iter::once(function)
        .chain(outer.iter().cloned())
        .collect()
}

/// Cached call expression metadata without lifetime ties to `ParsedFile`.
pub(in crate::code_lint::ast) struct CachedCallCandidate {
    span: SourceSpan,
    callee: String,
    method_name: Option<String>,
    receiver_call_callee: Option<String>,
    argument_spans: Vec<SourceSpan>,
    is_with_context_manager: bool,
    is_in_except_clause: bool,
    enclosing_functions: Arc<[EnclosingFunction]>,
}

/// Dedicated language-specific parsed syntax tree.
pub(in crate::code_lint::ast) enum CodeLintAst {
    /// Parsed Python module (`ruff_python_parser`).
    Python(
        Result<
            ruff_python_parser::Parsed<ruff_python_ast::ModModule>,
            ruff_python_parser::ParseError,
        >,
    ),
    /// Parsed Rust source file (`ra_ap_syntax`).
    Rust(ra_ap_syntax::Parse<ra_ap_syntax::ast::SourceFile>),
}

/// A parsed source file encapsulating the language and syntax tree.
///
/// The inner syntax tree is restricted to `crate::code_lint::ast` so that higher layers
/// (semantic engines, rule traits, and lint rules) interact strictly through typed AST helpers.
/// File-level queries shared across multiple rules or semantic engines are memoized via `OnceLock`.
pub struct ParsedFile {
    pub(in crate::code_lint::ast) source: String,
    pub(in crate::code_lint::ast) line_index: LineIndex,
    pub(in crate::code_lint::ast) ast: CodeLintAst,
    lang: Language,
    pub(in crate::code_lint::ast) comment_spans: OnceLock<Vec<SourceSpan>>,
    pub(in crate::code_lint::ast) bindings: OnceLock<Vec<(SourceSpan, BindingKind)>>,
    pub(in crate::code_lint::ast) call_candidates: OnceLock<Vec<CachedCallCandidate>>,
    pub(in crate::code_lint::ast) rust_inline_test_ranges: OnceLock<Vec<std::ops::Range<usize>>>,
    pub(in crate::code_lint::ast) imports: OnceLock<imports::ImportMap>,
    pub(in crate::code_lint::ast) locally_mutated_return_functions: OnceLock<HashSet<String>>,
}

impl ParsedFile {
    /// Parses `source` into a syntax tree for `lang`.
    #[must_use]
    pub fn new(source: &str, lang: Language) -> Self {
        let ast = match lang {
            Language::Python => CodeLintAst::Python(ruff_python_parser::parse_module(source)),
            Language::Rust => CodeLintAst::Rust(ra_ap_syntax::SourceFile::parse(
                source,
                ra_ap_syntax::Edition::Edition2024,
            )),
        };
        Self {
            source: source.to_string(),
            line_index: LineIndex::new(source),
            ast,
            lang,
            comment_spans: OnceLock::new(),
            bindings: OnceLock::new(),
            call_candidates: OnceLock::new(),
            rust_inline_test_ranges: OnceLock::new(),
            imports: OnceLock::new(),
            locally_mutated_return_functions: OnceLock::new(),
        }
    }

    /// Parses `source` into a Rust syntax tree.
    #[must_use]
    pub fn rust(source: &str) -> Self {
        Self::new(source, Language::Rust)
    }

    /// Returns the programming language of this parsed file.
    #[must_use]
    pub const fn lang(&self) -> Language {
        self.lang
    }

    /// Returns the full source text of the file.
    #[must_use]
    pub fn source_text(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(&self.source)
    }

    /// Returns whether the parser had to recover from invalid or missing syntax.
    #[must_use]
    pub fn has_syntax_error(&self) -> bool {
        match &self.ast {
            CodeLintAst::Python(Ok(parsed)) => !parsed.errors().is_empty(),
            CodeLintAst::Python(Err(_)) => true,
            CodeLintAst::Rust(parsed) => !parsed.errors().is_empty(),
        }
    }

    /// Returns true if `offset` lies inside a Rust `#[cfg(test)]` or `#[test]` item.
    #[must_use]
    pub fn is_in_rust_inline_test(&self, offset: usize) -> bool {
        self.lang == Language::Rust
            && rust::collect_inline_test_ranges(self)
                .iter()
                .any(|range| range.contains(&offset))
    }

    /// Returns the parsed Python module if this is a Python file without fatal parse failure.
    #[must_use]
    pub(in crate::code_lint::ast) const fn py_module(
        &self,
    ) -> Option<&ruff_python_parser::Parsed<ruff_python_ast::ModModule>> {
        match &self.ast {
            CodeLintAst::Python(Ok(parsed)) => Some(parsed),
            CodeLintAst::Python(Err(_)) | CodeLintAst::Rust(_) => None,
        }
    }

    /// Returns the parsed Rust syntax tree if this is a Rust file.
    #[must_use]
    pub(in crate::code_lint::ast) const fn rs_parsed(
        &self,
    ) -> Option<&ra_ap_syntax::Parse<ra_ap_syntax::ast::SourceFile>> {
        match &self.ast {
            CodeLintAst::Rust(parsed) => Some(parsed),
            CodeLintAst::Python(_) => None,
        }
    }
}

/// An opaque syntax tree node exposing source text and span coordinates without leaking
/// low-level grammar vocabulary.
#[derive(Clone, Copy)]
pub struct AstNode<'a> {
    pub(in crate::code_lint::ast) file: &'a ParsedFile,
    pub(in crate::code_lint::ast) span: SourceSpan,
}

impl<'a> AstNode<'a> {
    #[must_use]
    pub(in crate::code_lint::ast) const fn from_span(
        file: &'a ParsedFile,
        span: SourceSpan,
    ) -> Self {
        Self { file, span }
    }

    /// Returns the source text slice spanned by this node.
    #[must_use]
    pub fn text(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(&self.file.source[self.span.start..self.span.end])
    }

    /// Returns the programming language of the file containing this node.
    #[must_use]
    pub const fn lang(&self) -> Language {
        self.file.lang()
    }

    /// Returns the [`SourceSpan`] (byte range) of this node.
    #[must_use]
    pub const fn span(&self) -> SourceSpan {
        self.span
    }

    /// Returns the 1-indexed starting line number of this node.
    #[must_use]
    pub fn start_line(&self) -> usize {
        self.file.line_index.line(self.span.start)
    }

    /// Returns the 1-indexed ending line number of this node.
    #[must_use]
    pub fn end_line(&self) -> usize {
        self.file.line_index.line(self.span.end)
    }

    /// Resolves the 1-indexed start `(line, column)` coordinate of this node.
    #[must_use]
    pub fn start_coordinate(&self) -> LineColumn {
        self.file.line_index.lookup(self.span.start)
    }

    /// Returns true if this node starts inside a Rust `#[cfg(test)]` or `#[test]` item.
    #[must_use]
    pub fn is_in_rust_inline_test(&self) -> bool {
        self.file.is_in_rust_inline_test(self.span.start)
    }

    /// Constructs a [`SourceLocation`] for this node inside the file at `path`.
    #[must_use]
    pub fn to_source_location(&self, path: impl Into<PathBuf>) -> SourceLocation {
        SourceLocation::file_span(path, self.span, self.start_coordinate())
    }
}

/// Converts a `ruff_text_size::TextRange` into a [`SourceSpan`].
#[must_use]
pub(in crate::code_lint::ast) const fn span_from_ruff_range(
    range: ruff_text_size::TextRange,
) -> SourceSpan {
    SourceSpan::new(range.start().to_usize(), range.end().to_usize())
}

/// Converts a `ra_ap_syntax::TextRange` into a [`SourceSpan`].
#[must_use]
pub(in crate::code_lint::ast) fn span_from_rowan_range(
    range: ra_ap_syntax::TextRange,
) -> SourceSpan {
    SourceSpan::new(range.start().into(), range.end().into())
}

/// Returns true if `kind` is a Rust comment token (`//`, `/* */`, `///`, `//!`, `/** */`, `/*! */`).
#[must_use]
pub(in crate::code_lint::ast) const fn is_rust_comment_kind(
    kind: ra_ap_syntax::SyntaxKind,
) -> bool {
    use ra_ap_syntax::SyntaxKind;
    matches!(
        kind,
        SyntaxKind::COMMENT | SyntaxKind::OUTER_DOC_COMMENT | SyntaxKind::INNER_DOC_COMMENT
    )
}

/// Collects all comment nodes in `file` in source order.
///
/// Uses `ruff_python_ast::token::TokenKind::Comment` for Python and `is_rust_comment_kind`
/// for Rust so non-comment trivia (such as Python line continuations) is never included.
#[must_use]
pub fn collect_comment_nodes(file: &ParsedFile) -> Vec<AstNode<'_>> {
    let spans = file.comment_spans.get_or_init(|| match &file.ast {
        CodeLintAst::Python(Ok(parsed)) => parsed
            .tokens()
            .iter()
            .filter(|token| token.kind() == ruff_python_ast::token::TokenKind::Comment)
            .map(|token| span_from_ruff_range(token.range()))
            .collect(),
        CodeLintAst::Python(Err(_)) => Vec::new(),
        CodeLintAst::Rust(parsed) => parsed
            .tree()
            .syntax()
            .descendants_with_tokens()
            .filter_map(|element| {
                let token = element.into_token()?;
                is_rust_comment_kind(token.kind())
                    .then(|| span_from_rowan_range(token.text_range()))
            })
            .collect(),
    });
    spans
        .iter()
        .map(|&span| AstNode::from_span(file, span))
        .collect()
}

/// Candidate call expression extracted from the syntax tree.
pub struct AstCallCandidate<'a> {
    /// The call expression AST node.
    pub node: AstNode<'a>,
    /// Full source text of the invoked function/callee expression.
    pub callee: String,
    /// Terminal method identifier text if the callee is a method access (e.g. `obj.method`).
    pub method_name: Option<String>,
    /// If the receiver of a method call is itself a call expression (e.g. `get_loop().create_task(coro)`),
    /// the callee text of that receiver call (e.g. `"get_loop"` or `"asyncio.get_running_loop"`).
    pub receiver_call_callee: Option<String>,
    /// Semantic argument nodes passed to the call.
    pub arguments: Vec<AstNode<'a>>,
    /// True if the call is the context-manager expression of a Python `with` item (`with
    /// suppress(KeyError):`). Always false in Rust.
    pub is_with_context_manager: bool,
    /// True if the call is inside a Python `except` handler without an intervening `def`,
    /// `class`, or `lambda` boundary. Always false in Rust.
    pub is_in_except_clause: bool,
    /// The functions enclosing the call, innermost first. A Python function encloses its
    /// decorators, parameters, and body; lambdas and Rust closures are not functions.
    pub enclosing_functions: Arc<[EnclosingFunction]>,
}

/// Collects all direct call expressions in a Python file using `ruff_python_ast`.
fn collect_python_call_candidates(file: &ParsedFile) -> Vec<CachedCallCandidate> {
    use ruff_python_ast::visitor::source_order::{
        SourceOrderVisitor, walk_except_handler, walk_expr, walk_stmt, walk_with_item,
    };
    use ruff_python_ast::{ExceptHandler, Expr, ExprCall, Stmt, WithItem};

    struct CallVisitor<'a> {
        file: &'a ParsedFile,
        /// Span of the context-manager expression of the `with` item being visited.
        with_item_context_span: Option<SourceSpan>,
        /// True while inside an `except` handler of the current function scope.
        in_except_clause: bool,
        enclosing_functions: python::EnclosingFunctionTracker,
        out: Vec<CachedCallCandidate>,
    }

    impl<'a> SourceOrderVisitor<'a> for CallVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            let enclosing = self.in_except_clause;
            if matches!(statement, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
                self.in_except_clause = false;
            }
            let outer_functions = self.enclosing_functions.enter(statement);
            walk_stmt(self, statement);
            self.enclosing_functions.exit(outer_functions);
            self.in_except_clause = enclosing;
        }

        fn visit_expr(&mut self, expr: &'a Expr) {
            let enclosing = self.in_except_clause;
            match expr {
                Expr::Call(call) => self.record_call(call),
                Expr::Lambda(_) => self.in_except_clause = false,
                _ => {}
            }
            walk_expr(self, expr);
            self.in_except_clause = enclosing;
        }

        fn visit_with_item(&mut self, with_item: &'a WithItem) {
            let enclosing = self.with_item_context_span;
            self.with_item_context_span =
                Some(span_from_ruff_range(with_item.context_expr.range()));
            walk_with_item(self, with_item);
            self.with_item_context_span = enclosing;
        }

        fn visit_except_handler(&mut self, except_handler: &'a ExceptHandler) {
            let enclosing = self.in_except_clause;
            self.in_except_clause = true;
            walk_except_handler(self, except_handler);
            self.in_except_clause = enclosing;
        }
    }

    impl CallVisitor<'_> {
        fn record_call(&mut self, call: &ExprCall) {
            let span = span_from_ruff_range(call.range());
            let func_span = span_from_ruff_range(call.func.range());
            let callee = self.file.source[func_span.start..func_span.end].to_string();
            let (method_name, receiver_call_callee) =
                if let Expr::Attribute(attr) = call.func.as_ref() {
                    let receiver_callee = if let Expr::Call(inner_call) = attr.value.as_ref() {
                        let inner_span = span_from_ruff_range(inner_call.func.range());
                        Some(self.file.source[inner_span.start..inner_span.end].to_string())
                    } else {
                        None
                    };
                    (Some(attr.attr.to_string()), receiver_callee)
                } else {
                    (None, None)
                };
            let mut argument_spans: Vec<SourceSpan> = call
                .arguments
                .args
                .iter()
                .map(|arg| span_from_ruff_range(arg.range()))
                .chain(
                    call.arguments
                        .keywords
                        .iter()
                        .map(|kw| span_from_ruff_range(kw.range())),
                )
                .collect();
            argument_spans.sort_by_key(|span| (span.start, span.end));
            self.out.push(CachedCallCandidate {
                span,
                callee,
                method_name,
                receiver_call_callee,
                argument_spans,
                is_with_context_manager: self.with_item_context_span == Some(span),
                is_in_except_clause: self.in_except_clause,
                enclosing_functions: Arc::clone(&self.enclosing_functions.functions),
            });
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = CallVisitor {
        file,
        with_item_context_span: None,
        in_except_clause: false,
        enclosing_functions: python::EnclosingFunctionTracker::default(),
        out: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.out
}

/// Extracts the callee string of a Rust call or method-call expression.
fn rust_call_callee_text(expr: &ra_ap_syntax::ast::Expr, source: &str) -> Option<String> {
    match expr {
        ra_ap_syntax::ast::Expr::CallExpr(call) => {
            let func = call.expr()?;
            let span = span_from_rowan_range(func.syntax().text_range());
            Some(source[span.start..span.end].to_string())
        }
        ra_ap_syntax::ast::Expr::MethodCallExpr(method_call) => {
            let receiver = method_call.receiver()?;
            let name_ref = method_call.name_ref()?;
            let start: usize = receiver.syntax().text_range().start().into();
            let end: usize = name_ref.syntax().text_range().end().into();
            Some(source[start..end].to_string())
        }
        _ => None,
    }
}

/// Collects all direct call and method-call expressions in a Rust file using `ra_ap_syntax`.
fn collect_rust_call_candidates(file: &ParsedFile) -> Vec<CachedCallCandidate> {
    use ra_ap_syntax::ast::{self, HasArgList as _};

    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut enclosing_functions: Arc<[EnclosingFunction]> = Arc::default();
    let mut outer_functions = Vec::new();
    for event in parsed.tree().syntax().preorder() {
        let syntax_node = match event {
            ra_ap_syntax::WalkEvent::Enter(syntax_node) => syntax_node,
            ra_ap_syntax::WalkEvent::Leave(syntax_node) => {
                if rust::named_function(&syntax_node).is_some()
                    && let Some(outer) = outer_functions.pop()
                {
                    enclosing_functions = outer;
                }
                continue;
            }
        };
        if let Some(function) = rust::named_function(&syntax_node) {
            let inner = with_innermost_function(function, &enclosing_functions);
            outer_functions.push(std::mem::replace(&mut enclosing_functions, inner));
        }
        if let Some(call) = ast::CallExpr::cast(syntax_node.clone()) {
            let Some(func) = call.expr() else {
                continue;
            };
            let func_span = span_from_rowan_range(func.syntax().text_range());
            let callee = file.source[func_span.start..func_span.end].to_string();
            let argument_spans = call.arg_list().map_or_else(Vec::new, |arguments| {
                arguments
                    .args()
                    .map(|arg| span_from_rowan_range(arg.syntax().text_range()))
                    .collect()
            });
            out.push(CachedCallCandidate {
                span: span_from_rowan_range(call.syntax().text_range()),
                callee,
                method_name: None,
                receiver_call_callee: None,
                argument_spans,
                is_with_context_manager: false,
                is_in_except_clause: false,
                enclosing_functions: Arc::clone(&enclosing_functions),
            });
        } else if let Some(method_call) = ast::MethodCallExpr::cast(syntax_node) {
            let (Some(receiver), Some(name_ref)) = (method_call.receiver(), method_call.name_ref())
            else {
                continue;
            };
            let start: usize = receiver.syntax().text_range().start().into();
            let end: usize = name_ref.syntax().text_range().end().into();
            let callee = file.source[start..end].to_string();
            let method_name = Some(name_ref.text().to_string());
            let receiver_call_callee = rust_call_callee_text(&receiver, &file.source);
            let argument_spans = method_call.arg_list().map_or_else(Vec::new, |arguments| {
                arguments
                    .args()
                    .map(|arg| span_from_rowan_range(arg.syntax().text_range()))
                    .collect()
            });
            out.push(CachedCallCandidate {
                span: span_from_rowan_range(method_call.syntax().text_range()),
                callee,
                method_name,
                receiver_call_callee,
                argument_spans,
                is_with_context_manager: false,
                is_in_except_clause: false,
                enclosing_functions: Arc::clone(&enclosing_functions),
            });
        }
    }
    out
}

/// Collects all direct call expressions in `file` along with their callee text, optional method
/// target name, and semantic argument nodes.
#[must_use]
pub fn collect_call_candidates(file: &ParsedFile) -> Vec<AstCallCandidate<'_>> {
    let cached = file.call_candidates.get_or_init(|| match file.lang() {
        Language::Python => collect_python_call_candidates(file),
        Language::Rust => collect_rust_call_candidates(file),
    });
    cached
        .iter()
        .map(|entry| AstCallCandidate {
            node: AstNode::from_span(file, entry.span),
            callee: entry.callee.clone(),
            method_name: entry.method_name.clone(),
            receiver_call_callee: entry.receiver_call_callee.clone(),
            arguments: entry
                .argument_spans
                .iter()
                .map(|&span| AstNode::from_span(file, span))
                .collect(),
            is_with_context_manager: entry.is_with_context_manager,
            is_in_except_clause: entry.is_in_except_clause,
            enclosing_functions: Arc::clone(&entry.enclosing_functions),
        })
        .collect()
}

/// A read of one positional element through an integer literal: `receiver[1]` or
/// `receiver[-1]` in Python, `receiver.1` in Rust.
pub struct PositionalRead<'a> {
    /// The whole indexing expression (`point[0]`, `span.0`).
    pub node: AstNode<'a>,
    /// Source text of the indexed value (`point`, `self.pair`, `rows[i]`).
    pub receiver: String,
    /// The literal position; negative for Python end-relative indices.
    pub position: i64,
}

/// Positional reads of one scope (a function body, or the Python module top level), in source
/// order.
#[derive(Default)]
pub struct ScopePositionalReads<'a> {
    /// Literal-position reads whose receiver contains no call.
    pub reads: Vec<PositionalRead<'a>>,
    /// Receivers for which unpacking would not be equivalent. Python: receivers the scope writes
    /// to or uses as a collection (iterated, sized, indexed by a non-literal, sliced, mutated).
    /// Rust: receivers with a field assigned or mutably borrowed.
    pub exempt_receivers: HashSet<String>,
}

/// Collects positional reads in `file` grouped by scope.
///
/// There is one group per function body, plus the Python module top level. Python lambdas and
/// class bodies, and Rust items outside functions, are not collected; Rust closures belong to
/// their enclosing function.
#[must_use]
pub fn collect_positional_reads(file: &ParsedFile) -> Vec<ScopePositionalReads<'_>> {
    dispatch_lang!(file.lang(), collect_positional_reads(file))
}

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
    /// Builds a float value, storing `-0.0` as `0.0` so both spellings compare equal.
    fn float(value: f64) -> Self {
        Self::Float(if value == 0.0 { 0.0_f64 } else { value }.to_bits())
    }

    /// Returns the arithmetic negation of a number, or `None` for strings and overflow.
    pub(in crate::code_lint::ast) fn negated(&self) -> Option<Self> {
        match self {
            Self::Int(value) => value.checked_neg().map(Self::Int),
            Self::Float(bits) => Some(Self::float(-f64::from_bits(*bits))),
            Self::Str(_) | Self::Bytes(_) => None,
        }
    }
}

/// Whether a literal defines a named constant or is used inline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiteralRole {
    /// The whole value of a named scalar constant (`MAX_RETRIES = 3`, `const JJ: &str = "jj";`).
    ConstantDefinition,
    /// Any other position.
    Inline,
}

/// A literal that could be replaced by a named constant.
pub struct LiteralOccurrence<'a> {
    /// The literal, or for a negative number the node spanning its sign and digits.
    pub node: AstNode<'a>,
    /// The normalized value.
    pub value: LiteralValue,
    /// Whether the literal defines a constant or is used inline.
    pub role: LiteralRole,
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
pub(in crate::code_lint::ast) fn parse_integer_literal(text: &str) -> Option<LiteralValue> {
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
pub(in crate::code_lint::ast) fn parse_float_literal(text: &str) -> Option<LiteralValue> {
    text.replace('_', "").parse().ok().map(LiteralValue::float)
}

/// What introduced a binding name, recorded by the walk that collects the binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingKind {
    /// A name introduced by an import (`import x`, `from m import y`, `use a::b`).
    Import,
    /// The name of a member mandated by a contract: a function, type alias, or constant defined
    /// in a Rust `impl Trait for Type`, or a Python method decorated with `@override`.
    ContractMember,
    /// The declared name of a function, class, struct, enum, trait, or type alias that is not a
    /// [`Self::ContractMember`].
    StructuralDefinition,
    /// Any other binding: a variable, parameter, constant, static, field, or attribute.
    Value,
}

/// A binding definition name and what introduced it.
#[derive(Clone, Copy)]
pub struct Binding<'a> {
    /// The binding identifier node.
    pub node: AstNode<'a>,
    /// What introduced the binding.
    pub kind: BindingKind,
}

/// Appends each node that `extract` collects to `bindings` as a binding of `kind`.
pub(in crate::code_lint::ast) fn push_bindings<'a>(
    bindings: &mut Vec<Binding<'a>>,
    kind: BindingKind,
    extract: impl FnOnce(&mut Vec<AstNode<'a>>),
) {
    let mut nodes = Vec::new();
    extract(&mut nodes);
    bindings.extend(nodes.into_iter().map(|node| Binding { node, kind }));
}

/// Collects all binding definitions from a parsed file, in source order.
#[must_use]
pub fn collect_bindings(file: &ParsedFile) -> Vec<Binding<'_>> {
    let cached = file.bindings.get_or_init(|| {
        dispatch_lang!(file.lang(), collect_bindings(file))
            .into_iter()
            .map(|binding| (binding.node.span(), binding.kind))
            .collect()
    });
    cached
        .iter()
        .map(|&(span, kind)| Binding {
            node: AstNode::from_span(file, span),
            kind,
        })
        .collect()
}

/// Collects all outermost test functions in `file` along with their identifier node, name, and assertion count.
#[must_use]
pub fn collect_test_function_assertion_counts(
    file: &ParsedFile,
) -> Vec<(AstNode<'_>, String, usize)> {
    dispatch_lang!(file.lang(), collect_test_function_assertion_counts(file))
}

/// Finds all multiline string literals in `file` that are not docstrings/doc-attributes/snapshots
/// and are not wrapped in an allowed dedent helper.
#[must_use]
pub fn find_unwrapped_multiline_strings(
    file: &ParsedFile,
    is_allowed_wrapper: impl Fn(&str, &str) -> bool,
) -> Vec<AstNode<'_>> {
    dispatch_lang!(
        file.lang(),
        find_unwrapped_multiline_strings(file, is_allowed_wrapper)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_location_from_node() {
        let source = "fn main() {\n    let value = 42;\n}\n";
        let file = ParsedFile::new(source, Language::Rust);
        let node = AstNode::from_span(&file, SourceSpan::new(16, 31));
        assert_eq!(
            node.to_source_location("src/main.rs"),
            SourceLocation::file_span(
                "src/main.rs",
                SourceSpan::new(16, 31),
                LineColumn { line: 2, column: 5 },
            )
        );
    }

    /// Call arguments exclude punctuation and trivia such as comments.
    #[rstest::rstest]
    #[case::python(
        indoc::indoc! {r"
            sleep(
                # comment before arg
                0,  # inline comment
            )
        "},
        Language::Python,
        "0"
    )]
    #[case::rust(
        indoc::indoc! {r"
            fn f() {
                sleep(
                    /* block comment */
                    Duration::ZERO, // line comment
                );
            }
        "},
        Language::Rust,
        "Duration::ZERO"
    )]
    fn test_call_argument_nodes_excludes_comments_and_trivia(
        #[case] source: &str,
        #[case] lang: Language,
        #[case] expected_argument: &str,
    ) {
        let file = ParsedFile::new(source, lang);
        let argument_texts: Vec<Vec<String>> = collect_call_candidates(&file)
            .iter()
            .map(|call| {
                call.arguments
                    .iter()
                    .map(|argument| argument.text().into_owned())
                    .collect()
            })
            .collect();
        assert_eq!(argument_texts, vec![vec![expected_argument.to_owned()]]);
    }
}
