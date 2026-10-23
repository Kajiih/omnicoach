//! Multiline string literal extraction across languages.

use super::tree::{AstNode, ParsedFile};

/// Minimum number of non-blank lines for a single-line literal with `\n` escapes to be
/// multiline text rather than a single line with leading or trailing line feeds.
const MIN_MULTILINE_NON_BLANK_LINES: usize = 2;

/// How a string literal spans multiple lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MultilineKind {
    /// The literal spans multiple source lines with an unescaped line break.
    Source {
        /// Canonical paths (resolved through [`crate::code_lint::ast::resolve_name`]) of the
        /// non-local calls (Python) or macro invocations (Rust) enclosing the literal within its
        /// innermost function or closure scope.
        enclosing_calls: Vec<String>,
    },
    /// The literal is written on one logical source line and embeds `\n` line breaks across at
    /// least two non-blank lines.
    Escaped {
        /// Number of decoded `\n` line breaks in the literal's value.
        newlines: usize,
    },
}

/// A string, byte-string, C-string, or f-string literal that spans multiple lines in source or
/// through `\n` escapes.
///
/// Documentation strings (Python docstrings and Rust `#[doc = ...]` attributes) and `insta`
/// inline snapshots (`@"..."`) are excluded.
pub struct MultilineString<'a> {
    /// The literal (or part of an implicitly concatenated literal).
    pub node: AstNode<'a>,
    /// How the literal spans multiple lines.
    pub kind: MultilineKind,
}

/// Collects all multiline string literals in `file`, in source order.
#[must_use]
pub fn collect_multiline_strings(file: &ParsedFile) -> Vec<MultilineString<'_>> {
    dispatch_lang!(file.lang(), collect_multiline_strings(file))
}

/// Returns the number of `\n` line breaks in `decoded` when `decoded` spans at least two
/// non-blank `\n`-separated lines.
pub(super) fn escaped_multiline_newlines(decoded: &[u8]) -> Option<usize> {
    let non_blank_lines = decoded
        .split(|&byte| byte == b'\n')
        .filter(|line| line.iter().any(|byte| !byte.is_ascii_whitespace()))
        .count();
    (non_blank_lines >= MIN_MULTILINE_NON_BLANK_LINES)
        .then(|| decoded.split(|&byte| byte == b'\n').count() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;

    #[rstest::rstest]
    #[case::single_line_no_newline(b"hello", None)]
    #[case::single_line_trailing_newline(b"hello\n", None)]
    #[case::blank_newlines_only(b"\n\n\n", None)]
    #[case::whitespace_lines_only(b" \n\t\n ", None)]
    #[case::single_line_bounded_by_newlines(b"\n## Configuration\n", None)]
    #[case::single_line_with_paragraph_break(b"line 1\n\n", None)]
    #[case::two_lines_one_newline(b"line 1\nline 2", Some(1))]
    #[case::two_lines_trailing_newline(
        indoc::indoc! {b"
            line 1
            line 2
        "},
        Some(2),
    )]
    #[case::two_lines_with_blank_line_between(
        indoc::indoc! {b"
            line 1

            line 2"},
        Some(2),
    )]
    #[case::three_lines_two_newlines(
        indoc::indoc! {b"
            line 1
            line 2
            line 3"},
        Some(2),
    )]
    fn test_escaped_multiline_newlines(#[case] decoded: &[u8], #[case] expected: Option<usize>) {
        assert_eq!(escaped_multiline_newlines(decoded), expected);
    }

    #[rstest::rstest]
    #[case::source_multiline_with_wrapper(
        Language::Python,
        indoc::indoc! {r#"
            from inspect import cleandoc as clean
            def local_wrap(s):
                return s
            x = local_wrap(clean("""
                line 1
                line 2
            """))
        "#},
        vec![(
            indoc::indoc! {r#"
                """
                    line 1
                    line 2
                """"#},
            MultilineKind::Source {
                enclosing_calls: vec!["inspect.cleandoc".to_string()],
            },
        )],
    )]
    #[case::python_escaped_strings_and_fstrings(
        Language::Python,
        indoc::indoc! {r#"
            a = "line 1\nline 2\nline 3"
            b = b"bytes 1\nbytes 2\n"
            c = f"head {x}\ntail {y}\n"
            d = r"raw 1\nraw 2\nraw 3"
            e = "\n## Section\n"
        "#},
        vec![
            (
                "\"line 1\\nline 2\\nline 3\"",
                MultilineKind::Escaped { newlines: 2 },
            ),
            (
                "b\"bytes 1\\nbytes 2\\n\"",
                MultilineKind::Escaped { newlines: 2 },
            ),
            (
                "f\"head {x}\\ntail {y}\\n\"",
                MultilineKind::Escaped { newlines: 2 },
            ),
        ],
    )]
    #[case::rust_escaped_strings_byte_strings_and_c_strings(
        Language::Rust,
        indoc::indoc! {r#"
            fn demo() {
                let s = "line 1\nline 2\nline 3";
                let b = b"bytes 1\nbytes 2\n";
                let c = c"cstr 1\ncstr 2\n";
                let r = r"raw 1\nraw 2\nraw 3";
            }
        "#},
        vec![
            (
                "\"line 1\\nline 2\\nline 3\"",
                MultilineKind::Escaped { newlines: 2 },
            ),
            (
                "b\"bytes 1\\nbytes 2\\n\"",
                MultilineKind::Escaped { newlines: 2 },
            ),
            (
                "c\"cstr 1\\ncstr 2\\n\"",
                MultilineKind::Escaped { newlines: 2 },
            ),
        ],
    )]
    fn test_collect_multiline_strings(
        #[case] lang: Language,
        #[case] source: &str,
        #[case] expected: Vec<(&str, MultilineKind)>,
    ) {
        let file = ParsedFile::new(source, lang);
        let actual: Vec<(String, MultilineKind)> = collect_multiline_strings(&file)
            .into_iter()
            .map(|item| (item.node.text().into_owned(), item.kind))
            .collect();
        let expected: Vec<(String, MultilineKind)> = expected
            .into_iter()
            .map(|(text, kind)| (text.to_string(), kind))
            .collect();
        assert_eq!(actual, expected);
    }
}
