//! Diagnostic representation, serialization, and reporting.

use crate::core::AstNode;
pub use crate::core::RuleName;
use ast_grep_language::SupportLang;
use serde::Serialize;
use std::path::PathBuf;

/// Detailed explanation and description of a rule violation.
#[derive(Debug, Serialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ViolationMessage {
    /// A short summary of the violation.
    pub summary: String,
    /// The rationale or explanation of why the rule was triggered.
    pub rationale: String,
    /// A suggestion or workaround to resolve the violation.
    pub suggestion: String,
}

impl ViolationMessage {
    /// Constructs a `ViolationMessage` from string-like values.
    #[must_use]
    pub fn new(
        summary: impl Into<String>,
        rationale: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self {
            summary: summary.into(),
            rationale: rationale.into(),
            suggestion: suggestion.into(),
        }
    }
}

/// A text string that defines a default baseline and language-specific overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageText {
    /// Default text used when no language-specific override matches.
    pub base: &'static str,
    /// Language-specific overrides.
    pub overrides: &'static [(SupportLang, &'static str)],
}

impl LanguageText {
    /// Creates a new `LanguageText` with a base string and language-specific overrides.
    #[must_use]
    pub const fn new(
        base: &'static str,
        overrides: &'static [(SupportLang, &'static str)],
    ) -> Self {
        Self { base, overrides }
    }

    /// Creates a `LanguageText` with no language overrides.
    #[must_use]
    pub const fn from_static(base: &'static str) -> Self {
        Self {
            base,
            overrides: &[],
        }
    }

    /// Resolves the raw static text for the given language.
    #[must_use]
    pub fn resolve_for_lang(&self, lang: SupportLang) -> &'static str {
        for &(override_lang, text) in self.overrides {
            if override_lang == lang {
                return text;
            }
        }
        self.base
    }

    /// Interpolates named `{key}` placeholders on the base text (for language-independent rules).
    #[must_use]
    pub fn render(&self, params: &[(&str, &str)]) -> String {
        Self::interpolate(self.base, params)
    }

    /// Resolves and interpolates named `{key}` placeholders for the given language.
    #[must_use]
    pub fn render_for_lang(&self, lang: SupportLang, params: &[(&str, &str)]) -> String {
        Self::interpolate(self.resolve_for_lang(lang), params)
    }

    fn interpolate(template: &str, params: &[(&str, &str)]) -> String {
        let mut result = template.to_string();
        for &(placeholder, replacement) in params {
            let pattern = format!("{{{placeholder}}}");
            result = result.replace(&pattern, replacement);
        }
        result
    }
}

/// A declarative template for constructing `ViolationMessage`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViolationTemplate {
    /// Summary template.
    pub summary: LanguageText,
    /// Rationale template.
    pub rationale: LanguageText,
    /// Suggestion template.
    pub suggestion: LanguageText,
}

impl ViolationTemplate {
    /// Creates a new `ViolationTemplate`.
    #[must_use]
    pub const fn new(
        summary: LanguageText,
        rationale: LanguageText,
        suggestion: LanguageText,
    ) -> Self {
        Self {
            summary,
            rationale,
            suggestion,
        }
    }

    /// Creates a template with uniform static text and no language overrides.
    #[must_use]
    pub const fn from_static(
        summary: &'static str,
        rationale: &'static str,
        suggestion: &'static str,
    ) -> Self {
        Self {
            summary: LanguageText::from_static(summary),
            rationale: LanguageText::from_static(rationale),
            suggestion: LanguageText::from_static(suggestion),
        }
    }

    /// Renders the base template into a concrete `ViolationMessage` (for command/language-independent rules).
    #[must_use]
    pub fn render(&self, params: &[(&str, &str)]) -> ViolationMessage {
        ViolationMessage {
            summary: self.summary.render(params),
            rationale: self.rationale.render(params),
            suggestion: self.suggestion.render(params),
        }
    }

    /// Renders the template into a concrete `ViolationMessage` for a specific programming language.
    #[must_use]
    pub fn render_for_lang(&self, lang: SupportLang, params: &[(&str, &str)]) -> ViolationMessage {
        ViolationMessage {
            summary: self.summary.render_for_lang(lang, params),
            rationale: self.rationale.render_for_lang(lang, params),
            suggestion: self.suggestion.render_for_lang(lang, params),
        }
    }
}

/// Declaratively constructs a `const` [`ViolationTemplate`].
///
/// Fields can either be static strings or blocks containing `base:` and per-language overrides:
/// ```rust,ignore
/// violation_template! {
///     summary: "Test function `{func}` has {count} assertions...",
///     rationale: "Tests with too many assertions often verify multiple unrelated behaviors...",
///     suggestion: {
///         base: "Parameterize test variations...",
///         Python => "Use `@pytest.mark.parametrize`...",
///         Rust => "Use `#[rstest]`...",
///     },
/// }
/// ```
#[macro_export]
macro_rules! violation_template {
    // Internal arm: field with language overrides
    (@text { base: $base:expr, $($lang:ident => $text:expr),+ $(,)? }) => {
        $crate::diagnostic::LanguageText::new(
            $base,
            &[$((::ast_grep_language::SupportLang::$lang, $text)),+],
        )
    };
    // Internal arm: static string field
    (@text $base:expr) => {
        $crate::diagnostic::LanguageText::from_static($base)
    };

    (
        summary: $summary:tt,
        rationale: $rationale:tt,
        suggestion: $suggestion:tt $(,)?
    ) => {
        $crate::diagnostic::ViolationTemplate::new(
            $crate::violation_template!(@text $summary),
            $crate::violation_template!(@text $rationale),
            $crate::violation_template!(@text $suggestion),
        )
    };
}

pub use crate::violation_template;

/// The default virtual location context name for VCS changes.
pub const VCS_CONTEXT_NAME: &str = "VCS_Context";

/// Represents the file or environment context of a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LocationContext {
    /// A physical file on disk.
    File(PathBuf),
    /// A virtual execution context (e.g., `VCS_Context`).
    Virtual(String),
}

impl std::fmt::Display for LocationContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(path) => write!(f, "{}", path.display()),
            Self::Virtual(name) => write!(f, "{name}"),
        }
    }
}

impl Serialize for LocationContext {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::File(path) => serializer.serialize_str(&path.to_string_lossy()),
            Self::Virtual(name) => serializer.serialize_str(name),
        }
    }
}

/// A byte range span (start, end) inside a source file.
#[derive(Debug, Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceSpan {
    /// Start byte offset (inclusive).
    pub start: usize,
    /// End byte offset (exclusive).
    pub end: usize,
}

impl SourceSpan {
    /// Creates a new `SourceSpan`.
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Creates a `SourceSpan` from an AST node byte range.
    #[must_use]
    pub const fn from_range(range: std::ops::Range<usize>) -> Self {
        Self {
            start: range.start,
            end: range.end,
        }
    }
}

// TODO: Should we use line-index of the line/column?
/// Represents a location span and 1-indexed coordinate inside a source file or linter context.
#[derive(Debug, Serialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceLocation {
    /// The file path or virtual context name.
    pub context: LocationContext,
    /// The byte range span of the violation.
    pub span: SourceSpan,
    /// 1-indexed start line number.
    pub line: usize,
    /// 1-indexed start column number.
    pub column: usize,
}

impl SourceLocation {
    /// Creates a `SourceLocation` directly from a file path and an AST node.
    #[must_use]
    pub fn from_node(path: impl Into<PathBuf>, node: &AstNode<'_>) -> Self {
        Self::file_span(
            path,
            SourceSpan::from_range(node.range()),
            LineColumn::from_node(node),
        )
    }

    /// Creates a `SourceLocation` for a file path, byte span, and resolved 1-indexed coordinate.
    #[must_use]
    pub fn file_span(path: impl Into<PathBuf>, span: SourceSpan, coord: LineColumn) -> Self {
        Self {
            context: LocationContext::File(path.into()),
            span,
            line: coord.line,
            column: coord.column,
        }
    }

    /// Creates a `SourceLocation` for a virtual context, resolving `(line, column)` from `content`.
    #[must_use]
    pub fn virtual_span(name: impl Into<String>, content: &str, span: SourceSpan) -> Self {
        let coord = LineIndex::new(content).lookup(span.start);
        Self {
            context: LocationContext::Virtual(name.into()),
            span,
            line: coord.line,
            column: coord.column,
        }
    }

    /// Formats the source location into a user-friendly coordinate header (e.g. `path/to/file.rs:10:4`).
    #[must_use]
    pub fn format_header(&self) -> String {
        format!("{}:{}:{}", self.context, self.line, self.column)
    }
}

/// An alert diagnostic containing a rule violation payload and its source location.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The rule name (e.g., "no-edits-on-described-commits").
    pub rule_name: RuleName,
    /// The detailed explanation and description of the rule violation.
    pub message: ViolationMessage,
    /// The source location where the violation occurred.
    pub location: SourceLocation,
}

impl Ord for Diagnostic {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.location
            .cmp(&other.location)
            .then_with(|| self.rule_name.cmp(&other.rule_name))
            .then_with(|| self.message.cmp(&other.message))
    }
}

impl PartialOrd for Diagnostic {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Diagnostic {
    /// Creates a new diagnostic.
    #[must_use]
    pub const fn new(
        rule_name: RuleName,
        message: ViolationMessage,
        location: SourceLocation,
    ) -> Self {
        Self {
            rule_name,
            message,
            location,
        }
    }
}

/// A 1-indexed line and column coordinate inside a file.
#[derive(Debug, Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq)]
pub struct LineColumn {
    /// 1-indexed line number.
    pub line: usize,
    /// 1-indexed column number.
    pub column: usize,
}

impl LineColumn {
    /// Resolves the 1-indexed start `(line, column)` coordinate of an AST node.
    #[must_use]
    pub fn from_node(node: &AstNode<'_>) -> Self {
        let start_pos = node.start_pos();
        Self {
            line: start_pos.line() + 1,
            column: start_pos.column(node) + 1,
        }
    }
}

/// Optimized index for mapping flat byte offsets to 1-indexed (line, column) positions.
struct LineIndex {
    inner: line_index::LineIndex,
}

impl LineIndex {
    /// Creates a new `LineIndex` for the given file content.
    #[must_use]
    fn new(content: &str) -> Self {
        Self {
            inner: line_index::LineIndex::new(content),
        }
    }

    /// Resolves a byte offset to a 1-indexed (line, column) coordinate.
    #[must_use]
    fn lookup(&self, offset: usize) -> LineColumn {
        let line_col = self.inner.line_col(line_index::TextSize::from(
            u32::try_from(offset).unwrap_or(u32::MAX),
        ));
        LineColumn {
            line: line_col.line as usize + 1,
            column: line_col.col as usize + 1,
        }
    }
}

/// Formats and prints a list of diagnostics to stdout.
///
/// Supports JSON pretty-printing and plain text with pre-resolved line-column coordinates.
///
/// # Errors
///
/// Returns an error if JSON serialization fails.
pub fn print_diagnostics(diagnostics: &[Diagnostic], format: &str) -> anyhow::Result<()> {
    let mut sorted: Vec<&Diagnostic> = diagnostics.iter().collect();
    sorted.sort_unstable();

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&sorted)?);
    } else {
        for diagnostic in sorted {
            let location_header = diagnostic.location.format_header();

            println!(
                "{}: [{}] {}\n  Rationale: {}\n  Suggestion: {}\n",
                location_header,
                diagnostic.rule_name,
                diagnostic.message.summary,
                diagnostic.message.rationale,
                diagnostic.message.suggestion
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_text_resolve_and_render() {
        const ADVICE: LanguageText = LanguageText::new(
            "Parameterize variations for {func}",
            &[
                (
                    SupportLang::Python,
                    "Use @pytest.mark.parametrize for {func}",
                ),
                (SupportLang::Rust, "Use #[rstest] for {func}"),
            ],
        );

        assert_eq!(
            ADVICE.resolve_for_lang(SupportLang::Python),
            "Use @pytest.mark.parametrize for {func}"
        );
        assert_eq!(
            ADVICE.render_for_lang(SupportLang::Python, &[("func", "test_math")]),
            "Use @pytest.mark.parametrize for test_math"
        );
        assert_eq!(
            ADVICE.render_for_lang(SupportLang::Rust, &[("func", "test_math")]),
            "Use #[rstest] for test_math"
        );
        assert_eq!(
            ADVICE.render(&[("func", "test_math")]),
            "Parameterize variations for test_math"
        );
    }

    #[test]
    fn test_violation_template_render() {
        const TEMPLATE: ViolationTemplate = ViolationTemplate::new(
            LanguageText::from_static("Function `{func}` too long"),
            LanguageText::from_static("Long functions are hard to read"),
            LanguageText::new(
                "Split function `{func}` into smaller helpers",
                &[
                    (
                        SupportLang::Python,
                        "Refactor `{func}` with helper functions",
                    ),
                    (
                        SupportLang::Rust,
                        "Extract logic from `{func}` into sub-functions",
                    ),
                ],
            ),
        );

        assert_eq!(
            TEMPLATE.render_for_lang(SupportLang::Python, &[("func", "process_data")]),
            ViolationMessage::new(
                "Function `process_data` too long",
                "Long functions are hard to read",
                "Refactor `process_data` with helper functions",
            )
        );

        assert_eq!(
            TEMPLATE.render_for_lang(SupportLang::Rust, &[("func", "process_data")]),
            ViolationMessage::new(
                "Function `process_data` too long",
                "Long functions are hard to read",
                "Extract logic from `process_data` into sub-functions",
            )
        );
    }

    #[test]
    fn test_violation_template_macro() {
        const TEMPLATE: ViolationTemplate = violation_template! {
            summary: "Function `{func}` too long",
            rationale: "Long functions are hard to read",
            suggestion: {
                base: "Split function `{func}` into smaller helpers",
                Python => "Refactor `{func}` with helper functions",
                Rust => "Extract logic from `{func}` into sub-functions",
            },
        };

        assert_eq!(
            TEMPLATE.render_for_lang(SupportLang::Python, &[("func", "process_data")]),
            ViolationMessage::new(
                "Function `process_data` too long",
                "Long functions are hard to read",
                "Refactor `process_data` with helper functions",
            )
        );

        assert_eq!(
            TEMPLATE.render_for_lang(SupportLang::Rust, &[("func", "process_data")]),
            ViolationMessage::new(
                "Function `process_data` too long",
                "Long functions are hard to read",
                "Extract logic from `process_data` into sub-functions",
            )
        );
    }

    #[test]
    fn test_source_location_from_node() {
        let source = "fn main() {\n    let value = 42;\n}\n";
        let grep = ast_grep_core::AstGrep::new(source, SupportLang::Rust);
        let let_node = grep.root().find("let $VAR = $VAL");
        assert!(let_node.is_some());
        if let Some(matched) = let_node {
            let loc = SourceLocation::from_node("src/main.rs", &matched);
            assert_eq!(
                loc,
                SourceLocation {
                    context: LocationContext::File(PathBuf::from("src/main.rs")),
                    span: SourceSpan::new(16, 31),
                    line: 2,
                    column: 5,
                }
            );
            assert_eq!(loc.format_header(), "src/main.rs:2:5");
            assert_eq!(
                serde_json::to_value(&loc).ok(),
                Some(serde_json::json!({
                    "context": "src/main.rs",
                    "span": { "start": 16, "end": 31 },
                    "line": 2,
                    "column": 5,
                }))
            );
        }
    }

    #[test]
    fn test_source_location_virtual_span() {
        let virtual_loc = SourceLocation::virtual_span(
            VCS_CONTEXT_NAME,
            "echo ok\njj edit main",
            SourceSpan::new(8, 20),
        );
        assert_eq!(
            virtual_loc,
            SourceLocation {
                context: LocationContext::Virtual(VCS_CONTEXT_NAME.to_string()),
                span: SourceSpan::new(8, 20),
                line: 2,
                column: 1,
            }
        );
        assert_eq!(virtual_loc.format_header(), "VCS_Context:2:1");
    }

    #[test]
    fn test_diagnostic_canonical_ordering() {
        let message_a = ViolationMessage::new("Summary A", "Rationale A", "Suggestion A");
        let message_b = ViolationMessage::new("Summary B", "Rationale B", "Suggestion B");

        let diag1 = Diagnostic::new(
            RuleName("rule-b"),
            message_a.clone(),
            SourceLocation::file_span(
                "src/b.rs",
                SourceSpan::new(10, 20),
                LineColumn { line: 2, column: 1 },
            ),
        );
        let diag2 = Diagnostic::new(
            RuleName("rule-a"),
            message_a,
            SourceLocation::file_span(
                "src/a.rs",
                SourceSpan::new(5, 15),
                LineColumn { line: 1, column: 1 },
            ),
        );
        let diag3 = Diagnostic::new(
            RuleName("rule-a"),
            message_b,
            SourceLocation::file_span(
                "src/a.rs",
                SourceSpan::new(5, 15),
                LineColumn { line: 1, column: 1 },
            ),
        );

        let mut diagnostics = vec![diag1.clone(), diag3.clone(), diag2.clone()];
        diagnostics.sort_unstable();

        assert_eq!(diagnostics, vec![diag2, diag3, diag1]);
    }
}
