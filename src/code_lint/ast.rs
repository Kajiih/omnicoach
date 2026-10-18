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
    /// Byte span of the function's declared name identifier.
    pub name_span: SourceSpan,
    /// True if the function is defined at file scope: a statement of the Python module body or
    /// an item of the Rust source file (not nested in a class, `impl`, `mod`, or block).
    pub is_top_level: bool,
    /// Name of the immediately enclosing top-level Python `class`, if this function is a direct
    /// method of that class. `None` for module-level functions, nested functions, methods of
    /// nested classes, and all Rust functions.
    pub enclosing_class: Option<String>,
}

/// Returns `outer` (innermost first) with `function` prepended as the new innermost function.
pub(in crate::code_lint::ast) fn with_innermost_function(
    function: EnclosingFunction,
    outer: &[EnclosingFunction],
) -> Arc<[EnclosingFunction]> {
    std::iter::once(function)
        .chain(outer.iter().cloned())
        .collect()
}

/// A dotted name together with its last segment (`typing.List` and `List`).
#[derive(Clone)]
pub(in crate::code_lint::ast) struct QualifiedName {
    /// The full dotted or `::`-separated path.
    pub(in crate::code_lint::ast) path: String,
    /// The last segment of `path`.
    pub(in crate::code_lint::ast) terminal: String,
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
    is_in_loop: bool,
    enclosing_functions: Arc<[EnclosingFunction]>,
}

/// Cached call-cluster ordering finding without lifetime ties to `ParsedFile`.
#[derive(Clone)]
pub(in crate::code_lint::ast) struct CachedCallOrderFinding {
    span: SourceSpan,
    function: String,
    caller: String,
    anchor: PlacementAnchor,
}

/// Cached priority-ordered call-cluster findings for a file.
#[derive(Default)]
pub(in crate::code_lint::ast) struct CachedCallClusterFindings {
    uncolocated_helpers: Vec<CachedCallOrderFinding>,
    private_before_public: Vec<CachedCallOrderFinding>,
    callee_before_caller: Vec<CachedCallOrderFinding>,
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
    pub(in crate::code_lint::ast) call_cluster_findings: OnceLock<CachedCallClusterFindings>,
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
            call_cluster_findings: OnceLock::new(),
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

    /// Resolves the 1-indexed start `(line, column)` coordinate of this node.
    #[must_use]
    pub fn start_coordinate(&self) -> LineColumn {
        self.file.line_index.lookup(self.span.start)
    }
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

/// Returns true if `text` is a Rust outer or inner doc comment (`///`, `//!`, `/** */`, `/*! */`).
///
/// Per the Rust Reference, `////...`, `/***...`, and `/**/` are ordinary non-doc comments.
#[must_use]
pub fn is_rust_doc_comment(text: &str) -> bool {
    (text.starts_with("///") && !text.starts_with("////"))
        || text.starts_with("//!")
        || (text.starts_with("/**") && !text.starts_with("/***") && text != "/**/")
        || text.starts_with("/*!")
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
    /// True if the call is inside the condition or body of a `for`, `while`, or `loop`
    /// statement without an intervening function, lambda, or closure boundary.
    pub is_in_loop: bool,
    /// The functions enclosing the call, innermost first. A Python function encloses its
    /// decorators, parameters, and body; lambdas and Rust closures are not functions.
    pub enclosing_functions: Arc<[EnclosingFunction]>,
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
            is_in_loop: entry.is_in_loop,
            enclosing_functions: Arc::clone(&entry.enclosing_functions),
        })
        .collect()
}

/// Collects all direct call expressions in a Python file using `ruff_python_ast`.
fn collect_python_call_candidates(file: &ParsedFile) -> Vec<CachedCallCandidate> {
    use ruff_python_ast::visitor::source_order::{
        SourceOrderVisitor, walk_except_handler, walk_expr, walk_stmt, walk_with_item,
    };
    use ruff_python_ast::{ExceptHandler, Expr, Stmt, WithItem};

    struct CallVisitor<'a> {
        file: &'a ParsedFile,
        /// Span of the context-manager expression of the `with` item being visited.
        with_item_context_span: Option<SourceSpan>,
        /// True while inside an `except` handler of the current function scope.
        in_except_clause: bool,
        /// True while inside the condition or body of a `for` or `while` loop of the current scope.
        in_loop: bool,
        enclosing_functions: python::EnclosingFunctionTracker,
        out: Vec<CachedCallCandidate>,
    }

    impl<'a> SourceOrderVisitor<'a> for CallVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            let enclosing_except = self.in_except_clause;
            let enclosing_loop = self.in_loop;
            if matches!(statement, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
                self.in_except_clause = false;
            }
            if matches!(statement, Stmt::FunctionDef(_)) {
                self.in_loop = false;
            }
            let outer_functions = self.enclosing_functions.enter(statement);
            match statement {
                Stmt::For(for_stmt) => {
                    self.visit_expr(&for_stmt.target);
                    self.visit_expr(&for_stmt.iter);
                    self.in_loop = true;
                    self.visit_body(&for_stmt.body);
                    self.in_loop = enclosing_loop;
                    self.visit_body(&for_stmt.orelse);
                }
                Stmt::While(while_stmt) => {
                    self.in_loop = true;
                    self.visit_expr(&while_stmt.test);
                    self.visit_body(&while_stmt.body);
                    self.in_loop = enclosing_loop;
                    self.visit_body(&while_stmt.orelse);
                }
                _ => walk_stmt(self, statement),
            }
            self.enclosing_functions.exit(outer_functions);
            self.in_except_clause = enclosing_except;
            self.in_loop = enclosing_loop;
        }

        fn visit_expr(&mut self, expr: &'a Expr) {
            let enclosing_except = self.in_except_clause;
            let enclosing_loop = self.in_loop;
            match expr {
                Expr::Call(call) => {
                    let span = span_from_ruff_range(call.range());
                    let is_with_context_manager = self.with_item_context_span == Some(span);
                    self.out.push(build_python_call_candidate(
                        &self.file.source,
                        call,
                        span,
                        is_with_context_manager,
                        self.in_except_clause,
                        self.in_loop,
                        &self.enclosing_functions.functions,
                    ));
                }
                Expr::Lambda(_) => {
                    self.in_except_clause = false;
                    self.in_loop = false;
                }
                _ => {}
            }
            walk_expr(self, expr);
            self.in_except_clause = enclosing_except;
            self.in_loop = enclosing_loop;
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

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = CallVisitor {
        file,
        with_item_context_span: None,
        in_except_clause: false,
        in_loop: false,
        enclosing_functions: python::EnclosingFunctionTracker::default(),
        out: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.out
}

/// Builds a [`CachedCallCandidate`] for a Python call expression.
fn build_python_call_candidate(
    source: &str,
    call: &ruff_python_ast::ExprCall,
    span: SourceSpan,
    is_with_context_manager: bool,
    is_in_except_clause: bool,
    is_in_loop: bool,
    enclosing_functions: &Arc<[EnclosingFunction]>,
) -> CachedCallCandidate {
    use ruff_python_ast::Expr;

    let func_span = span_from_ruff_range(call.func.range());
    let callee = source[func_span.start..func_span.end].to_string();
    let (method_name, receiver_call_callee) = if let Expr::Attribute(attr) = call.func.as_ref() {
        let receiver_callee = if let Expr::Call(inner_call) = attr.value.as_ref() {
            let inner_span = span_from_ruff_range(inner_call.func.range());
            Some(source[inner_span.start..inner_span.end].to_string())
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
    CachedCallCandidate {
        span,
        callee,
        method_name,
        receiver_call_callee,
        argument_spans,
        is_with_context_manager,
        is_in_except_clause,
        is_in_loop,
        enclosing_functions: Arc::clone(enclosing_functions),
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
                is_in_loop: is_in_rust_loop(call.syntax()),
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
                is_in_loop: is_in_rust_loop(method_call.syntax()),
                enclosing_functions: Arc::clone(&enclosing_functions),
            });
        }
    }
    out
}

/// Returns true if `node` lies inside a Rust `loop`, `while`, or `for` body without an intervening
/// `fn` or closure boundary.
fn is_in_rust_loop(node: &ra_ap_syntax::SyntaxNode) -> bool {
    use ra_ap_syntax::ast::{self, HasLoopBody as _};

    let range = node.text_range();
    for ancestor in node.ancestors().skip(1) {
        if ast::Fn::can_cast(ancestor.kind()) || ast::ClosureExpr::can_cast(ancestor.kind()) {
            return false;
        }
        if ast::LoopExpr::can_cast(ancestor.kind()) || ast::WhileExpr::can_cast(ancestor.kind()) {
            return true;
        }
        if let Some(for_expr) = ast::ForExpr::cast(ancestor)
            && for_expr
                .loop_body()
                .is_some_and(|body| body.syntax().text_range().contains_range(range))
        {
            return true;
        }
    }
    false
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
    /// Returns the arithmetic negation of a number, or `None` for strings and overflow.
    pub(in crate::code_lint::ast) fn negated(&self) -> Option<Self> {
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

/// Collects control-flow and conditional constructs directly inside outermost test functions in `file`,
/// returning `(construct_node, test_function_name, construct_description)`.
#[must_use]
pub fn collect_test_logic_constructs(
    file: &ParsedFile,
) -> Vec<(AstNode<'_>, String, &'static str)> {
    dispatch_lang!(file.lang(), collect_test_logic_constructs(file))
}

/// Collects outermost test functions in `file` that interleave non-assertion statements between
/// behavioral assertions, returning `(function_name_node, test_function_name)`.
#[must_use]
pub fn collect_multi_phase_test_functions(file: &ParsedFile) -> Vec<(AstNode<'_>, String)> {
    dispatch_lang!(file.lang(), collect_multi_phase_test_functions(file))
}

/// A test function and its normalized leading non-assertion setup statements.
pub struct TestFunctionSetup<'a> {
    /// Identifier AST node of the test function declaration.
    pub name_node: AstNode<'a>,
    /// Name of the test function.
    pub name: String,
    /// Normalized source text of each leading non-assertion setup statement, in source order.
    pub leading_setup_statements: Vec<String>,
}

/// Collects test functions grouped by enclosing module, class, or `mod` scope (segmented on
/// duplicate function names) along with their leading non-assertion setup statements.
#[must_use]
pub fn collect_test_setup_scopes(file: &ParsedFile) -> Vec<Vec<TestFunctionSetup<'_>>> {
    dispatch_lang!(file.lang(), collect_test_setup_scopes(file))
}

/// A call inside a test file to another test function defined in the same file.
pub struct TestToTestCall<'a> {
    /// The call expression node.
    pub node: AstNode<'a>,
    /// The name of the enclosing function making the call.
    pub caller: String,
    /// The source callee text as written (`test_login`, `self.test_login`).
    pub callee: String,
}

/// Collects calls in `file` where a function calls a test function defined in the same file.
#[must_use]
pub fn collect_test_to_test_calls(file: &ParsedFile) -> Vec<TestToTestCall<'_>> {
    dispatch_lang!(file.lang(), collect_test_to_test_calls(file))
}

/// Normalizes a statement's source slice by trimming each line and joining with `\n`.
pub(in crate::code_lint::ast) fn normalize_statement_text(raw: &str) -> String {
    raw.lines().map(str::trim).collect::<Vec<_>>().join("\n")
}

/// Finds all multiline string literals in `file` that are not docstrings/doc-attributes/snapshots
/// and are not wrapped in an allowed dedent helper.
#[must_use]
pub fn find_unwrapped_multiline_strings(
    file: &ParsedFile,
    is_allowed_wrapper: impl Fn(&str) -> bool,
) -> Vec<AstNode<'_>> {
    dispatch_lang!(
        file.lang(),
        find_unwrapped_multiline_strings(file, is_allowed_wrapper)
    )
}

/// What a function returns, as far as its signature (or, for an unannotated Python function, its
/// own body) shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnShape {
    /// No value: Rust `()` or no return type (also as the success type of `Result` / `Poll`);
    /// Python `-> None`, or an unannotated function whose body neither returns a value nor yields.
    Unit,
    /// A boolean: `bool`, Python `TypeGuard[T]` / `TypeIs[T]`, and Rust `Result<bool, E>` /
    /// `Poll<bool>`.
    Boolean,
    /// A fixed-length tuple of at least two elements, alone or inside an optional, a union, a
    /// Rust `Result` / `Poll` / pointer wrapper, or a Python generator's yield type.
    PositionalTuple {
        /// The number of elements (the largest when several tuples are returned).
        arity: usize,
    },
    /// An unannotated Python function that returns a value, or whose body is a stub.
    Unknown,
    /// Any other return type.
    Other,
}

/// The return contract of a function or method whose signature its author chose.
///
/// Signatures imposed from outside are not collected: Python `@override`, `@overload`,
/// `@abstractmethod`, fixtures, `@register`, data-model dunders and `Protocol` / `ABC` members;
/// Rust trait items, trait implementations and inline tests.
#[derive(Clone)]
pub struct FunctionReturn<'a> {
    /// Function identifier name.
    pub name: String,
    /// Identifier AST node of the function declaration.
    pub name_node: AstNode<'a>,
    /// The declared return type (`-> T`), if any.
    pub declared_type: Option<AstNode<'a>>,
    /// What the function returns.
    pub shape: ReturnShape,
}

impl FunctionReturn<'_> {
    /// The declared return type as written, or the language's unit type (`None` in Python, `()`
    /// in Rust) when there is none.
    #[must_use]
    pub fn declared_type_text(&self) -> String {
        self.declared_type.map_or_else(
            || {
                match self.name_node.lang() {
                    Language::Python => "None",
                    Language::Rust => "()",
                }
                .to_owned()
            },
            |declared_type| declared_type.text().into_owned(),
        )
    }
}

/// Collects the return contract of every function and method in `file` whose signature its
/// author chose, in source order.
#[must_use]
pub fn collect_function_returns(file: &ParsedFile) -> Vec<FunctionReturn<'_>> {
    dispatch_lang!(file.lang(), collect_function_returns(file))
}

/// Verbs that read as a yes/no question alone or as a prefix, such as `text.contains(word)` or
/// `contains_key`.
const PREDICATE_VERBS: &[&str] = &["contains", "starts_with", "ends_with", "matches"];

/// Words that read as a yes/no question only as a prefix, such as `is_valid`: a bare `is`,
/// `requires` or `needs` names a value as often as a question (`Distribution.requires`).
const PREDICATE_PREFIXES: &[&str] = &[
    "is", "has", "can", "should", "supports", "allows", "requires", "needs",
];

/// Returns true if function name `name` reads as a yes/no question.
///
/// Without leading underscores, the name is `contains`, `starts_with`, `ends_with` or `matches`,
/// or one of those or `is`, `has`, `can`, `should`, `supports`, `allows`, `requires`, `needs`
/// followed by `_` and more (`is_valid`, `_has_key`, `contains_key`).
#[must_use]
pub fn is_predicate_name(name: &str) -> bool {
    let bare = name.trim_start_matches('_');
    PREDICATE_VERBS.contains(&bare)
        || PREDICATE_VERBS
            .iter()
            .chain(PREDICATE_PREFIXES)
            .any(|prefix| {
                bare.strip_prefix(prefix)
                    .and_then(|rest| rest.strip_prefix('_'))
                    .is_some_and(|rest| !rest.is_empty())
            })
}

/// Visibility tier of a method or associated function in a Python class or Rust inherent `impl`
/// block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodVisibility {
    /// Public or dunder (`__name__`) method in Python; exported (`pub`, `pub(crate)`, `pub(super)`,
    /// `pub(in ...)`) method or associated function in Rust.
    Public,
    /// Single-underscore (`_name`) or name-mangled (`__name`) method in Python; bare `fn` without
    /// a visibility qualifier in Rust.
    Private,
}

/// A direct method or associated function of a Python `class` or Rust inherent `impl` block.
#[derive(Clone)]
pub struct TypeMethod<'a> {
    /// Identifier AST node of the method declaration.
    pub name_node: AstNode<'a>,
    /// Method identifier name.
    pub name: String,
    /// 1-based first line of the method declaration (including decorators or attributes).
    pub start_line: usize,
    /// Visibility tier (`Public` vs. `Private`).
    pub visibility: MethodVisibility,
    /// True if this method is a lifecycle constructor in Python (`__prepare__`,
    /// `__init_subclass__`, `__new__`, `__init__`, `__post_init__`, `__attrs_pre_init__`,
    /// `__attrs_post_init__`) or an exported `new` / `try_new` / `new_*` / `try_new_*` associated
    /// function without a `self` receiver in a Rust inherent `impl` block.
    pub is_constructor: bool,
}

/// A Python `class` or Rust inherent `impl` block and its direct methods in source order.
pub struct TypeMethodScope<'a> {
    /// Name of the enclosing class or implemented type.
    pub type_name: String,
    /// Direct methods in source order (with Python `@overload` signatures and
    /// `@<prop>.setter` / `@<prop>.deleter` accessors grouped at their first declaration).
    pub methods: Vec<TypeMethod<'a>>,
}

/// Collects each Python `class` and Rust inherent `impl` block in `file` with its direct methods
/// in source order.
#[must_use]
pub fn collect_type_method_scopes(file: &ParsedFile) -> Vec<TypeMethodScope<'_>> {
    dispatch_lang!(file.lang(), collect_type_method_scopes(file))
}

/// Whether a [`ScopeItem`] is a callable (`fn` / `def`) or a type definition (`struct`, `enum`,
/// `union`, `trait`, `class`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeItemKind {
    /// A function or method definition.
    Callable,
    /// A class, struct, enum, union, or trait definition.
    Type,
}

/// A function, method, or type definition inside a module, Python `class`, or Rust inherent
/// `impl` scope.
#[derive(Clone)]
pub struct ScopeItem<'a> {
    /// Identifier AST node of the declaration.
    pub name_node: AstNode<'a>,
    /// Declaration identifier name.
    pub name: String,
    /// 1-based last line of the declaration (including contiguous overloads/accessors or inherent
    /// `impl` blocks).
    pub end_line: usize,
    /// Whether this item is a callable or a type definition.
    pub kind: ScopeItemKind,
    /// Visibility tier (`Public` vs. `Private`).
    pub visibility: MethodVisibility,
    /// True if this callable is a Tier 0 lifecycle constructor.
    pub is_constructor: bool,
    /// Indices in the enclosing [`DeclarationScope::items`] slice of all sibling items directly
    /// called or referenced by this item (signature + body).
    pub callees: Vec<usize>,
    /// Indices of sibling items referenced in this callable's parameter, generic, decorator, or
    /// `where`-clause signature.
    pub param_callees: Vec<usize>,
    /// Indices of sibling items referenced in this callable's return type signature.
    pub return_callees: Vec<usize>,
    /// Indices of sibling items referenced in this callable's body (or in a type definition /
    /// its attached `impl` blocks).
    pub body_callees: Vec<usize>,
    /// Smallest index in [`DeclarationScope::items`] before which this item must remain due to
    /// top-to-bottom runtime evaluation (e.g. Python import-time or class-definition-time uses).
    pub pinned_before: Option<usize>,
}

/// A module, Python `class`, or Rust inherent `impl` scope with its direct declarations in
/// source order and their intra-scope reference edges.
pub struct DeclarationScope<'a> {
    /// Direct declarations in source order.
    pub items: Vec<ScopeItem<'a>>,
}

/// Splits `declarations` into consecutive segments, starting a new segment whenever `name`
/// returns a name already declared in the current segment.
pub(in crate::code_lint::ast) fn split_at_redefinitions<T>(
    declarations: Vec<T>,
    name: impl Fn(&T) -> &str,
) -> Vec<Vec<T>> {
    let mut segments = Vec::new();
    let mut current_segment = Vec::new();
    let mut seen_names = HashSet::new();
    for declaration in declarations {
        if !seen_names.insert(name(&declaration).to_owned()) {
            segments.push(std::mem::take(&mut current_segment));
            seen_names.clear();
            seen_names.insert(name(&declaration).to_owned());
        }
        current_segment.push(declaration);
    }
    if !current_segment.is_empty() {
        segments.push(current_segment);
    }
    segments
}

/// A sibling declaration next to which a misplaced item belongs, along with its 1-based boundary
/// line number (its starting line when moving above it, or its ending line when moving below it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementAnchor {
    /// Identifier name of the anchor declaration (`{anchor}`).
    pub name: String,
    /// 1-based boundary line number of the anchor declaration (`{line}`).
    pub line: usize,
}

impl PlacementAnchor {
    /// Anchor for moving an item directly above `declaration_node` (records `start_line()`).
    #[must_use]
    pub fn before(name: impl Into<String>, declaration_node: AstNode<'_>) -> Self {
        Self {
            name: name.into(),
            line: declaration_node.start_line(),
        }
    }

    /// Anchor for moving an item immediately below a declaration ending at `end_line`.
    #[must_use]
    pub fn after(name: impl Into<String>, end_line: usize) -> Self {
        Self {
            name: name.into(),
            line: end_line,
        }
    }
}

/// A misplaced declaration reported by the call-cluster ordering analyzer.
pub struct CallOrderFinding<'a> {
    /// Identifier AST node of the misplaced declaration.
    pub name_node: AstNode<'a>,
    /// Name of the misplaced declaration (`{function}`).
    pub function: String,
    /// Name of the owning or calling declaration it relates to (`{caller}`).
    pub caller: String,
    /// Sibling declaration immediately below which `function` belongs.
    pub anchor: PlacementAnchor,
}

/// Priority-ordered findings for the three declaration-cluster ordering rules across a file:
/// 1. `private-before-public` (private helper above its public caller `pos < max(Roots(h))`,
///    or unrooted private declaration above `last_pub`)
/// 2. `uncolocated-helper` (private helper after its public callers `pos > max(Roots(h))` that is
///    neither in Place 1 immediately after its single consumer nor in Place 2 in the trailing
///    private helper section after `last_pub`)
/// 3. `callee-before-caller` (private callee before private caller among remaining items)
///
/// Evaluating all three together guarantees mutual exclusion so a single misplaced declaration
/// is never double-reported across rules.
pub struct CallClusterFindings<'a> {
    /// Findings for `uncolocated-helper` (private helpers after their public callers that are
    /// neither in Place 1 nor in Place 2).
    pub uncolocated_helpers: Vec<CallOrderFinding<'a>>,
    /// Findings for `private-before-public` (private helpers declared before their
    /// public callers).
    pub private_before_public: Vec<CallOrderFinding<'a>>,
    /// Findings for `callee-before-caller` (private callees declared before their private callers).
    pub callee_before_caller: Vec<CallOrderFinding<'a>>,
}

/// Computes and memoizes the declaration-cluster ordering findings (`private-before-public`,
/// `uncolocated-helper`, and `callee-before-caller`) across all module and class/`impl` scopes
/// in `file`.
#[must_use]
pub fn collect_call_cluster_findings(file: &ParsedFile) -> CallClusterFindings<'_> {
    let cached = file.call_cluster_findings.get_or_init(|| {
        let scopes = dispatch_lang!(file.lang(), collect_declaration_scopes(file));
        let mut out = CachedCallClusterFindings::default();
        for scope in &scopes {
            analyze_declaration_scope(scope, &mut out);
        }
        out
    });
    let materialize = |items: &[CachedCallOrderFinding]| -> Vec<CallOrderFinding<'_>> {
        items
            .iter()
            .map(|entry| CallOrderFinding {
                name_node: AstNode::from_span(file, entry.span),
                function: entry.function.clone(),
                caller: entry.caller.clone(),
                anchor: entry.anchor.clone(),
            })
            .collect()
    };
    CallClusterFindings {
        uncolocated_helpers: materialize(&cached.uncolocated_helpers),
        private_before_public: materialize(&cached.private_before_public),
        callee_before_caller: materialize(&cached.callee_before_caller),
    }
}

/// Returns true if `name` is an assertion helper function or method name (`assert_*` or `_assert_*`).
#[must_use]
pub(in crate::code_lint::ast) fn is_assertion_function_name(name: &str) -> bool {
    name.starts_with("assert_") || name.starts_with("_assert_")
}

/// Analyzes a single [`DeclarationScope`], resolves the target placement anchor for each finding,
/// and appends the findings to `out`.
fn analyze_declaration_scope(scope: &DeclarationScope<'_>, out: &mut CachedCallClusterFindings) {
    let mut raw = RawScopeFindings::default();
    collect_raw_scope_findings(scope, &mut raw);
    if raw.is_empty() {
        return;
    }
    let mut orig_flagged = vec![false; scope.items.len()];
    for finding in raw.all() {
        orig_flagged[finding.item_idx] = true;
    }
    let to_cached = |finding: &RawScopeFinding| -> CachedCallOrderFinding {
        let anchor_idx =
            find_best_anchor(scope, finding.item_idx, finding.caller_idx, &orig_flagged);
        let item = &scope.items[finding.item_idx];
        let anchor_item = &scope.items[anchor_idx];
        CachedCallOrderFinding {
            span: item.name_node.span(),
            function: item.name.clone(),
            caller: scope.items[finding.caller_idx].name.clone(),
            anchor: PlacementAnchor::after(&anchor_item.name, anchor_item.end_line),
        }
    };
    out.uncolocated_helpers
        .extend(raw.uncolocated_helpers.iter().map(to_cached));
    out.private_before_public
        .extend(raw.private_before_public.iter().map(to_cached));
    out.callee_before_caller
        .extend(raw.callee_before_caller.iter().map(to_cached));
}

/// Finds the index in `scope.items` immediately after which item `from` can be moved to clear its
/// finding without introducing new findings, preferring the slot closest to `from`.
fn find_best_anchor(
    scope: &DeclarationScope<'_>,
    from: usize,
    caller_idx: usize,
    orig_flagged: &[bool],
) -> usize {
    let count = scope.items.len();
    let pinned_before = scope.items[from].pinned_before;
    (0..count)
        .filter_map(|after| {
            if after == from || after + 1 == from || pinned_before.is_some_and(|pin| after >= pin) {
                return None;
            }
            let reordered = reorder_scope_after(scope, from, after);
            let mut candidate = RawScopeFindings::default();
            collect_raw_scope_findings(&reordered.scope, &mut candidate);
            if candidate
                .all()
                .any(|finding| reordered.orig_idx_of[finding.item_idx] == from)
            {
                return None;
            }
            let creates_new_finding = candidate
                .all()
                .any(|finding| !orig_flagged[reordered.orig_idx_of[finding.item_idx]]);
            let new_pos = if from < after { after } else { after + 1 };
            Some(CandidateSlot {
                creates_new_finding,
                distance: new_pos.abs_diff(from),
                new_pos,
                after,
            })
        })
        .min()
        .map_or(caller_idx, |slot| slot.after)
}

/// Ranking key for a candidate placement slot in [`find_best_anchor`], ordered by:
/// 1. whether moving the item introduces a new finding elsewhere in the scope,
/// 2. vertical distance (`|new_pos - from|`),
/// 3. earlier position in the scope (`new_pos`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CandidateSlot {
    creates_new_finding: bool,
    distance: usize,
    new_pos: usize,
    after: usize,
}

struct ReorderedScope<'a> {
    scope: DeclarationScope<'a>,
    orig_idx_of: Vec<usize>,
}

/// Returns a copy of `scope` with item `from` moved immediately after item `after`, paired with
/// the mapping from each reordered item index back to its original index in `scope.items`.
fn reorder_scope_after<'a>(
    scope: &DeclarationScope<'a>,
    from: usize,
    after: usize,
) -> ReorderedScope<'a> {
    let count = scope.items.len();
    let mut orig_idx_of: Vec<usize> = (0..count).filter(|&idx| idx != from).collect();
    let new_pos = if from < after { after } else { after + 1 };
    orig_idx_of.insert(new_pos, from);

    let mut new_idx_of = vec![0; count];
    for (new_idx, &orig_idx) in orig_idx_of.iter().enumerate() {
        new_idx_of[orig_idx] = new_idx;
    }

    let remap =
        |indices: &[usize]| -> Vec<usize> { indices.iter().map(|&idx| new_idx_of[idx]).collect() };
    let items = orig_idx_of
        .iter()
        .map(|&orig_idx| {
            let item = &scope.items[orig_idx];
            ScopeItem {
                name_node: item.name_node,
                name: item.name.clone(),
                end_line: item.end_line,
                kind: item.kind,
                visibility: item.visibility,
                is_constructor: item.is_constructor,
                callees: remap(&item.callees),
                param_callees: remap(&item.param_callees),
                return_callees: remap(&item.return_callees),
                body_callees: remap(&item.body_callees),
                pinned_before: item
                    .pinned_before
                    .map(|pin| new_idx_of.get(pin).copied().unwrap_or(pin)),
            }
        })
        .collect();

    ReorderedScope {
        scope: DeclarationScope { items },
        orig_idx_of,
    }
}

/// Index-based call-cluster findings for a single [`DeclarationScope`] before anchor resolution.
#[derive(Default)]
struct RawScopeFindings {
    uncolocated_helpers: Vec<RawScopeFinding>,
    private_before_public: Vec<RawScopeFinding>,
    callee_before_caller: Vec<RawScopeFinding>,
}

impl RawScopeFindings {
    const fn is_empty(&self) -> bool {
        self.uncolocated_helpers.is_empty()
            && self.private_before_public.is_empty()
            && self.callee_before_caller.is_empty()
    }

    fn all(&self) -> impl Iterator<Item = &RawScopeFinding> {
        self.uncolocated_helpers
            .iter()
            .chain(&self.private_before_public)
            .chain(&self.callee_before_caller)
    }
}

/// Evaluates the three call-cluster ordering stages on `scope` and appends raw findings to `out`.
fn collect_raw_scope_findings(scope: &DeclarationScope<'_>, out: &mut RawScopeFindings) {
    let callables = &scope.items;
    let count = callables.len();
    if count < 2 {
        return;
    }

    let mut callers_of = vec![Vec::new(); count];
    for (caller_idx, callable) in callables.iter().enumerate() {
        for &callee_idx in &callable.callees {
            if caller_idx != callee_idx
                && callee_idx < count
                && !callers_of[callee_idx].contains(&caller_idx)
            {
                callers_of[callee_idx].push(caller_idx);
            }
        }
    }

    let stage3_callees = compute_stage3_callees(callables);
    let scc_id = compute_tarjan_scc(callables, &stage3_callees);
    let roots_of = compute_public_roots(callables, &callers_of);

    let last_pub = (0..count)
        .rev()
        .find(|&idx| callables[idx].visibility == MethodVisibility::Public);
    let mut flagged_p1_or_p2 = vec![false; count];

    for pos in 0..count {
        if callables[pos].visibility == MethodVisibility::Public {
            continue;
        }
        let pinned_before = callables[pos].pinned_before;
        let roots = &roots_of[pos];
        let Some(&max_root) = roots.last() else {
            if let Some(last_pub_idx) = last_pub
                && pinned_before.is_none_or(|pin| pin > last_pub_idx)
                && pos < last_pub_idx
            {
                out.private_before_public.push(RawScopeFinding {
                    item_idx: pos,
                    caller_idx: last_pub_idx,
                });
                flagged_p1_or_p2[pos] = true;
            }
            continue;
        };
        if pos < max_root {
            if pinned_before.is_none_or(|pin| pin > max_root) {
                out.private_before_public.push(RawScopeFinding {
                    item_idx: pos,
                    caller_idx: max_root,
                });
                flagged_p1_or_p2[pos] = true;
            }
        } else if let Some(last_pub_idx) = last_pub
            && !is_valid_helper_placement(callables, &roots_of, pos, last_pub_idx)
            && (roots.len() == 1 || pinned_before.is_none_or(|pin| pin > last_pub_idx))
        {
            out.uncolocated_helpers.push(RawScopeFinding {
                item_idx: pos,
                caller_idx: max_root,
            });
            flagged_p1_or_p2[pos] = true;
        }
    }

    for callee in 0..count {
        if flagged_p1_or_p2[callee] || callables[callee].visibility == MethodVisibility::Public {
            continue;
        }
        let pinned_before = callables[callee].pinned_before;
        let last_caller = ((callee + 1)..count)
            .filter(|&caller| {
                !flagged_p1_or_p2[caller]
                    && callables[caller].visibility == MethodVisibility::Private
                    && stage3_callees[caller].contains(&callee)
                    && scc_id[caller] != scc_id[callee]
                    && pinned_before.is_none_or(|pin| pin > caller)
                    // Do not let an unrooted private caller pull a rooted helper below its
                    // public root's cluster.
                    && (!roots_of[caller].is_empty() || roots_of[callee].is_empty())
            })
            .max();
        if let Some(caller_idx) = last_caller {
            out.callee_before_caller.push(RawScopeFinding {
                item_idx: callee,
                caller_idx,
            });
        }
    }
}

#[derive(Clone, Copy)]
struct RawScopeFinding {
    item_idx: usize,
    caller_idx: usize,
}

/// Computes the Stage-3 (`callee-before-caller`) downward abstraction edges for each item in
/// `callables`:
/// - `Callable -> Callable`, `Type -> Type`, and `Type -> Callable` include all `callees`.
/// - `Callable -> Type` includes a type `callee` iff `caller` encapsulates `callee` inside its
///   body (`EffectiveBody(caller) \ SigClosure(caller)`).
/// - `Type -> Callable` additionally includes every callable whose parameter or return signature
///   names the type directly: the type is the vocabulary of that callable's contract.
fn compute_stage3_callees(callables: &[ScopeItem<'_>]) -> Vec<Vec<usize>> {
    let count = callables.len();
    let mut out = vec![Vec::new(); count];
    for (caller, item) in callables.iter().enumerate() {
        if item.kind == ScopeItemKind::Type {
            out[caller] = item
                .callees
                .iter()
                .copied()
                .filter(|&callee| callee != caller && callee < count)
                .collect();
            continue;
        }

        let mut sig_closure = vec![false; count];
        let mut stack: Vec<usize> = item
            .param_callees
            .iter()
            .chain(&item.return_callees)
            .copied()
            .filter(|&idx| idx < count && callables[idx].kind == ScopeItemKind::Type)
            .collect();
        while let Some(cur) = stack.pop() {
            if sig_closure[cur] {
                continue;
            }
            sig_closure[cur] = true;
            for &next in &callables[cur].callees {
                if next < count && callables[next].kind == ScopeItemKind::Type {
                    stack.push(next);
                }
            }
        }

        let mut edges = Vec::new();
        for &callee in &item.callees {
            if callee == caller || callee >= count {
                continue;
            }
            if callables[callee].kind == ScopeItemKind::Callable {
                edges.push(callee);
            }
        }
        for callee in 0..count {
            if callee == caller
                || callables[callee].kind != ScopeItemKind::Type
                || sig_closure[callee]
            {
                continue;
            }
            let in_body = item.body_callees.contains(&callee);
            let in_inferred_return = item.body_callees.iter().any(|&callee_fn| {
                callee_fn < count
                    && callables[callee_fn].kind == ScopeItemKind::Callable
                    && callables[callee_fn].return_callees.contains(&callee)
            });
            if in_body || in_inferred_return {
                edges.push(callee);
            }
        }
        out[caller] = edges;
    }
    for (callable, item) in callables.iter().enumerate() {
        if item.kind != ScopeItemKind::Callable {
            continue;
        }
        for &sig_type in item.param_callees.iter().chain(&item.return_callees) {
            if sig_type < count
                && callables[sig_type].kind == ScopeItemKind::Type
                && !out[sig_type].contains(&callable)
            {
                out[sig_type].push(callable);
            }
        }
    }
    out
}

/// Returns true if a private helper at `pos` (already after all of its `roots_of[pos]`) is
/// either immediately after its single consumer or in the trailing helper section after
/// `last_pub_idx`.
fn is_valid_helper_placement(
    callables: &[ScopeItem<'_>],
    roots_of: &[Vec<usize>],
    pos: usize,
    last_pub_idx: usize,
) -> bool {
    let roots = &roots_of[pos];
    if roots.len() != 1 {
        return pos > last_pub_idx;
    }
    let owner = roots[0];
    let in_owner_cluster = |idx: usize| -> bool {
        roots_of[idx] == *roots
            || (callables[owner].is_constructor
                && (callables[idx].is_constructor
                    || (roots_of[idx].len() == 1 && callables[roots_of[idx][0]].is_constructor)))
    };
    let is_just_after_owner = ((owner + 1)..pos).all(in_owner_cluster);
    let is_at_scope_end =
        pos > last_pub_idx && ((owner + 1)..last_pub_idx).all(|mid| roots_of[mid] != *roots);
    is_just_after_owner || is_at_scope_end
}

/// Computes `Roots(f)` for each item in `callables`: the sorted indices of public entrypoints
/// that reach `f` through private reference paths (stopping at public boundaries).
fn compute_public_roots(callables: &[ScopeItem<'_>], callers_of: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let count = callables.len();
    let mut roots_of = vec![Vec::new(); count];
    for idx in 0..count {
        if callables[idx].visibility == MethodVisibility::Public {
            roots_of[idx] = vec![idx];
        } else {
            let mut visited = vec![false; count];
            let mut stack = vec![idx];
            let mut pub_roots = Vec::new();
            while let Some(cur) = stack.pop() {
                if visited[cur] {
                    continue;
                }
                visited[cur] = true;
                for &caller in &callers_of[cur] {
                    if callables[caller].visibility == MethodVisibility::Public {
                        pub_roots.push(caller);
                    } else {
                        stack.push(caller);
                    }
                }
            }
            pub_roots.sort_unstable();
            pub_roots.dedup();
            roots_of[idx] = pub_roots;
        }
    }
    roots_of
}

/// Computes Strongly Connected Component IDs for `callables` using Tarjan's algorithm over
/// private-to-private Stage-3 edges (stopping at public boundaries).
fn compute_tarjan_scc(callables: &[ScopeItem<'_>], stage3_callees: &[Vec<usize>]) -> Vec<usize> {
    struct TarjanState {
        next_index: usize,
        next_scc: usize,
        indices: Vec<Option<usize>>,
        lowlink: Vec<usize>,
        on_stack: Vec<bool>,
        stack: Vec<usize>,
        scc_id: Vec<usize>,
    }

    impl TarjanState {
        fn strongconnect(
            &mut self,
            node: usize,
            callables: &[ScopeItem<'_>],
            stage3_callees: &[Vec<usize>],
        ) {
            let current_index = self.next_index;
            self.next_index += 1;
            self.indices[node] = Some(current_index);
            self.lowlink[node] = current_index;
            self.stack.push(node);
            self.on_stack[node] = true;

            if callables[node].visibility == MethodVisibility::Private {
                for &callee in &stage3_callees[node] {
                    if callee >= callables.len()
                        || callables[callee].visibility == MethodVisibility::Public
                    {
                        continue;
                    }
                    if self.indices[callee].is_none() {
                        self.strongconnect(callee, callables, stage3_callees);
                        self.lowlink[node] = self.lowlink[node].min(self.lowlink[callee]);
                    } else if self.on_stack[callee]
                        && let Some(callee_index) = self.indices[callee]
                    {
                        self.lowlink[node] = self.lowlink[node].min(callee_index);
                    }
                }
            }

            if Some(self.lowlink[node]) == self.indices[node] {
                let id = self.next_scc;
                self.next_scc += 1;
                while let Some(member) = self.stack.pop() {
                    self.on_stack[member] = false;
                    self.scc_id[member] = id;
                    if member == node {
                        break;
                    }
                }
            }
        }
    }

    let count = callables.len();
    let mut state = TarjanState {
        next_index: 0,
        next_scc: 0,
        indices: vec![None; count],
        lowlink: vec![0; count],
        on_stack: vec![false; count],
        stack: Vec::new(),
        scc_id: vec![0; count],
    };
    for node in 0..count {
        if state.indices[node].is_none() {
            state.strongconnect(node, callables, stage3_callees);
        }
    }
    state.scc_id
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

    #[test]
    fn test_field_after_method_placement_anchor() {
        let field_file = ParsedFile::new(
            indoc::indoc! {r"
                class Config:
                    @classmethod
                    def from_env(cls) -> 'Config':
                        return cls()

                    retries: int = 3
            "},
            Language::Python,
        );
        let fields = python::collect_fields_after_methods(&field_file);
        assert_eq!(fields.len(), 1);
        assert_eq!(
            fields[0].first_method,
            PlacementAnchor {
                name: "from_env".to_string(),
                line: 2,
            }
        );
    }

    #[test]
    fn test_inline_attribute_annotation_placement_anchor() {
        let attribute_file = ParsedFile::new(
            indoc::indoc! {r"
                class Client:
                    @classmethod
                    def default_port(cls) -> int:
                        return 80

                    def __init__(self, host: str) -> None:
                        self.host: str = host
            "},
            Language::Python,
        );
        let attributes = python::collect_instance_attribute_annotations(&attribute_file);
        assert_eq!(attributes.len(), 1);
        assert_eq!(
            attributes[0].first_method,
            PlacementAnchor {
                name: "default_port".to_string(),
                line: 2,
            }
        );
    }

    #[test]
    fn test_type_method_scope_start_line_includes_doc_comment() {
        let constructor_file = ParsedFile::new(
            indoc::indoc! {r"
                pub struct Connection {
                    port: u16,
                }

                impl Connection {
                    /// Returns the configured port.
                    pub fn port(&self) -> u16 {
                        self.port
                    }

                    pub fn new(port: u16) -> Self {
                        Self { port }
                    }
                }
            "},
            Language::Rust,
        );
        let scopes = collect_type_method_scopes(&constructor_file);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].methods[0].name, "port");
        assert_eq!(scopes[0].methods[0].start_line, 6);
    }

    #[test]
    fn test_associated_item_after_method_placement_anchor() {
        let associated_file = ParsedFile::new(
            indoc::indoc! {r"
                pub trait Decoder {
                    /// Decodes an input frame.
                    fn decode(&self, input: &str) -> Self::Output;

                    type Output;
                }
            "},
            Language::Rust,
        );
        let associated_items = rust::collect_associated_items_after_methods(&associated_file);
        assert_eq!(associated_items.len(), 1);
        assert_eq!(
            associated_items[0].first_fn,
            PlacementAnchor {
                name: "decode".to_string(),
                line: 2,
            }
        );
    }

    #[test]
    fn test_statement_after_main_guard_line() {
        let guard_file = ParsedFile::new(
            indoc::indoc! {r#"
                DEFAULT_PORT = 8080

                if __name__ == "__main__":
                    print(DEFAULT_PORT)

                EXTRA = 1
            "#},
            Language::Python,
        );
        let after_guard = python::collect_statements_after_main_guard(&guard_file);
        assert_eq!(after_guard.len(), 1);
        assert_eq!(after_guard[0].guard_line, 3);
    }

    #[rstest::rstest]
    #[case::uncolocated_place_1(
        Language::Python,
        indoc::indoc! {r"
            def render_header(title: str) -> str:
                return _format_title(title)

            def render_body(body: str) -> str:
                return body.strip()

            def _format_title(title: str) -> str:
                return title.upper()

            def render_footer(footer: str) -> str:
                return footer.lower()
        "},
        ("_format_title", "render_header", "render_header", 2)
    )]
    #[case::uncolocated_place_2_split_cluster(
        Language::Python,
        indoc::indoc! {r"
            def build_report(raw: str) -> str:
                return _parse_section(raw) + _format_section(raw)

            def _parse_section(raw: str) -> str:
                return raw.strip()

            def build_summary(raw: str) -> str:
                return raw.lower()

            def _format_section(raw: str) -> str:
                return raw.upper()
        "},
        ("_format_section", "build_report", "_parse_section", 5)
    )]
    #[case::uncolocated_shared_helper(
        Language::Python,
        indoc::indoc! {r"
            def export_csv(rows: list[str]) -> str:
                return _join_lines(rows)

            def export_tsv(rows: list[str]) -> str:
                return _join_lines(rows)

            def _join_lines(rows: list[str]) -> str:
                return '\n'.join(rows)

            def export_count(rows: list[str]) -> int:
                return len(rows)
        "},
        ("_join_lines", "export_tsv", "export_count", 11)
    )]
    #[case::private_before_public_rust_struct_impl(
        Language::Rust,
        indoc::indoc! {r"
            fn compute_hash(input: &str) -> usize {
                input.len()
            }

            pub struct Hasher;

            impl Hasher {
                pub fn hash(&self, input: &str) -> usize {
                    compute_hash(input)
                }
            }
        "},
        ("compute_hash", "Hasher", "Hasher", 11)
    )]
    #[case::callee_before_caller_chain(
        Language::Python,
        indoc::indoc! {r"
            def run_pipeline(raw: str) -> str:
                return _step_one(raw)

            def _step_two(text: str) -> str:
                return text.lower()

            def _step_one(raw: str) -> str:
                return _step_two(raw.strip())
        "},
        ("_step_two", "_step_one", "_step_one", 8)
    )]
    fn test_call_cluster_placement_anchors(
        #[case] lang: Language,
        #[case] source: &str,
        #[case] expected: (&str, &str, &str, usize),
    ) {
        let file = ParsedFile::new(source, lang);
        let findings = collect_call_cluster_findings(&file);
        let all: Vec<_> = findings
            .uncolocated_helpers
            .iter()
            .chain(&findings.private_before_public)
            .chain(&findings.callee_before_caller)
            .map(|finding| {
                (
                    finding.function.as_str(),
                    finding.caller.as_str(),
                    finding.anchor.name.as_str(),
                    finding.anchor.line,
                )
            })
            .collect();
        assert_eq!(all, vec![expected]);
    }
}
