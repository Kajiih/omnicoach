//! Syntax-tree representation and opaque node handles: [`ParsedFile`], [`AstNode`], byte-span
//! conversions, and language dispatch.

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
pub(crate) use dispatch_lang;

use super::bindings::BindingKind;
use super::call_clusters::CachedCallOrderFinding;
use super::calls::CachedCallCandidate;
use super::imports::ImportMap;
use crate::diagnostic::{Language, LineColumn, LineIndex, SourceLocation, SourceSpan};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::OnceLock;

/// A parsed source file encapsulating the language and syntax tree.
///
/// The inner syntax tree is restricted to `crate::code_lint::ast` so that higher layers
/// (semantic engines, rule traits, and lint rules) interact strictly through typed AST helpers.
/// File-level queries shared across multiple rules or semantic engines are memoized via `OnceLock`.
pub struct ParsedFile {
    pub(super) source: String,
    pub(super) line_index: LineIndex,
    ast: CodeLintAst,
    lang: Language,
    pub(super) comment_spans: OnceLock<Vec<SourceSpan>>,
    pub(super) bindings: OnceLock<Vec<(SourceSpan, BindingKind)>>,
    pub(super) call_candidates: OnceLock<Vec<CachedCallCandidate>>,
    pub(super) rust_inline_test_ranges: OnceLock<Vec<std::ops::Range<usize>>>,
    pub(super) imports: OnceLock<ImportMap>,
    pub(super) locally_mutated_return_functions: OnceLock<HashSet<String>>,
    pub(super) call_cluster_findings: OnceLock<Vec<CachedCallOrderFinding>>,
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

    /// Returns the parsed Python module if this is a Python file without fatal parse failure.
    #[must_use]
    pub(super) const fn py_module(
        &self,
    ) -> Option<&ruff_python_parser::Parsed<ruff_python_ast::ModModule>> {
        match &self.ast {
            CodeLintAst::Python(Ok(parsed)) => Some(parsed),
            CodeLintAst::Python(Err(_)) | CodeLintAst::Rust(_) => None,
        }
    }

    /// Returns the parsed Rust syntax tree if this is a Rust file.
    #[must_use]
    pub(super) const fn rs_parsed(
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
    pub(super) file: &'a ParsedFile,
    pub(super) span: SourceSpan,
}

impl<'a> AstNode<'a> {
    #[must_use]
    pub(super) const fn from_span(file: &'a ParsedFile, span: SourceSpan) -> Self {
        Self { file, span }
    }

    /// Returns the source text slice spanned by this node.
    #[must_use]
    pub(crate) fn text(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(&self.file.source[self.span.start..self.span.end])
    }

    /// Returns the programming language of the file containing this node.
    #[must_use]
    pub(in crate::code_lint) const fn lang(&self) -> Language {
        self.file.lang()
    }

    /// Returns the [`SourceSpan`] (byte range) of this node.
    #[must_use]
    pub(crate) const fn span(&self) -> SourceSpan {
        self.span
    }

    /// Returns the 1-indexed starting line number of this node.
    #[must_use]
    pub(in crate::code_lint) fn start_line(&self) -> usize {
        self.file.line_index.line(self.span.start)
    }

    /// Returns the 1-indexed ending line number of this node.
    #[must_use]
    pub(in crate::code_lint) fn end_line(&self) -> usize {
        self.file.line_index.line(self.span.end)
    }

    /// Constructs a [`SourceLocation`] for this node inside the file at `path`.
    #[must_use]
    pub(in crate::code_lint) fn to_source_location(
        self,
        path: impl Into<PathBuf>,
    ) -> SourceLocation {
        SourceLocation::file_span(path, self.span, self.start_coordinate())
    }

    /// Resolves the 1-indexed start `(line, column)` coordinate of this node.
    #[must_use]
    pub(in crate::code_lint) fn start_coordinate(&self) -> LineColumn {
        self.file.line_index.lookup(self.span.start)
    }
}

/// Converts a `ruff_text_size::TextRange` into a [`SourceSpan`].
#[must_use]
pub(super) const fn span_from_ruff_range(range: ruff_text_size::TextRange) -> SourceSpan {
    SourceSpan::new(range.start().to_usize(), range.end().to_usize())
}

/// Converts a `ra_ap_syntax::TextRange` into a [`SourceSpan`].
#[must_use]
pub(super) fn span_from_rowan_range(range: ra_ap_syntax::TextRange) -> SourceSpan {
    SourceSpan::new(range.start().into(), range.end().into())
}

/// Dedicated language-specific parsed syntax tree.
enum CodeLintAst {
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
}
