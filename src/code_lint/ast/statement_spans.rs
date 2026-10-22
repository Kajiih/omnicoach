//! Statement header line ranges and leading decorator or attribute line extents.

use super::tree::ParsedFile;
use crate::diagnostic::SourceSpan;
use std::collections::HashMap;
use std::ops::RangeInclusive;

/// Resolves the 1-indexed inclusive header line range of the innermost statement enclosing `span`.
#[must_use]
pub fn enclosing_statement_header_range(
    file: &ParsedFile,
    span: SourceSpan,
) -> Option<RangeInclusive<usize>> {
    let header = dispatch_lang!(file.lang(), enclosing_statement_header_span(file, span))?;
    Some(file.line_index.line(header.start)..=file.line_index.line(header.end))
}

/// Maps each line where a decorator or attribute begins to the last line it spans.
///
/// Lines are 1-indexed. Covers Python `@decorator` and Rust `#[...]` attributes; when several
/// start on the same line, the largest ending line is kept.
#[must_use]
pub fn decorator_and_attribute_end_lines(file: &ParsedFile) -> HashMap<usize, usize> {
    let mut end_lines = HashMap::new();
    let mut record = |span: SourceSpan| {
        let start_line = file.line_index.line(span.start);
        let end_line = file.line_index.line(span.end);
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
    use crate::diagnostic::Language;

    /// Finds the byte span of the first occurrence of `target` in `source`.
    fn span_of(source: &str, target: &str) -> SourceSpan {
        let start = source
            .find(target)
            .expect("target snippet should exist in source");
        SourceSpan::new(start, start + target.len())
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
