//! Diagnostic representation, serialization, and reporting.

use serde::Serialize;
use std::path::PathBuf;

/// Detailed explanation and description of a rule violation.
#[derive(Debug, Serialize, Clone)]
pub struct ViolationMessage {
    /// A short summary of the violation.
    pub summary: String,
    /// The rationale or explanation of why the rule was triggered.
    pub rationale: String,
    /// A suggestion or workaround to resolve the violation.
    pub suggestion: String,
}

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

/// Represents a location span inside a source file or linter context.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    /// The file path or virtual context name.
    pub context: LocationContext,
    /// The byte range span of the violation.
    pub span: SourceSpan,
}

impl SourceLocation {
    /// Formats the source location into a user-friendly coordinate header (e.g. `path/to/file.rs:10:4`).
    #[must_use]
    pub fn format_header(&self, file_index: Option<&LineIndex>) -> String {
        match &self.context {
            LocationContext::File(path) => file_index.map_or_else(
                || path.to_string_lossy().into_owned(),
                |idx| {
                    let coords = idx.lookup(self.span.start);
                    format!(
                        "{}:{}:{}",
                        path.to_string_lossy(),
                        coords.line,
                        coords.column
                    )
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

/// The unique, short identifier code of a rule (e.g., "VCS001").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct RuleCode(pub &'static str);

impl std::fmt::Display for RuleCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
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
    /// The unique rule code (e.g., "PY001").
    pub rule_code: RuleCode,
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
        rule_code: RuleCode,
        rule_name: RuleName,
        message: ViolationMessage,
        location: SourceLocation,
    ) -> Self {
        Self {
            rule_code,
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

/// Optimized index for mapping flat byte offsets to 1-indexed (line, column) positions.
pub struct LineIndex {
    inner: line_index::LineIndex,
}

impl LineIndex {
    /// Creates a new `LineIndex` for the given file content.
    #[must_use]
    pub fn new(content: &str) -> Self {
        Self {
            inner: line_index::LineIndex::new(content),
        }
    }

    /// Resolves a byte offset to a 1-indexed (line, column) coordinate.
    #[must_use]
    pub fn lookup(&self, offset: usize) -> LineColumn {
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
            grouped
                .entry(diagnostic.location.context.clone())
                .or_default()
                .push(diagnostic);
        }

        for (context, mut diags) in grouped {
            // Sort diagnostics by their span start position to ensure stable/orderly reporting within each context
            diags.sort_by_key(|diagnostic| diagnostic.location.span.start);

            let file_index = match &context {
                LocationContext::File(path) => std::fs::read_to_string(path)
                    .ok()
                    .map(|content| LineIndex::new(&content)),
                LocationContext::Virtual { content, .. } => Some(LineIndex::new(content)),
            };

            for diagnostic in diags {
                let location_header = diagnostic.location.format_header(file_index.as_ref());

                println!(
                    "{}: [{}] {}\n  Rationale: {}\n  Suggestion: {}\n",
                    location_header,
                    diagnostic.rule_code,
                    diagnostic.message.summary,
                    diagnostic.message.rationale,
                    diagnostic.message.suggestion
                );
            }
        }
    }
    Ok(())
}
