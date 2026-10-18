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

mod call_clusters;
mod imports;
pub mod python;
pub mod rust;
mod test_analysis;

pub(in crate::code_lint::ast) use self::call_clusters::{
    CachedCallClusterFindings, CachedCallOrderFinding, DeclarationScope, DeclarationScopeKind,
    ScopeItem, ScopeItemKind, extend_unique, push_unique, split_at_redefinitions,
};
pub use self::call_clusters::{
    CallClusterFindings, CallOrderFinding, MemberAfterMethod, MethodVisibility, PlacementAnchor,
    TypeMethod, TypeMethodScope,
};
pub use self::test_analysis::{
    AssertionCheck, MultiPhaseTest, TestAssertionCount, TestFunctionSetup, TestLogicConstruct,
    TestToTestCall,
};
pub(in crate::code_lint::ast) use self::test_analysis::{
    TestStatementRole, has_multi_phase_interleaving, is_assertion_function_name,
    normalize_statement_text, resolve_assertion_helpers,
};
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

/// Syntactic context surrounding a call expression, shared across [`AstCallCandidate`] and
/// [`crate::code_lint::semantic::calls::CallMatch`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallSiteContext {
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

/// Cached call expression metadata without lifetime ties to `ParsedFile`.
pub(in crate::code_lint::ast) struct CachedCallCandidate {
    pub(in crate::code_lint::ast) span: SourceSpan,
    pub(in crate::code_lint::ast) callee: String,
    pub(in crate::code_lint::ast) method_name: Option<String>,
    pub(in crate::code_lint::ast) receiver_call_callee: Option<String>,
    pub(in crate::code_lint::ast) argument_spans: Vec<SourceSpan>,
    pub(in crate::code_lint::ast) context: CallSiteContext,
}

impl std::ops::Deref for CachedCallCandidate {
    type Target = CallSiteContext;

    fn deref(&self) -> &Self::Target {
        &self.context
    }
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
    /// Syntactic context surrounding the call expression.
    pub context: CallSiteContext,
}

impl std::ops::Deref for AstCallCandidate<'_> {
    type Target = CallSiteContext;

    fn deref(&self) -> &Self::Target {
        &self.context
    }
}

/// Collects all direct call expressions in `file` along with their callee text, optional method
/// target name, and semantic argument nodes.
#[must_use]
pub fn collect_call_candidates(file: &ParsedFile) -> Vec<AstCallCandidate<'_>> {
    let cached = file
        .call_candidates
        .get_or_init(|| dispatch_lang!(file.lang(), collect_call_candidates(file)));
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
            context: entry.context.clone(),
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
pub fn collect_test_function_assertion_counts(file: &ParsedFile) -> Vec<TestAssertionCount<'_>> {
    dispatch_lang!(file.lang(), collect_test_function_assertion_counts(file))
}

/// Collects control-flow and conditional constructs directly inside outermost test functions in `file`.
#[must_use]
pub fn collect_test_logic_constructs(file: &ParsedFile) -> Vec<TestLogicConstruct<'_>> {
    dispatch_lang!(file.lang(), collect_test_logic_constructs(file))
}

/// Collects outermost test functions in `file` that interleave non-assertion statements between
/// behavioral assertions.
#[must_use]
pub fn collect_multi_phase_test_functions(file: &ParsedFile) -> Vec<MultiPhaseTest<'_>> {
    dispatch_lang!(file.lang(), collect_multi_phase_test_functions(file))
}

/// Collects all Python `assert` statements and Rust assertion macro invocations in `file`
/// with their condition-packing properties.
#[must_use]
pub fn collect_assertion_checks(file: &ParsedFile) -> Vec<AssertionCheck<'_>> {
    dispatch_lang!(file.lang(), collect_assertion_checks(file))
}

/// Collects test functions grouped by enclosing module, class, or `mod` scope (segmented on
/// duplicate function names) along with their leading non-assertion setup statements.
#[must_use]
pub fn collect_test_setup_scopes(file: &ParsedFile) -> Vec<Vec<TestFunctionSetup<'_>>> {
    dispatch_lang!(file.lang(), collect_test_setup_scopes(file))
}

/// Collects calls in `file` where a function calls a test function defined in the same file.
#[must_use]
pub fn collect_test_to_test_calls(file: &ParsedFile) -> Vec<TestToTestCall<'_>> {
    dispatch_lang!(file.lang(), collect_test_to_test_calls(file))
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

/// Collects all module and class/inherent-`impl` [`DeclarationScope`]s in `file`.
#[must_use]
pub(in crate::code_lint::ast) fn collect_declaration_scopes(
    file: &ParsedFile,
) -> Vec<DeclarationScope<'_>> {
    dispatch_lang!(file.lang(), collect_declaration_scopes(file))
}

/// Collects each Python `class` and Rust inherent `impl` block in `file` with its direct methods
/// in source order.
#[must_use]
pub fn collect_type_method_scopes(file: &ParsedFile) -> Vec<TypeMethodScope<'_>> {
    collect_declaration_scopes(file)
        .into_iter()
        .filter_map(|scope| {
            let type_name = match scope.kind {
                DeclarationScopeKind::Class { name }
                | DeclarationScopeKind::Impl { type_name: name } => name,
                DeclarationScopeKind::Module => return None,
            };
            let methods = scope
                .items
                .into_iter()
                .map(|item| TypeMethod {
                    name_node: item.name_node,
                    name: item.name,
                    start_line: item.start_line,
                    visibility: item.visibility,
                    is_constructor: item.is_constructor,
                })
                .collect();
            Some(TypeMethodScope { type_name, methods })
        })
        .collect()
}

/// Collects non-method members (type-annotated Python class/instance attributes or Rust
/// associated `type` / `const` items) declared after a method in the same `class`, `impl`, or
/// `trait` body.
#[must_use]
pub fn collect_members_after_methods(file: &ParsedFile) -> Vec<MemberAfterMethod<'_>> {
    dispatch_lang!(file.lang(), collect_members_after_methods(file))
}

/// Computes and memoizes the declaration-cluster ordering findings (`private-before-public`,
/// `uncolocated-helper`, and `callee-before-caller`) across all module and class/`impl` scopes
/// in `file`.
#[must_use]
pub fn collect_call_cluster_findings(file: &ParsedFile) -> CallClusterFindings<'_> {
    let cached = file.call_cluster_findings.get_or_init(|| {
        let scopes = collect_declaration_scopes(file);
        let mut out = CachedCallClusterFindings::default();
        for scope in &scopes {
            call_clusters::analyze_declaration_scope(scope, &mut out);
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

/// Resolves the 1-indexed inclusive header line range of the innermost statement enclosing `span`.
#[must_use]
pub fn enclosing_statement_header_range(
    file: &ParsedFile,
    span: SourceSpan,
) -> Option<std::ops::RangeInclusive<usize>> {
    dispatch_lang!(file.lang(), enclosing_statement_header_range(file, span))
}

/// Maps each line where a decorator or attribute begins to the last line it spans.
///
/// Lines are 1-indexed. Covers Python `@decorator` and Rust `#[...]` attributes; when several
/// start on the same line, the largest ending line is kept.
#[must_use]
pub fn decorator_and_attribute_end_lines(
    file: &ParsedFile,
) -> std::collections::HashMap<usize, usize> {
    let mut end_lines = std::collections::HashMap::new();
    let mut record = |start: usize, end: usize| {
        let start_line = file.line_index.line(start);
        let end_line = file.line_index.line(end);
        end_lines
            .entry(start_line)
            .and_modify(|longest: &mut usize| *longest = (*longest).max(end_line))
            .or_insert(end_line);
    };
    dispatch_lang!(
        file.lang(),
        record_decorator_and_attribute_spans(file, &mut record)
    );
    end_lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::RangeInclusive;

    /// Finds the byte span of the first occurrence of `target` in `source`.
    fn span_of(source: &str, target: &str) -> SourceSpan {
        let start = source
            .find(target)
            .expect("target snippet should exist in source");
        SourceSpan::new(start, start + target.len())
    }

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
        let fields = collect_members_after_methods(&field_file);
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
        let associated_items = collect_members_after_methods(&associated_file);
        assert_eq!(associated_items.len(), 1);
        assert_eq!(
            associated_items[0].first_method,
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

    #[rstest::rstest]
    #[case::python_multiline_with(
        indoc::indoc! {r#"
            with (
                open("file.txt"),
                suppress(FileNotFoundError),
            ):
                pass
        "#},
        Language::Python, "suppress(FileNotFoundError)", 1..=4
    )]
    #[case::python_if(
        indoc::indoc! {r"
            if (
                ready()
            ):
                pass
        "},
        Language::Python, "ready()", 1..=3
    )]
    #[case::python_for(
        indoc::indoc! {r"
            for item in fetch():
                pass
        "},
        Language::Python, "fetch()", 1..=1
    )]
    #[case::python_try(
        indoc::indoc! {r"
            try:
                run()
            except ValueError:
                pass
        "},
        Language::Python, "run()", 2..=2
    )]
    #[case::python_multiline_assignment(
        indoc::indoc! {r#"
            CONFIG = (
                get_config("FALLBACK")
            )
        "#},
        Language::Python, "get_config(\"FALLBACK\")", 1..=3
    )]
    #[case::python_annotated_assignment(
        indoc::indoc! {r"
            CONFIG: dict[str, str] = build()
        "},
        Language::Python, "build()", 1..=1
    )]
    #[case::python_decorated_function(
        indoc::indoc! {r"
            @staticmethod
            def build(
                items,
            ) -> list[str]:
                return []
        "},
        Language::Python, "list[str]", 1..=4
    )]
    #[case::rust_let_with_block_value(
        indoc::indoc! {r"
            fn f() {
                let x = {
                    compute()
                };
            }
        "},
        Language::Rust, "{\n        compute()\n    }", 2..=2
    )]
    #[case::rust_simple_let(
        indoc::indoc! {r"
            fn f() {
                let x = compute();
            }
        "},
        Language::Rust, "compute()", 2..=2
    )]
    #[case::rust_const_item(
        indoc::indoc! {r"
            const LIMIT: usize = compute();
        "},
        Language::Rust, "compute()", 1..=1
    )]
    #[case::rust_attributed_function(
        indoc::indoc! {r"
            /// Doc comment line 1.
            ///
            /// Doc comment line 3.
            #[must_use]
            pub fn compute_items(input: &str) -> usize {
                input.len()
            }
        "},
        Language::Rust, "usize", 1..=5
    )]
    fn test_statement_and_header_resolution(
        #[case] source: &str,
        #[case] lang: Language,
        #[case] target: &str,
        #[case] expected_header: RangeInclusive<usize>,
    ) {
        let file = ParsedFile::new(source, lang);
        let span = span_of(source, target);
        assert_eq!(
            enclosing_statement_header_range(&file, span),
            Some(expected_header)
        );
    }

    #[rstest::rstest]
    #[case::python(
        indoc::indoc! {r"
            with suppress(Exception):
                cleanup()
        "},
        Language::Python, "cleanup()", 2..=2
    )]
    #[case::rust(
        indoc::indoc! {r"
            fn f() {
                let x = {
                    compute()
                };
            }
        "},
        Language::Rust, "compute()", 3..=3
    )]
    fn test_node_in_body_resolves_to_inner_statement_not_outer(
        #[case] source: &str,
        #[case] lang: Language,
        #[case] target: &str,
        #[case] expected_header: RangeInclusive<usize>,
    ) {
        let file = ParsedFile::new(source, lang);
        let span = span_of(source, target);
        assert_eq!(
            enclosing_statement_header_range(&file, span),
            Some(expected_header)
        );
    }

    #[test]
    fn test_comment_between_header_and_body_does_not_extend_header() {
        let source = indoc::indoc! {r"
            with suppress(Exception):
                # Explanation that belongs to the body, not the header
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let span = span_of(source, "suppress(Exception)");
        assert_eq!(enclosing_statement_header_range(&file, span), Some(1..=1));
    }
}
