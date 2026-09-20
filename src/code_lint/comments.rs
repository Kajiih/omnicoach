//! Shared comment extraction and documentation explanation engine.
//!
//! Provides a zero-allocation `CommentIndex` for indexing comments by line number,
//! stripping standard linter/tooling directive prefixes, and verifying that sensitive
//! operations (such as exception suppression) are accompanied by substantive explanation comments.

use crate::code_lint::{AstNode, SourceDoc};
use ast_grep_core::AstGrep;
use std::collections::HashMap;

/// Recursively collects all Tree-sitter comment nodes in source order.
pub fn collect_comment_nodes<'a>(node: &AstNode<'a>, comments: &mut Vec<AstNode<'a>>) {
    let kind = node.kind();
    if kind == "comment" || kind == "line_comment" || kind == "block_comment" {
        comments.push(node.clone());
        return;
    }
    for child in node.children() {
        collect_comment_nodes(&child, comments);
    }
}

/// Strips leading comment delimiters (`#`, `//`, `/*`, `*`, `*/`) and surrounding whitespace.
#[must_use]
pub fn strip_comment_delimiters(text: &str) -> &str {
    text.trim_start_matches(|c: char| c == '#' || c == '/' || c == '*' || c.is_whitespace())
        .trim_end_matches(|c: char| c == '*' || c == '/' || c.is_whitespace())
}

/// Strips standard linter/tooling directives (`noqa`, `type: ignore`, `pyright`, `pylint`, `omni:ignore`).
///
/// If the comment contains an explanatory reason after a separator (e.g. `-- reason`),
/// the directive prefix is stripped and the remaining reason text is returned.
/// If the comment consists solely of directives and rule codes, `""` is returned.
#[must_use]
pub fn clean_explanation(text: &str) -> &str {
    let stripped = strip_comment_delimiters(text);
    if stripped.is_empty() {
        return "";
    }

    let lower = stripped.to_ascii_lowercase();

    // Check for directive prefixes
    let directive_prefix_len = if lower.starts_with("omni:ignore") {
        Some("omni:ignore".len())
    } else if lower.starts_with("omni:disable-file") {
        Some("omni:disable-file".len())
    } else if lower.starts_with("noqa") {
        Some("noqa".len())
    } else if lower.starts_with("ruff: noqa") {
        Some("ruff: noqa".len())
    } else if lower.starts_with("ruff:noqa") {
        Some("ruff:noqa".len())
    } else if lower.starts_with("type: ignore") {
        Some("type: ignore".len())
    } else if lower.starts_with("type:ignore") {
        Some("type:ignore".len())
    } else if lower.starts_with("pyright: ignore") {
        Some("pyright: ignore".len())
    } else if lower.starts_with("pyright:ignore") {
        Some("pyright:ignore".len())
    } else if lower.starts_with("pylint: disable") {
        Some("pylint: disable".len())
    } else if lower.starts_with("pylint:disable") {
        Some("pylint:disable".len())
    } else {
        None
    };

    let Some(prefix_len) = directive_prefix_len else {
        return stripped.trim();
    };

    let after_prefix = stripped[prefix_len..].trim_start();

    // If there is an explicit `--` separator, the explanation is everything after it.
    if let Some(idx) = after_prefix.find("--") {
        return after_prefix[idx + 2..].trim();
    }

    let mut remainder = after_prefix;

    // Strip bracketed code list: `[rule-name, other-rule]`
    if remainder.starts_with('[') {
        if let Some(close_idx) = remainder.find(']') {
            remainder = remainder[close_idx + 1..].trim_start();
        } else {
            return "";
        }
    } else if remainder.starts_with(':') || remainder.starts_with('=') {
        remainder = remainder[1..].trim_start();
        // Skip rule code tokens (e.g. `SIM105`, `broad-except`, `W0718,unused-import`)
        while !remainder.is_empty() {
            let first_word = remainder.split_whitespace().next().unwrap_or("");
            let is_rule_token = first_word.split(',').all(|part| {
                let token_segment = part.trim();
                !token_segment.is_empty()
                    && token_segment
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
            });
            if is_rule_token {
                remainder = remainder[first_word.len()..].trim_start();
            } else {
                break;
            }
        }
    }

    remainder
        .trim_start_matches(|c: char| c == '-' || c == ':' || c.is_whitespace())
        .trim()
}

/// Checks if an explanation text is substantive (at least 3 words and 10 non-whitespace chars).
#[must_use]
pub fn is_substantive_explanation(text: &str) -> bool {
    let words = text.split_whitespace().count();
    let chars = text.chars().filter(|c| !c.is_whitespace()).count();
    if words < 3 || chars < 10 {
        return false;
    }

    // Disallow bare TODO/FIXME notes without substantive explanation
    let lower = text.to_ascii_lowercase();
    if (lower.starts_with("todo") || lower.starts_with("fixme")) && words <= 3 {
        return false;
    }

    true
}

/// Index of source comments, mapping 1-indexed lines to comment text.
#[derive(Debug, Default)]
pub struct CommentIndex {
    comments_by_line: HashMap<usize, String>,
}

impl CommentIndex {
    /// Builds a `CommentIndex` from `AstGrep`.
    #[must_use]
    pub fn from_ast(grep: &AstGrep<SourceDoc>) -> Self {
        let mut comment_nodes = Vec::new();
        collect_comment_nodes(&grep.root(), &mut comment_nodes);

        let mut comments_by_line = HashMap::with_capacity(comment_nodes.len());
        for node in comment_nodes {
            let line = node.start_pos().line() + 1;
            comments_by_line.insert(line, node.text().into_owned());
        }

        Self { comments_by_line }
    }

    /// Returns the raw comment text on a specific 1-indexed line, if any.
    #[must_use]
    pub fn comment_on_line(&self, line: usize) -> Option<&str> {
        self.comments_by_line.get(&line).map(String::as_str)
    }

    /// Verifies if a given line has an adjacent, substantive explanation comment.
    ///
    /// Checks:
    /// 1. An inline trailing comment on `line`.
    /// 2. Contiguous preceding comment lines walking upward from `line - 1`.
    #[must_use]
    pub fn has_adjacent_explanation(&self, line: usize) -> bool {
        // 1. Inline comment on the same line
        if let Some(inline_text) = self.comment_on_line(line) {
            let cleaned = clean_explanation(inline_text);
            if is_substantive_explanation(cleaned) {
                return true;
            }
        }

        // 2. Contiguous comment block directly above `line`
        let mut curr_line = line.saturating_sub(1);
        let mut block_parts = Vec::new();

        while curr_line > 0 {
            if let Some(comment_text) = self.comment_on_line(curr_line) {
                let cleaned = clean_explanation(comment_text);
                if !cleaned.is_empty() {
                    block_parts.push(cleaned);
                }
                curr_line = curr_line.saturating_sub(1);
            } else {
                break;
            }
        }

        if block_parts.is_empty() {
            return false;
        }

        block_parts.reverse();
        let combined = block_parts.join(" ");
        is_substantive_explanation(&combined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast_grep_language::SupportLang;
    use indoc::indoc;
    use rstest::rstest;

    #[rstest]
    #[case::plain_comment("# Safe because cache is transient", "Safe because cache is transient")]
    #[case::noqa_bare("# noqa: SIM105", "")]
    #[case::noqa_with_comment("# noqa: SIM105 -- file may be removed", "file may be removed")]
    #[case::type_ignore("# type: ignore[import]", "")]
    #[case::omni_ignore("# omni:ignore[rule-name] -- explanation", "explanation")]
    #[case::pylint_disable("# pylint: disable=broad-except", "")]
    #[case::rust_safety_comment(
        "// SAFETY: pointer is non-null and aligned",
        "SAFETY: pointer is non-null and aligned"
    )]
    fn test_clean_explanation_directive_stripping(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(clean_explanation(input), expected);
    }

    #[rstest]
    #[case::valid_long("Cache file may be removed concurrently", true)]
    #[case::valid_safety("Pointer is guaranteed non-null", true)]
    #[case::too_short_words("ignore", false)]
    #[case::todo_too_short("todo fix", false)]
    #[case::too_short_chars("ok", false)]
    #[case::empty("", false)]
    fn test_is_substantive_explanation(#[case] input: &str, #[case] expected: bool) {
        assert_eq!(is_substantive_explanation(input), expected);
    }

    #[test]
    fn test_comment_index_inline_and_preceding() {
        let source = indoc! {r"
            # Lock file is cleaned up by background daemon
            with suppress(FileNotFoundError):
                pass

            with suppress(KeyError):  # Optional configuration entry
                pass

            # noqa: SIM105
            with suppress(ValueError):
                pass

            with suppress(OSError):
                pass
        "};

        let grep = AstGrep::new(source, SupportLang::Python);
        let index = CommentIndex::from_ast(&grep);

        // Line 2: `with suppress(FileNotFoundError):` has preceding comment on Line 1
        assert!(index.has_adjacent_explanation(2));

        // Line 5: `with suppress(KeyError):` has inline comment on Line 5
        assert!(index.has_adjacent_explanation(5));

        // Line 9: `with suppress(ValueError):` has directive only on Line 8
        assert!(!index.has_adjacent_explanation(9));

        // Line 12: `with suppress(OSError):` has no comment
        assert!(!index.has_adjacent_explanation(12));
    }

    #[test]
    fn test_multiline_preceding_comment_block() {
        let source = indoc! {r"
            # The cache file might already be deleted
            # by another concurrent worker thread.
            with suppress(FileNotFoundError):
                pass
        "};

        let grep = AstGrep::new(source, SupportLang::Python);
        let index = CommentIndex::from_ast(&grep);

        assert!(index.has_adjacent_explanation(3));
    }
}
