//! Diagnostic representation, serialization, and reporting.

use ast_grep_language::SupportLang;
use serde::Serialize;
use std::path::PathBuf;

/// Detailed explanation and description of a rule violation.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
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
        Self { summary: summary.into(), rationale: rationale.into(), suggestion: suggestion.into() }
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
    /// Creates a new `LanguageText` with the given base and overrides.
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
        Self { base, overrides: &[] }
    }

    /// Resolves the raw static text for the given language.
    #[must_use]
    pub fn resolve(&self, lang: SupportLang) -> &'static str {
        for &(override_lang, text) in self.overrides {
            if override_lang == lang {
                return text;
            }
        }
        self.base
    }

    /// Resolves and interpolates named `{key}` placeholders for the given language.
    #[must_use]
    pub fn render(&self, lang: SupportLang, params: &[(&str, &str)]) -> String {
        let mut result = self.resolve(lang).to_string();
        for &(placeholder, replacement) in params {
            let pattern = format!("{{{placeholder}}}");
            result = result.replace(&pattern, replacement);
        }
        result
    }
}

/// A declarative template for constructing multi-language `ViolationMessage`s.
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
        Self { summary, rationale, suggestion }
    }

    /// Creates a template with uniform static text for all languages.
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

    /// Renders the template into a concrete `ViolationMessage` for the given language and parameters.
    #[must_use]
    pub fn render(&self, lang: SupportLang, params: &[(&str, &str)]) -> ViolationMessage {
        ViolationMessage {
            summary: self.summary.render(lang, params),
            rationale: self.rationale.render(lang, params),
            suggestion: self.suggestion.render(lang, params),
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
    Virtual {
        /// The name of the virtual context.
        name: String,
        /// The raw string content of the virtual context.
        content: String,
    },
}

impl Serialize for LocationContext {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::File(path) => serializer.serialize_str(&path.to_string_lossy()),
            Self::Virtual { name, .. } => serializer.serialize_str(name),
        }
    }
}

/// A byte range span (start, end) inside a source file.
#[derive(Debug, Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq)]
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
        Self { start: range.start, end: range.end }
    }
}

/// Represents a location span inside a source file or linter context.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    /// The file path or virtual context name.
    pub context: LocationContext,
    /// The byte range span of the violation.
    pub span: SourceSpan,
}

impl SourceLocation {
    /// Creates a `SourceLocation` for a file path and span.
    #[must_use]
    pub fn file(path: impl Into<PathBuf>, span: SourceSpan) -> Self {
        Self { context: LocationContext::File(path.into()), span }
    }

    /// Creates a `SourceLocation` for a file path and an AST byte range.
    #[must_use]
    pub fn file_range(path: impl Into<PathBuf>, range: std::ops::Range<usize>) -> Self {
        Self::file(path, SourceSpan::from_range(range))
    }

    /// Formats the source location into a user-friendly coordinate header (e.g. `path/to/file.rs:10:4`).
    #[must_use]
    pub fn format_header(&self, file_index: Option<&LineIndex>) -> String {
        match &self.context {
            LocationContext::File(path) => file_index.map_or_else(
                || path.to_string_lossy().into_owned(),
                |idx| {
                    let coords = idx.lookup(self.span.start);
                    format!("{}:{}:{}", path.to_string_lossy(), coords.line, coords.column)
                },
            ),
            LocationContext::Virtual { name, .. } => file_index.map_or_else(
                || name.clone(),
                |idx| {
                    let coords = idx.lookup(self.span.start);
                    format!("{}:{}:{}", name, coords.line, coords.column)
                },
            ),
        }
    }
}

/// The verbose name of a rule (e.g., "no-edits-on-described-commits").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct RuleName(pub &'static str);

impl std::fmt::Display for RuleName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An alert diagnostic containing a rule violation payload and its source location.
#[derive(Debug, Serialize, Clone)]
pub struct Diagnostic {
    /// The rule name (e.g., "no-edits-on-described-commits").
    pub rule_name: RuleName,
    /// The detailed explanation and description of the rule violation.
    pub message: ViolationMessage,
    /// The source location where the violation occurred.
    pub location: SourceLocation,
}

impl Diagnostic {
    /// Creates a new diagnostic.
    #[must_use]
    pub const fn new(
        rule_name: RuleName,
        message: ViolationMessage,
        location: SourceLocation,
    ) -> Self {
        Self { rule_name, message, location }
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

/// Optimized index for mapping flat byte offsets to 1-indexed (line, column) positions.
pub struct LineIndex {
    inner: line_index::LineIndex,
}

impl LineIndex {
    /// Creates a new `LineIndex` for the given file content.
    #[must_use]
    pub fn new(content: &str) -> Self {
        Self { inner: line_index::LineIndex::new(content) }
    }

    /// Resolves a byte offset to a 1-indexed (line, column) coordinate.
    #[must_use]
    pub fn lookup(&self, offset: usize) -> LineColumn {
        let line_col = self
            .inner
            .line_col(line_index::TextSize::from(u32::try_from(offset).unwrap_or(u32::MAX)));
        LineColumn { line: line_col.line as usize + 1, column: line_col.col as usize + 1 }
    }
}

/// Formats and prints a list of diagnostics to stdout.
///
/// Supports JSON pretty-printing and plain text with automatic line-column offset resolution
/// for source file diagnostics.
///
/// # Errors
///
/// Returns an error if JSON serialization fails.
pub fn print_diagnostics(diagnostics: &[Diagnostic], format: &str) -> anyhow::Result<()> {
    if format == "json" {
        println!("{}", serde_json::to_string_pretty(diagnostics)?);
    } else {
        use std::collections::BTreeMap;
        // TODO(performance): Avoid redundant file disk reads here when file content has already been loaded for linting.
        // Group diagnostics by location context directly and deterministically
        let mut grouped: BTreeMap<LocationContext, Vec<&Diagnostic>> = BTreeMap::new();
        for diagnostic in diagnostics {
            grouped.entry(diagnostic.location.context.clone()).or_default().push(diagnostic);
        }

        for (context, mut diags) in grouped {
            // Sort diagnostics by their span start position to ensure stable/orderly reporting within each context
            diags.sort_by_key(|diagnostic| diagnostic.location.span.start);

            let file_index = match &context {
                LocationContext::File(path) => {
                    std::fs::read_to_string(path).ok().map(|content| LineIndex::new(&content))
                }
                LocationContext::Virtual { content, .. } => Some(LineIndex::new(content)),
            };

            for diagnostic in diags {
                let location_header = diagnostic.location.format_header(file_index.as_ref());

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
                (SupportLang::Python, "Use @pytest.mark.parametrize for {func}"),
                (SupportLang::Rust, "Use #[rstest] for {func}"),
            ],
        );

        assert_eq!(ADVICE.resolve(SupportLang::Python), "Use @pytest.mark.parametrize for {func}");
        assert_eq!(ADVICE.resolve(SupportLang::Rust), "Use #[rstest] for {func}");

        assert_eq!(
            ADVICE.render(SupportLang::Python, &[("func", "test_math")]),
            "Use @pytest.mark.parametrize for test_math"
        );
        assert_eq!(
            ADVICE.render(SupportLang::Rust, &[("func", "test_math")]),
            "Use #[rstest] for test_math"
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
                    (SupportLang::Python, "Refactor `{func}` with helper functions"),
                    (SupportLang::Rust, "Extract logic from `{func}` into sub-functions"),
                ],
            ),
        );

        assert_eq!(
            TEMPLATE.render(SupportLang::Python, &[("func", "process_data")]),
            ViolationMessage::new(
                "Function `process_data` too long",
                "Long functions are hard to read",
                "Refactor `process_data` with helper functions",
            )
        );

        assert_eq!(
            TEMPLATE.render(SupportLang::Rust, &[("func", "process_data")]),
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
            TEMPLATE.render(SupportLang::Python, &[("func", "process_data")]),
            ViolationMessage::new(
                "Function `process_data` too long",
                "Long functions are hard to read",
                "Refactor `process_data` with helper functions",
            )
        );

        assert_eq!(
            TEMPLATE.render(SupportLang::Rust, &[("func", "process_data")]),
            ViolationMessage::new(
                "Function `process_data` too long",
                "Long functions are hard to read",
                "Extract logic from `process_data` into sub-functions",
            )
        );
    }
}
