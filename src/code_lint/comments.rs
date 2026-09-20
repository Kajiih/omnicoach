//! Shared comment extraction and documentation explanation engine.
//!
//! Provides an allocation-free `CommentIndex` for indexing comments by line number,
//! stripping standard linter/tooling directive prefixes, and verifying that sensitive
//! operations (such as exception suppression) are accompanied by substantive explanation comments.

use crate::code_lint::{AstNode, SourceDoc};
use ast_grep_core::AstGrep;
use std::collections::HashMap;

/// Collects all Tree-sitter comment nodes in source order.
pub fn collect_comment_nodes<'a>(node: &AstNode<'a>) -> impl Iterator<Item = AstNode<'a>> {
    node.dfs().filter(|curr| {
        matches!(
            curr.kind().as_ref(),
            "comment" | "line_comment" | "block_comment"
        )
    })
}

/// Strips leading and trailing comment delimiters (`//`, `#`, `/* ... */`).
///
/// Returns `Some(stripped)` if delimiters were present, or `None` if the text
/// does not start with standard comment delimiters.
#[must_use]
pub fn strip_comment_delimiters(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    trimmed
        .strip_prefix("//")
        .or_else(|| trimmed.strip_prefix('#'))
        .map(str::trim_start)
        .or_else(|| {
            trimmed
                .strip_prefix("/*")
                .map(|body| body.trim_start().trim_end_matches("*/").trim_end())
        })
}

const DIRECTIVE_PREFIXES: &[&str] = &[
    "omni:ignore",
    "omni:disable-file",
    "ruff: noqa",
    "ruff:noqa",
    "type: ignore",
    "type:ignore",
    "pyright: ignore",
    "pyright:ignore",
    "pylint: disable",
    "pylint:disable",
    "noqa",
];

fn find_directive_prefix(text: &str) -> Option<usize> {
    for &prefix in DIRECTIVE_PREFIXES {
        if text
            .get(..prefix.len())
            .is_some_and(|sub| sub.eq_ignore_ascii_case(prefix))
        {
            let rest = &text[prefix.len()..];
            if rest.is_empty()
                || rest.starts_with(|c: char| {
                    c.is_whitespace() || c == ':' || c == '=' || c == '[' || c == '-'
                })
            {
                return Some(prefix.len());
            }
        }
    }
    None
}

/// Checks if a whitespace-delimited word consists of rule codes (e.g. `SIM105`, `F401,`, `SIM105,F401`).
fn is_rule_code_token(word: &str) -> bool {
    let trimmed = word.trim_matches(|c: char| c == ',' || c == ';');
    !trimmed.is_empty()
        && trimmed.split(',').all(|part| {
            let segment = part.trim();
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        })
}

fn strip_rule_codes(mut text: &str) -> &str {
    text = text.trim_start();
    while !text.is_empty() {
        let first_word = text.split_whitespace().next().unwrap_or("");
        if is_rule_code_token(first_word) {
            let had_comma = first_word.ends_with(',');
            text = text[first_word.len()..].trim_start();
            if !had_comma {
                break;
            }
        } else {
            break;
        }
    }
    text
}

/// Strips standard linter/tooling directives (`noqa`, `type: ignore`, `pyright`, `pylint`, `omni:ignore`).
///
/// If the comment contains an explanatory reason after a separator (e.g. `-- reason`),
/// the directive prefix is stripped and the remaining reason text is returned.
/// If the comment consists solely of directives and rule codes, `""` is returned.
#[must_use]
pub fn clean_explanation(text: &str) -> &str {
    let stripped = strip_comment_delimiters(text).unwrap_or_else(|| text.trim());
    if stripped.is_empty() {
        return "";
    }

    let Some(prefix_len) = find_directive_prefix(stripped) else {
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
        remainder = strip_rule_codes(&remainder[1..]);
    }

    remainder
        .trim_start_matches(|c: char| c == '-' || c == ':' || c == ';' || c.is_whitespace())
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
    let starts_with_todo = text
        .get(..4)
        .is_some_and(|sub| sub.eq_ignore_ascii_case("todo"));
    let starts_with_fixme = text
        .get(..5)
        .is_some_and(|sub| sub.eq_ignore_ascii_case("fixme"));
    if (starts_with_todo || starts_with_fixme) && words <= 3 {
        return false;
    }

    true
}

#[derive(Clone)]
struct IndexedComment<'a> {
    node: AstNode<'a>,
    is_standalone: bool,
}

/// Index of source comments, mapping 1-indexed lines to comment AST nodes.
#[derive(Default)]
pub struct CommentIndex<'a> {
    comments_by_line: HashMap<usize, IndexedComment<'a>>,
}

impl<'a> CommentIndex<'a> {
    /// Builds a `CommentIndex` from `AstGrep`.
    #[must_use]
    pub fn from_ast(grep: &'a AstGrep<SourceDoc>) -> Self {
        let root = grep.root();
        let root_text = root.text();
        let source = root_text.as_ref();
        let mut comments_by_line = HashMap::new();
        for node in collect_comment_nodes(&root) {
            let start_line = node.start_pos().line() + 1;
            let end_line = node.end_pos().line() + 1;
            let node_offset = node.range().start;
            let line_start_offset = source[..node_offset].rfind('\n').map_or(0, |idx| idx + 1);
            let is_standalone = source[line_start_offset..node_offset].trim().is_empty();
            for line in start_line..=end_line {
                comments_by_line.insert(
                    line,
                    IndexedComment {
                        node: node.clone(),
                        is_standalone,
                    },
                );
            }
        }

        Self { comments_by_line }
    }

    /// Returns the raw comment text on a specific 1-indexed line, if any.
    #[must_use]
    pub fn comment_on_line(&self, line: usize) -> Option<std::borrow::Cow<'_, str>> {
        self.comments_by_line
            .get(&line)
            .map(|entry| entry.node.text())
    }

    /// Checks whether a given 1-indexed line has an inline, substantive explanation.
    #[must_use]
    pub fn has_inline_explanation(&self, line: usize) -> bool {
        self.comment_on_line(line).is_some_and(|text| {
            let cleaned = clean_explanation(text.as_ref());
            is_substantive_explanation(cleaned)
        })
    }

    /// Verifies if a given line has an adjacent, substantive explanation comment.
    ///
    /// Checks:
    /// 1. An inline trailing comment on `line`.
    /// 2. Contiguous preceding comment lines walking upward from `line - 1`.
    #[must_use]
    pub fn has_adjacent_explanation(&self, line: usize) -> bool {
        // 1. Inline comment on the same line
        if self.has_inline_explanation(line) {
            return true;
        }

        // 2. Contiguous standalone comment block directly above `line`
        let mut curr_line = line.saturating_sub(1);
        let mut block = String::new();
        let mut prev_range: Option<(usize, usize)> = None;

        while curr_line > 0 {
            if let Some(entry) = self.comments_by_line.get(&curr_line) {
                // Preceding comment blocks must consist exclusively of standalone comment lines.
                // An inline comment on a preceding code line belongs to that line, not this block.
                if !entry.is_standalone {
                    break;
                }

                let range = (entry.node.range().start, entry.node.range().end);
                if prev_range != Some(range) {
                    prev_range = Some(range);
                    let comment_text = entry.node.text();
                    let cleaned = clean_explanation(comment_text.as_ref());
                    if !cleaned.is_empty() {
                        if !block.is_empty() {
                            block.insert(0, ' ');
                        }
                        block.insert_str(0, cleaned);
                    }
                }
                curr_line = curr_line.saturating_sub(1);
            } else {
                break;
            }
        }

        is_substantive_explanation(&block)
    }

    /// Checks whether an AST node is accompanied by an explanatory comment:
    /// 1. A contiguous standalone comment block directly above its start line.
    /// 2. An inline comment on any line spanning the node.
    #[must_use]
    pub fn has_explanation_for_node(&self, node: &AstNode<'_>) -> bool {
        let start_line = node.start_pos().line() + 1;
        let end_line = node.end_pos().line() + 1;
        self.has_adjacent_explanation(start_line)
            || (start_line..=end_line).any(|target_line| self.has_inline_explanation(target_line))
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
    #[case::noqa_with_plain_explanation(
        "# noqa: SIM105 safe because transient",
        "safe because transient"
    )]
    #[case::noqa_multiple_codes_with_plain_explanation(
        "# noqa: SIM105, F401 safe because transient",
        "safe because transient"
    )]
    #[case::type_ignore("# type: ignore[import]", "")]
    #[case::type_ignore_with_explanation(
        "# type: ignore[import] third party library untyped",
        "third party library untyped"
    )]
    #[case::omni_ignore("# omni:ignore[rule-name] -- explanation", "explanation")]
    #[case::pylint_disable("# pylint: disable=broad-except", "")]
    #[case::pylint_disable_with_explanation(
        "# pylint: disable=broad-except file may be missing",
        "file may be missing"
    )]
    #[case::rust_safety_comment(
        "// SAFETY: pointer is non-null and aligned",
        "SAFETY: pointer is non-null and aligned"
    )]
    #[case::path_comment("# /usr/bin/env python", "/usr/bin/env python")]
    #[case::deref_comment("// *ptr = 10", "*ptr = 10")]
    #[case::c_block_comment("/* cleanup cache */", "cleanup cache")]
    #[case::noqa_no_space_comma("# noqa: SIM105,F401,E501", "")]
    #[case::noqa_no_space_comma_with_explanation(
        "# noqa: SIM105,F401 explanation text here",
        "explanation text here"
    )]
    #[case::utf8_accented_comment(
        "# café au lait délicieux et nécessaire",
        "café au lait délicieux et nécessaire"
    )]
    fn test_clean_explanation_directive_stripping(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(clean_explanation(input), expected);
    }

    #[rstest]
    #[case::valid_long("Cache file may be removed concurrently", true)]
    #[case::valid_safety("Pointer is guaranteed non-null", true)]
    #[case::utf8_valid("café au lait délicieux", true)]
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

    #[test]
    fn test_preceding_inline_comment_on_code_not_counted() {
        let source = indoc! {r"
            x = calculate()  # unrelated inline comment here
            with suppress(FileNotFoundError):
                pass
        "};
        let grep = AstGrep::new(source, SupportLang::Python);
        let index = CommentIndex::from_ast(&grep);

        // Line 2 has no standalone comment preceding it; line 1 is code with inline comment
        assert!(!index.has_adjacent_explanation(2));
    }
}
