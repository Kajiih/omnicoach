//! Shared comment extraction and documentation explanation engine.
//!
//! Provides a fast `CommentIndex` for indexing comments by line number,
//! stripping standard linter/tooling directive prefixes, and verifying that sensitive
//! operations (such as exception suppression) are accompanied by substantive explanation comments.

use crate::code_lint::ast::{self, AstNode, ParsedFile};
use crate::diagnostic::{Language, SourceSpan};
use std::collections::HashMap;

/// Index of non-doc source comments, mapping 1-indexed lines to comment AST nodes.
pub struct CommentIndex<'a> {
    lang: Language,
    comments_by_line: HashMap<usize, IndexedComment<'a>>,
}

impl<'a> CommentIndex<'a> {
    /// Builds a `CommentIndex` from a parsed file, excluding Rust doc comments (`///`, `//!`,
    /// `/** */`, `/*! */`).
    #[must_use]
    pub fn from_file(file: &'a ParsedFile) -> Self {
        let source_text = file.source_text();
        let source = source_text.as_ref();
        let mut comments_by_line = HashMap::new();
        for node in ast::collect_comment_nodes(file) {
            if file.lang() == Language::Rust && ast::rust::is_rust_doc_comment(&node.text()) {
                continue;
            }
            let start_line = node.start_line();
            let end_line = node.end_line();
            let is_standalone = is_standalone_comment_span(source, node.span());
            for line in start_line..=end_line {
                comments_by_line.insert(
                    line,
                    IndexedComment {
                        node,
                        is_standalone,
                    },
                );
            }
        }

        Self {
            lang: file.lang(),
            comments_by_line,
        }
    }

    /// Verifies if a diagnostic at `span` (starting at 1-indexed `line`) has a substantive
    /// explanation comment either directly adjacent to `line` or attached to the header of
    /// its enclosing statement (such as a multiline `with (...)` header or multiline assignment).
    ///
    /// Comments inside the statement's body are deliberately excluded: they document the body,
    /// not the statement that introduces it.
    #[must_use]
    pub fn has_explanation_for_span(
        &self,
        file: &ParsedFile,
        span: SourceSpan,
        line: usize,
    ) -> bool {
        if self.has_adjacent_explanation(line) {
            return true;
        }

        let Some(header_lines) = ast::enclosing_statement_header_range(file, span) else {
            return false;
        };

        self.has_adjacent_explanation(*header_lines.start())
            || header_lines.into_iter().any(|header_line| {
                self.has_inline_explanation(header_line) || self.has_explanation_above(header_line)
            })
    }

    /// Verifies if a given line has an adjacent, substantive explanation comment.
    ///
    /// Checks:
    /// 1. An inline trailing comment on `line`.
    /// 2. Contiguous preceding comment lines walking upward from `line - 1`.
    #[must_use]
    fn has_adjacent_explanation(&self, line: usize) -> bool {
        self.has_inline_explanation(line) || self.has_explanation_above(line)
    }

    /// Checks whether a given 1-indexed line has an inline, substantive explanation.
    #[must_use]
    fn has_inline_explanation(&self, line: usize) -> bool {
        self.comment_on_line(line).is_some_and(|text| {
            let cleaned = clean_explanation_for_lang(text.as_ref(), self.lang);
            is_substantive_explanation(cleaned)
        })
    }

    /// Returns the raw comment text on a specific 1-indexed line, if any.
    #[must_use]
    fn comment_on_line(&self, line: usize) -> Option<std::borrow::Cow<'_, str>> {
        self.comments_by_line
            .get(&line)
            .map(|entry| entry.node.text())
    }

    /// Verifies if the contiguous standalone comment block directly above 1-indexed `line`
    /// holds a substantive explanation (including reasons attached to directive comments).
    #[must_use]
    pub fn has_explanation_above(&self, line: usize) -> bool {
        self.collect_explanation_above(line, true)
    }

    /// Verifies if the contiguous standalone comment block directly above 1-indexed `line`
    /// holds a substantive prose explanation, ignoring standalone suppression directives so a
    /// directive's own `-- <reason>` never excuses a stacked reasonless directive below it.
    #[must_use]
    pub(in crate::code_lint) fn has_prose_explanation_above(&self, line: usize) -> bool {
        self.collect_explanation_above(line, false)
    }

    fn collect_explanation_above(&self, line: usize, allow_directive_reasons: bool) -> bool {
        let mut curr_line = line.saturating_sub(1);
        let mut prev_range: Option<SourceSpan> = None;
        let mut parts = Vec::new();

        while curr_line > 0 {
            let Some(entry) = self.comments_by_line.get(&curr_line) else {
                break;
            };
            // Preceding comment blocks must consist exclusively of standalone comment lines.
            // An inline comment on a preceding code line belongs to that line, not this block.
            if !entry.is_standalone {
                break;
            }

            let range = entry.node.span();
            if prev_range != Some(range) {
                prev_range = Some(range);
                let comment_text = entry.node.text();
                if allow_directive_reasons || !contains_any_directive(&comment_text, self.lang) {
                    let cleaned = clean_explanation_for_lang(&comment_text, self.lang);
                    if !cleaned.is_empty() {
                        parts.push(cleaned.to_string());
                    }
                }
            }
            curr_line = curr_line.saturating_sub(1);
        }

        if parts.is_empty() {
            return false;
        }
        parts.reverse();
        is_substantive_explanation(&parts.join(" "))
    }
}

/// Parsed `omni:` or `omnicoach:` suppression directive prefix.
pub(in crate::code_lint) struct OmniDirectivePrefix<'a> {
    /// True for `disable-file`, false for `ignore`.
    pub(in crate::code_lint) is_file: bool,
    /// Remainder of the comment segment after `ignore` or `disable-file`.
    pub(in crate::code_lint) remainder: &'a str,
}

const DIRECTIVE_NAMESPACES: &[&str] = &["omnicoach:", "omni:"];

/// Substrings, besides `noqa` in any case, of which every other third-party Python suppression
/// directive contains at least one.
const TOOL_DIRECTIVE_MARKERS: &[&str] = &["ignore", "disable", "skip-file"];

/// The bare `# noqa` directive, honored by Ruff and flake8 in any case.
pub(in crate::code_lint) const NOQA: &str = "noqa";

/// Fast pre-filter returning `true` if `content` may contain an `omni:`/`omnicoach:` suppression
/// directive in `lang`, or a third-party suppression directive in Python.
#[must_use]
pub(in crate::code_lint) fn may_contain_suppression_directive(
    content: &str,
    lang: Language,
) -> bool {
    DIRECTIVE_NAMESPACES
        .iter()
        .any(|namespace| content.contains(namespace))
        || (lang == Language::Python
            && (TOOL_DIRECTIVE_MARKERS
                .iter()
                .any(|marker| content.contains(marker))
                || content
                    .as_bytes()
                    .windows(NOQA.len())
                    .any(|window| window.eq_ignore_ascii_case(NOQA.as_bytes()))))
}

/// Parses an `omni:` or `omnicoach:` (`disable-file` / `ignore`) prefix at the start of `segment`
/// and validates boundary delimiters so prose such as `omni:ignored` is not matched.
#[must_use]
pub(in crate::code_lint) fn parse_omni_directive_prefix(
    segment: &str,
) -> Option<OmniDirectivePrefix<'_>> {
    let body = DIRECTIVE_NAMESPACES
        .iter()
        .find_map(|namespace| segment.strip_prefix(namespace))?;
    let (is_file, remainder) = if let Some(rest) = body.strip_prefix("disable-file") {
        (true, rest)
    } else {
        (false, body.strip_prefix("ignore")?)
    };
    if !remainder.is_empty()
        && !remainder.starts_with(|character: char| {
            character.is_whitespace() || matches!(character, '[' | '-' | ':' | '=')
        })
    {
        return None;
    }
    Some(OmniDirectivePrefix { is_file, remainder })
}

/// Splits a delimiter-stripped Python comment body into `#`-delimited directive segments.
///
/// Only splits at a `#` at byte `0` or preceded by whitespace when the text after `#` begins with
/// a recognized `omni:`/`omnicoach:` or third-party Python directive, so URL fragments such as
/// `https://docs.astral.sh/ruff/#noqa` and issue references such as `-- tracks #42` stay intact.
#[must_use]
pub(in crate::code_lint) fn split_python_directive_segments(stripped: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut segment_start = 0;
    for (byte_offset, character) in stripped.char_indices() {
        if character == '#'
            && (byte_offset == 0 || stripped[..byte_offset].ends_with(char::is_whitespace))
        {
            let after_hash = stripped[byte_offset + 1..].trim_start();
            if parse_omni_directive_prefix(after_hash).is_some()
                || starts_with_tool_directive(after_hash)
            {
                if byte_offset > segment_start {
                    segments.push(stripped[segment_start..byte_offset].trim_end());
                }
                segment_start = byte_offset + 1;
            }
        }
    }
    segments.push(stripped[segment_start..].trim_end());
    segments
}

/// Returns true if `segment` starts with a third-party directive as its tool accepts it: bare
/// `noqa` in any case (Ruff and flake8 honor `# NOQA`), the others in lowercase only (mypy,
/// pyright, ty, pyrefly, Ruff and pylint ignore `# TYPE: IGNORE` or `# PYLINT: DISABLE`).
#[must_use]
pub(in crate::code_lint) fn starts_with_tool_directive(segment: &str) -> bool {
    match_tool_directive_prefix_len(segment).is_some()
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

/// Returns true if `span` has only whitespace before `span.start` on its start line and only
/// whitespace after `span.end` on its end line.
#[must_use]
pub(in crate::code_lint) fn is_standalone_comment_span(source: &str, span: SourceSpan) -> bool {
    is_line_prefix_whitespace(source, span.start) && is_line_suffix_whitespace(source, span.end)
}

/// Returns true if the text on the line before byte `offset` consists only of whitespace.
#[must_use]
pub(in crate::code_lint) fn is_line_prefix_whitespace(source: &str, offset: usize) -> bool {
    let prefix_on_line = source[..offset]
        .rsplit_once('\n')
        .map_or(&source[..offset], |(_, prefix)| prefix);
    prefix_on_line.trim().is_empty()
}

/// Returns true if the text on the line after byte `offset` consists only of whitespace.
#[must_use]
pub(in crate::code_lint) fn is_line_suffix_whitespace(source: &str, offset: usize) -> bool {
    let suffix_on_line = source[offset..]
        .split_once('\n')
        .map_or(&source[offset..], |(suffix, _)| suffix);
    suffix_on_line.trim().is_empty()
}

#[derive(Clone)]
struct IndexedComment<'a> {
    node: AstNode<'a>,
    is_standalone: bool,
}

#[must_use]
fn contains_any_directive(text: &str, lang: Language) -> bool {
    let Some(stripped) = strip_comment_delimiters(text) else {
        return false;
    };
    if lang == Language::Python {
        split_python_directive_segments(stripped)
            .into_iter()
            .any(|segment| {
                let trimmed = segment.trim_start();
                parse_omni_directive_prefix(trimmed).is_some()
                    || starts_with_tool_directive(trimmed)
            })
    } else {
        parse_omni_directive_prefix(stripped).is_some()
    }
}

/// Strips standard linter/tooling directives (`noqa`, `type: ignore`, `pyright`, `pylint`, `omni:ignore`).
///
/// If the comment contains an explanatory reason after a separator (e.g. `-- reason`),
/// the directive prefix is stripped and the remaining reason text is returned.
/// If the comment consists solely of directives and rule codes, `""` is returned.
#[cfg(test)]
#[must_use]
fn clean_explanation(text: &str) -> &str {
    let lang = if text.trim_start().starts_with('#') {
        Language::Python
    } else {
        Language::Rust
    };
    clean_explanation_for_lang(text, lang)
}

#[must_use]
fn clean_explanation_for_lang(text: &str, lang: Language) -> &str {
    let stripped = strip_comment_delimiters(text).unwrap_or_else(|| text.trim());
    if stripped.is_empty() {
        return "";
    }
    if lang == Language::Python {
        return split_python_directive_segments(stripped)
            .into_iter()
            .rev()
            .map(|segment| clean_single_segment(segment.trim(), Language::Python))
            .find(|cleaned| !cleaned.is_empty())
            .unwrap_or("");
    }
    clean_single_segment(stripped, lang)
}

#[must_use]
fn clean_single_segment(stripped: &str, lang: Language) -> &str {
    let Some(prefix_len) = find_directive_prefix(stripped, lang) else {
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

/// Minimum words in an explanation; a `TODO` / `FIXME` note needs that many after its marker.
const MIN_EXPLANATION_WORDS: usize = 3;

/// Checks if an explanation text is substantive (at least 3 words and 10 non-whitespace chars).
#[must_use]
fn is_substantive_explanation(text: &str) -> bool {
    let words = text.split_whitespace().count();
    let chars = text.chars().filter(|c| !c.is_whitespace()).count();
    if words < MIN_EXPLANATION_WORDS || chars < 10 {
        return false;
    }

    // Disallow bare TODO/FIXME notes without substantive explanation
    let starts_with_todo = text
        .get(..4)
        .is_some_and(|sub| sub.eq_ignore_ascii_case("todo"));
    let starts_with_fixme = text
        .get(..5)
        .is_some_and(|sub| sub.eq_ignore_ascii_case("fixme"));
    !((starts_with_todo || starts_with_fixme) && words <= MIN_EXPLANATION_WORDS)
}

fn find_directive_prefix(text: &str, lang: Language) -> Option<usize> {
    if lang == Language::Python
        && let Some(len) = match_tool_directive_prefix_len(text)
    {
        return Some(len);
    }
    parse_omni_directive_prefix(text).map(|prefix| text.len() - prefix.remainder.len())
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

/// Checks if a whitespace-delimited word consists of rule codes (e.g. `SIM105`, `F401,`, `SIM105,F401`).
///
/// Rule codes must either contain digits (`SIM105`, `E501`), contain hyphens or dots
/// (`unused-import`, `pylint.errors`), or be all-uppercase category codes (`F`, `W`).
/// This prevents legitimate lowercase English explanation words (e.g. `safe`, `transient`)
/// from being erroneously swallowed as rule codes.
fn is_rule_code_token(word: &str) -> bool {
    let trimmed = word.trim_matches(|c: char| c == ',' || c == ';');
    !trimmed.is_empty()
        && trimmed.split(',').all(|part| {
            let segment = part.trim();
            let is_valid = !segment.is_empty()
                && segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
            let is_code = segment.chars().any(|c| c.is_ascii_digit())
                || segment.contains('-')
                || segment.contains('.')
                || segment.chars().all(|c| c.is_ascii_uppercase() || c == '_');
            is_valid && is_code
        })
}

/// Matches a third-party Python directive prefix at the start of `segment` and returns its byte
/// length.
///
/// Bare `noqa` matches in any case (Ruff and flake8 honor `# NOQA`); the others match in
/// lowercase only (mypy, pyright, ty, pyrefly, Ruff and pylint ignore `# TYPE: IGNORE` or
/// `# PYLINT: DISABLE`). Hyphenated keyword suffixes such as `disable-next`, `disable-line`,
/// and `ignore-errors` are included in the matched prefix length.
#[must_use]
fn match_tool_directive_prefix_len(segment: &str) -> Option<usize> {
    if starts_with_keyword(&segment.to_ascii_lowercase(), NOQA) {
        return Some(NOQA.len());
    }
    let (tool, rest) = segment.split_once(':')?;
    let directive = TOOL_DIRECTIVES.iter().find(|entry| entry.tool == tool)?;
    let trimmed_rest = rest.trim_start();
    let ws_len = rest.len() - trimmed_rest.len();
    let keyword = directive
        .keywords
        .iter()
        .copied()
        .find(|kw| starts_with_keyword(trimmed_rest, kw))?;
    let mut after_kw = &trimmed_rest[keyword.len()..];
    let mut suffix_len = 0;
    while after_kw.starts_with('-') && !after_kw.starts_with("--") {
        let word_len = after_kw[1..]
            .bytes()
            .take_while(u8::is_ascii_alphanumeric)
            .count();
        if word_len == 0 {
            break;
        }
        suffix_len += 1 + word_len;
        after_kw = &after_kw[1 + word_len..];
    }
    Some(tool.len() + 1 + ws_len + keyword.len() + suffix_len)
}

/// A third-party Python suppression directive, written `# <tool>: <keyword>...`.
struct ToolDirective {
    /// The tool namespace before the colon, such as `type` or `pylint`.
    tool: &'static str,
    /// The keywords after the colon that suppress findings, such as `ignore` or `disable`.
    keywords: &'static [&'static str],
}

/// Third-party Python suppression directives besides bare `# noqa`.
const TOOL_DIRECTIVES: &[ToolDirective] = &[
    ToolDirective {
        tool: "type",
        keywords: &["ignore"],
    },
    ToolDirective {
        tool: "pyright",
        keywords: &["ignore"],
    },
    ToolDirective {
        tool: "ty",
        keywords: &["ignore"],
    },
    ToolDirective {
        tool: "pyrefly",
        keywords: &["ignore"],
    },
    ToolDirective {
        tool: "ruff",
        keywords: &["noqa", "ignore", "disable", "file-ignore"],
    },
    ToolDirective {
        tool: "flake8",
        keywords: &["noqa"],
    },
    ToolDirective {
        tool: "pylint",
        keywords: &["disable", "skip-file"],
    },
];

/// Returns true if `text` starts with `keyword` as a whole word, so `ignore` matches
/// `ignore[code]` and `ignore-errors` but not `ignored`.
fn starts_with_keyword(text: &str, keyword: &str) -> bool {
    text.strip_prefix(keyword).is_some_and(|rest| {
        rest.chars()
            .next()
            .is_none_or(|next| !next.is_alphanumeric() && next != '_')
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::{Language, SourceSpan};
    use indoc::indoc;
    use rstest::rstest;

    /// Returns the span and 1-indexed line of the first occurrence of `target` in `source`.
    fn span_of(source: &str, target: &str) -> (SourceSpan, usize) {
        let start = source
            .find(target)
            .expect("target should exist in source snippet");
        let line = source[..start].matches('\n').count() + 1;
        (SourceSpan::new(start, start + target.len()), line)
    }

    #[rstest]
    #[case::plain_comment("# Safe because cache is transient", "Safe because cache is transient")]
    #[case::noqa_bare("# noqa: SIM105", "")]
    #[case::noqa_with_comment("# noqa: SIM105 -- file may be removed", "file may be removed")]
    #[case::noqa_with_plain_explanation(
        "# noqa: SIM105 safe because transient",
        "safe because transient"
    )]
    #[case::noqa_plain_words_without_code(
        "# noqa: safe because transient",
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
    #[case::pylint_disable_next("# pylint: disable-next=broad-except", "")]
    #[case::pylint_skip_file("# pylint: skip-file", "")]
    #[case::ty_ignore("# ty: ignore[invalid-argument-type]", "")]
    #[case::pyrefly_ignore_errors("# pyrefly: ignore-errors", "")]
    #[case::ruff_disable("# ruff: disable[E501]", "")]
    #[case::flake8_noqa("# flake8: noqa", "")]
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
    #[case::chained_bare_directives_stripped("# noqa: F401  # type: ignore[attr-defined]", "")]
    #[case::chained_directives_with_trailing_reason(
        "# noqa: F401  # type: ignore[attr-defined] -- re-exported for plugins",
        "re-exported for plugins"
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

    #[rstest]
    #[case::preceding_standalone_comment(2, true)]
    #[case::inline_explanation(5, true)]
    #[case::linter_directive_only_not_substantive(9, false)]
    #[case::uncommented(12, false)]
    fn test_comment_index_adjacent_explanation(#[case] line: usize, #[case] expected: bool) {
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

        let file = ParsedFile::new(source, Language::Python);
        let index = CommentIndex::from_file(&file);
        assert_eq!(index.has_adjacent_explanation(line), expected);
    }

    #[test]
    fn test_multiline_preceding_comment_block() {
        let source = indoc! {r"
            # The cache file might already be deleted
            # by another concurrent worker thread.
            with suppress(FileNotFoundError):
                pass
        "};

        let file = ParsedFile::new(source, Language::Python);
        let index = CommentIndex::from_file(&file);

        assert!(index.has_adjacent_explanation(3));
    }

    #[test]
    fn test_preceding_inline_comment_on_code_not_counted() {
        let source = indoc! {r"
            x = calculate()  # unrelated inline comment here
            with suppress(FileNotFoundError):
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let index = CommentIndex::from_file(&file);

        // Line 2 has no standalone comment preceding it; line 1 is code with inline comment
        assert!(!index.has_adjacent_explanation(2));
    }

    #[rstest]
    #[case::leading_block_comment_before_code_not_standalone(
        "/* substantive note on first line */ let first = 1;\nlet second = 2;",
        Language::Rust,
        2,
        false,
        false
    )]
    #[case::directive_reason_counts_for_code_but_not_stacked_directive(
        indoc! {r"
            # omni:ignore [nested-function] -- closure required for test fixture
            with suppress(FileNotFoundError):
                pass"},
        Language::Python,
        2,
        true,
        false
    )]
    #[case::prose_above_stacked_directives_counts_for_both(
        indoc! {r"
            # Shared prose reason covering stacked directives.
            # omni:ignore [nested-function]
            # omni:ignore [single-letter-name]
            a = 1"},
        Language::Python,
        3,
        true,
        true
    )]
    fn test_explanation_vs_prose_explanation_above(
        #[case] source: &str,
        #[case] language: Language,
        #[case] target_line: usize,
        #[case] expected_any: bool,
        #[case] expected_prose: bool,
    ) {
        let file = ParsedFile::new(source, language);
        let index = CommentIndex::from_file(&file);
        assert_eq!(index.has_explanation_above(target_line), expected_any);
        assert_eq!(
            index.has_prose_explanation_above(target_line),
            expected_prose
        );
    }

    #[rstest]
    #[case::multiline_with_preceding_comment(
        indoc! {r#"
            # Benign error ignored during cleanup
            with (
                open("file.txt"),
                suppress(FileNotFoundError),
            ):
                pass
        "#},
        "suppress(FileNotFoundError)",
        true
    )]
    #[case::singleline_with_trailing_inline_comment(
        indoc! {r"
            with (suppress(KeyError)): pass  # Inline explanation here
        "},
        "suppress(KeyError)",
        true
    )]
    #[case::singleline_uncommented(
        indoc! {r"
            with (suppress(ValueError)): pass
        "},
        "suppress(ValueError)",
        false
    )]
    #[case::multiline_with_inline_comment_on_header_line(
        indoc! {r#"
            with (
                open("file.txt"),
                suppress(KeyError),  # Config key is optional in legacy environments
            ):
                pass
        "#},
        "suppress(KeyError)",
        true
    )]
    #[case::body_comment_not_counted_for_header(
        indoc! {r"
            with suppress(Exception):
                # This comment is inside the body, not explaining the suppress
                pass
        "},
        "suppress(Exception)",
        false
    )]
    #[case::multiline_assignment_with_preceding_comment(
        indoc! {r#"
            # Fallback configuration when environment variable is missing
            CONFIG = (
                get_config("FALLBACK")
            )
        "#},
        "get_config",
        true
    )]
    #[case::annotated_assignment_colon_does_not_truncate_header(
        indoc! {r#"
            CONFIG: dict[str, str] = (
                get_config(
                    "FALLBACK",
                )  # Environment variable is absent in local development
            )
        "#},
        "get_config",
        true
    )]
    #[case::multiline_if_with_preceding_comment(
        indoc! {r"
            # Legacy environments do not expose the config key
            if (
                suppress(KeyError)
            ):
                pass
        "},
        "suppress(KeyError)",
        true
    )]
    #[case::multiline_while_with_preceding_comment(
        indoc! {r"
            # Retry transient failures until service becomes ready
            while (
                suppress(TimeoutError)
            ):
                pass
        "},
        "suppress(TimeoutError)",
        true
    )]
    #[case::multiline_with_trailing_comment_on_closing_paren(
        indoc! {r#"
            with (
                open("log.txt"),
                suppress(FileNotFoundError),
            ):  # Safe if lock file was already deleted
                pass
        "#},
        "suppress(FileNotFoundError)",
        true
    )]
    #[case::decorated_function_header_with_preceding_comment(
        indoc! {r"
            class Cache:
                # Returns None when the key has expired; an empty list means zero events.
                @staticmethod
                def cached_batch(key: str) -> list[int] | None:
                    return None
        "},
        "list[int] | None",
        true
    )]
    #[case::decorated_function_body_comment_not_counted_for_header(
        indoc! {r"
            @staticmethod
            def get_items() -> Sequence[str] | None:
                # Internal implementation comment inside the body block.
                return None
        "},
        "Sequence[str] | None",
        false
    )]
    fn test_has_explanation_for_span(
        #[case] source: &str,
        #[case] target_call: &str,
        #[case] expected: bool,
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let index = CommentIndex::from_file(&file);
        let (span, line) = span_of(source, target_call);

        assert_eq!(index.has_explanation_for_span(&file, span, line), expected);
    }

    #[rstest]
    #[case::block_body_comment_does_not_license_later_expression(
        indoc! {r"
            fn f() {
                let x = {
                    foo(); // Unrelated note about foo and its quirks
                    compute()
                };
            }
        "},
        "compute()",
        false
    )]
    #[case::attributed_function_header_with_preceding_comment(
        indoc! {r"
            // Returns None on cache miss; an empty slice means the user has no tags.
            #[must_use]
            fn cached_tags(&self, user_id: &str) -> Option<&[String]> {
                let _ = user_id;
                None
            }
        "},
        "Option<&[String]>",
        true
    )]
    #[case::attributed_function_body_comment_not_counted_for_header(
        indoc! {r"
            #[must_use]
            fn get_items() -> Option<Vec<String>> {
                // Internal implementation comment inside the body block.
                None
            }
        "},
        "Option<Vec<String>>",
        false
    )]
    #[case::outer_line_doc_comment_not_counted_as_explanation(
        indoc! {r"
            /// Returns None on cache miss; an empty slice means the user has no tags.
            #[must_use]
            fn cached_tags(&self, user_id: &str) -> Option<&[String]> {
                let _ = user_id;
                None
            }
        "},
        "Option<&[String]>",
        false
    )]
    #[case::outer_block_doc_comment_not_counted_as_explanation(
        indoc! {r"
            /** Returns None on cache miss; an empty slice means the user has no tags. */
            fn cached_tags(&self, user_id: &str) -> Option<&[String]> {
                let _ = user_id;
                None
            }
        "},
        "Option<&[String]>",
        false
    )]
    #[case::inner_doc_comments_not_counted_as_explanation(
        indoc! {r"
            //! Module-level doc comment with more than three words.
            /*! Block inner doc comment with more than three words. */
            fn cached_tags(&self, user_id: &str) -> Option<&[String]> {
                let _ = user_id;
                None
            }
        "},
        "Option<&[String]>",
        false
    )]
    #[case::regular_comment_above_doc_comment_counts_as_explanation(
        indoc! {r"
            // Returns None on cache miss; an empty slice means the user has no tags.
            /// Public API documentation for cached_tags.
            #[must_use]
            fn cached_tags(&self, user_id: &str) -> Option<&[String]> {
                let _ = user_id;
                None
            }
        "},
        "Option<&[String]>",
        true
    )]
    #[case::regular_comment_between_doc_comment_and_attribute_counts(
        indoc! {r"
            /// Public API documentation for cached_tags.
            // Returns None on cache miss; an empty slice means the user has no tags.
            #[must_use]
            fn cached_tags(&self, user_id: &str) -> Option<&[String]> {
                let _ = user_id;
                None
            }
        "},
        "Option<&[String]>",
        true
    )]
    #[case::four_slash_non_doc_comment_counts_as_explanation(
        indoc! {r"
            //// Returns None on cache miss; an empty slice means the user has no tags.
            fn cached_tags(&self, user_id: &str) -> Option<&[String]> {
                let _ = user_id;
                None
            }
        "},
        "Option<&[String]>",
        true
    )]
    fn test_has_explanation_for_span_rust(
        #[case] source: &str,
        #[case] target: &str,
        #[case] expected: bool,
    ) {
        let file = ParsedFile::new(source, Language::Rust);
        let index = CommentIndex::from_file(&file);
        let (span, line) = span_of(source, target);

        assert_eq!(index.has_explanation_for_span(&file, span, line), expected);
    }
}
