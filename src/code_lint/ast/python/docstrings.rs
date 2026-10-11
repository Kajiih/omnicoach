//! Python docstrings: the docstring of a function or class body and its Google-style sections.

use super::{AstNode, ParsedFile};
use crate::code_lint::ast::span_from_ruff_range;
use crate::diagnostic::SourceSpan;
use ruff_python_ast::{Expr, Stmt};
use ruff_text_size::Ranged as _;

const QUOTE_CHARS: &[char] = &['"', '\''];

/// The docstring of a Python module, class, or function: the string literal that is the first
/// statement of its body.
#[derive(Clone)]
pub struct PythonDocstring<'a> {
    /// The string literal expression.
    node: AstNode<'a>,
    /// The literal's value after escape processing.
    text: String,
    /// True if each line of [`Self::text`] is the same line of the literal's source, which holds
    /// unless the literal is implicitly concatenated or contains a line-altering escape (`\n`,
    /// line continuation). [`Self::line_node`] relies on it.
    lines_match_source: bool,
}

impl<'a> PythonDocstring<'a> {
    /// The string literal expression node.
    #[must_use]
    pub const fn node(&self) -> AstNode<'a> {
        self.node
    }

    /// The docstring value after escape processing, without indentation removal.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The number of lines that contain non-whitespace text.
    #[must_use]
    pub fn non_blank_line_count(&self) -> usize {
        self.text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count()
    }

    /// The Google-style sections (`Args:`, `Returns:`, …) of the docstring, in order.
    #[must_use]
    pub fn sections(&self) -> Vec<DocstringSection> {
        parse_google_sections(&self.text)
    }

    /// The first non-blank line, without surrounding whitespace.
    #[must_use]
    pub fn summary(&self) -> Option<&str> {
        self.text
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
    }

    /// The first paragraph (the lines up to the first blank line after the summary), joined by
    /// single spaces.
    #[must_use]
    pub fn summary_paragraph(&self) -> String {
        self.text
            .lines()
            .map(str::trim)
            .skip_while(|line| line.is_empty())
            .take_while(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The text compared for equality between docstrings: whitespace runs collapsed to one
    /// space, lowercase, without trailing periods. Indentation and line wrapping do not matter.
    #[must_use]
    pub fn normalized(&self) -> String {
        normalize_docstring_text(&self.text)
    }

    /// The source node of line `line_index` (0-based, as in [`DocstringLine::index`]), without
    /// its indentation. Falls back to the whole docstring when the line is the opening line or
    /// cannot be mapped to the source (implicit concatenation or a line-altering escape).
    #[must_use]
    pub fn line_node(&self, line_index: usize) -> AstNode<'a> {
        if line_index == 0 || !self.lines_match_source {
            return self.node;
        }
        let span = self.node.span();
        let raw = &self.node.file.source[span.start..span.end];
        let Some((line_start, line)) = raw
            .split_inclusive('\n')
            .scan(0, |offset, line| {
                let start = *offset;
                *offset += line.len();
                Some((start, line))
            })
            .nth(line_index)
        else {
            return self.node;
        };
        let content = line.trim_end().trim_end_matches(QUOTE_CHARS).trim_end();
        let indent = content.len() - content.trim_start().len();
        if content.len() == indent {
            return self.node;
        }
        let start = span.start + line_start + indent;
        AstNode::from_span(
            self.node.file,
            SourceSpan::new(start, span.start + line_start + content.len()),
        )
    }
}

/// Returns the docstring of `body` (a module, class, or function body), if its first statement
/// is a plain string literal. Bytes and f-strings are not docstrings.
pub(super) fn docstring_of<'a>(
    body: &'a [Stmt],
    file: &'a ParsedFile,
) -> Option<PythonDocstring<'a>> {
    let Stmt::Expr(statement) = body.first()? else {
        return None;
    };
    let Expr::StringLiteral(literal) = statement.value.as_ref() else {
        return None;
    };
    let span = span_from_ruff_range(literal.range());
    let text = literal.value.to_str().to_owned();
    let raw = &file.source[span.start..span.end];
    let lines_match_source = !literal.value.is_implicit_concatenated()
        && raw.matches('\n').count() == text.matches('\n').count();
    Some(PythonDocstring {
        node: AstNode::from_span(file, span),
        text,
        lines_match_source,
    })
}

/// Normalizes docstring `text` for equality comparisons, as [`PythonDocstring::normalized`].
#[must_use]
pub fn normalize_docstring_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .trim_end_matches('.')
        .to_owned()
}

/// The kind of a Google-style docstring section, from its header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocstringSectionKind {
    /// `Args:`, `Arguments:`, `Parameters:`, `Params:`, `Keyword Args:`, `Keyword Arguments:`,
    /// `Other Parameters:`.
    Args,
    /// `Attributes:`.
    Attributes,
    /// `Returns:` or `Return:`.
    Returns,
    /// `Yields:` or `Yield:`.
    Yields,
    /// `Raises:`, `Raise:`, `Exceptions:`, `Except:`.
    Raises,
    /// Any other recognized header: `Example:`, `Note:`, `Warning:`, `Todo:`, `See Also:`, ….
    Other,
}

impl DocstringSectionKind {
    /// Returns true if the section lists named entries (`name: description`).
    #[must_use]
    pub const fn has_entries(self) -> bool {
        matches!(self, Self::Args | Self::Attributes | Self::Raises)
    }
}

/// Header names recognized as sections, lowercase, as in Sphinx Napoleon's Google style.
const SECTION_HEADERS: &[(&str, DocstringSectionKind)] = &[
    ("args", DocstringSectionKind::Args),
    ("arguments", DocstringSectionKind::Args),
    ("parameters", DocstringSectionKind::Args),
    ("params", DocstringSectionKind::Args),
    ("keyword args", DocstringSectionKind::Args),
    ("keyword arguments", DocstringSectionKind::Args),
    ("other parameters", DocstringSectionKind::Args),
    ("attributes", DocstringSectionKind::Attributes),
    ("returns", DocstringSectionKind::Returns),
    ("return", DocstringSectionKind::Returns),
    ("yields", DocstringSectionKind::Yields),
    ("yield", DocstringSectionKind::Yields),
    ("raises", DocstringSectionKind::Raises),
    ("raise", DocstringSectionKind::Raises),
    ("exceptions", DocstringSectionKind::Raises),
    ("except", DocstringSectionKind::Raises),
    ("example", DocstringSectionKind::Other),
    ("examples", DocstringSectionKind::Other),
    ("note", DocstringSectionKind::Other),
    ("notes", DocstringSectionKind::Other),
    ("warning", DocstringSectionKind::Other),
    ("warnings", DocstringSectionKind::Other),
    ("warns", DocstringSectionKind::Other),
    ("todo", DocstringSectionKind::Other),
    ("see also", DocstringSectionKind::Other),
    ("references", DocstringSectionKind::Other),
    ("methods", DocstringSectionKind::Other),
];

/// One line of a docstring section body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocstringLine {
    /// 0-based line index in the docstring text, for [`PythonDocstring::line_node`].
    pub index: usize,
    /// The line without leading and trailing whitespace.
    pub text: String,
}

impl DocstringLine {
    /// Splits a leading Napoleon type prefix off the line: `bool: True if ready.` gives
    /// `("bool", "True if ready.")`.
    ///
    /// The prefix is the text before the first `:` when it is non-empty, has no whitespace
    /// outside brackets (`dict[str, int]` qualifies, `Note that` does not), and the colon is
    /// followed by whitespace or the end of the line.
    #[must_use]
    pub fn split_type_prefix(&self) -> Option<(&str, &str)> {
        let mut depth = 0_usize;
        for (offset, character) in self.text.char_indices() {
            match character {
                '[' | '(' => depth += 1,
                ']' | ')' => depth = depth.saturating_sub(1),
                ':' if depth == 0 => {
                    let (prefix, rest) = (&self.text[..offset], &self.text[offset + 1..]);
                    let followed_by_space =
                        rest.is_empty() || rest.starts_with(char::is_whitespace);
                    return (!prefix.is_empty() && followed_by_space)
                        .then(|| (prefix, rest.trim_start()));
                }
                _ if depth == 0 && character.is_whitespace() => return None,
                _ => {}
            }
        }
        None
    }
}

/// A named entry of an `Args:`, `Attributes:`, or `Raises:` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocstringEntry {
    /// The documented name, with any `*` / `**` prefix (`*args`) or dotted path (`os.error`).
    pub name: String,
    /// The parenthesized type of `name (type): …`, without the parentheses.
    pub type_text: Option<String>,
    /// The description: the text after the colon and the continuation lines, joined by spaces.
    pub description: String,
    /// 0-based line index of the entry's first line in the docstring text.
    pub line_index: usize,
}

impl DocstringEntry {
    /// The documented name without a `*` / `**` prefix, to match it with a parameter name.
    #[must_use]
    pub fn bare_name(&self) -> &str {
        self.name.trim_start_matches('*')
    }
}

/// A Google-style docstring section: a header line ending in a colon and its indented body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocstringSection {
    /// The section kind.
    pub kind: DocstringSectionKind,
    /// The header text without the colon, as written (`"Keyword Args"`).
    pub header: String,
    /// 0-based line index of the header in the docstring text.
    pub header_line_index: usize,
    /// The non-blank body lines, in order.
    pub body: Vec<DocstringLine>,
    /// The named entries, for kinds where [`DocstringSectionKind::has_entries`]; empty otherwise.
    pub entries: Vec<DocstringEntry>,
}

/// Parses the Google-style sections of docstring `text`.
///
/// A section starts at a line that is only a recognized header followed by a colon, and ends at
/// the next non-blank line indented no deeper than its header. Entries are the body lines at the
/// body's first indentation level; deeper lines continue the previous entry.
#[must_use]
pub fn parse_google_sections(text: &str) -> Vec<DocstringSection> {
    let lines: Vec<&str> = text.lines().collect();
    let mut sections = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let Some((kind, header)) = section_header(lines[index]) else {
            index += 1;
            continue;
        };
        let header_indent = indentation(lines[index]);
        let header_line_index = index;
        let mut body = Vec::new();
        index += 1;
        while index < lines.len() {
            let line = lines[index];
            if !line.trim().is_empty() {
                if indentation(line) <= header_indent {
                    break;
                }
                body.push((index, line));
            }
            index += 1;
        }
        let entries = if kind.has_entries() {
            parse_entries(&body)
        } else {
            Vec::new()
        };
        sections.push(DocstringSection {
            kind,
            header: header.to_owned(),
            header_line_index,
            body: body
                .iter()
                .map(|&(line_index, line)| DocstringLine {
                    index: line_index,
                    text: line.trim().to_owned(),
                })
                .collect(),
            entries,
        });
    }
    sections
}

/// Returns the kind and header text if `line` is only a recognized section header and a colon.
fn section_header(line: &str) -> Option<(DocstringSectionKind, &str)> {
    let header = line.trim().strip_suffix(':')?.trim_end();
    let lowercase = header.to_ascii_lowercase();
    SECTION_HEADERS
        .iter()
        .find(|(name, _)| *name == lowercase)
        .map(|&(_, kind)| (kind, header))
}

/// Groups section body lines into entries: lines at the first line's indentation start an entry,
/// deeper lines continue it. Lines at the entry level that are not `name: …` are skipped.
fn parse_entries(body: &[(usize, &str)]) -> Vec<DocstringEntry> {
    let Some(&(_, first)) = body.first() else {
        return Vec::new();
    };
    let entry_indent = indentation(first);
    let mut entries: Vec<DocstringEntry> = Vec::new();
    let mut continues_entry = false;
    for &(line_index, line) in body {
        if indentation(line) > entry_indent {
            if continues_entry && let Some(entry) = entries.last_mut() {
                if !entry.description.is_empty() {
                    entry.description.push(' ');
                }
                entry.description.push_str(line.trim());
            }
            continue;
        }
        continues_entry = false;
        if let Some((name, type_text, description)) = split_entry_line(line.trim()) {
            entries.push(DocstringEntry {
                name: name.to_owned(),
                type_text: type_text.map(str::to_owned),
                description: description.to_owned(),
                line_index,
            });
            continues_entry = true;
        }
    }
    entries
}

/// Splits `name (type): description` (type and description optional) into its parts.
fn split_entry_line(line: &str) -> Option<(&str, Option<&str>, &str)> {
    let name_start = line.len() - line.trim_start_matches('*').len();
    if name_start > 2 {
        return None;
    }
    let name_end = line[name_start..]
        .find(|character: char| !(character.is_alphanumeric() || matches!(character, '_' | '.')))
        .map_or(line.len(), |offset| name_start + offset);
    if name_end == name_start {
        return None;
    }
    let name = &line[..name_end];
    let rest = line[name_end..].trim_start();
    let (type_text, rest) = match rest.strip_prefix('(') {
        Some(after_paren) => {
            let close = matching_paren(after_paren)?;
            (
                Some(after_paren[..close].trim()),
                after_paren[close + 1..].trim_start(),
            )
        }
        None => (None, rest),
    };
    let description = rest.strip_prefix(':')?.trim();
    Some((name, type_text, description))
}

/// Returns the byte offset of the `)` closing an already-opened parenthesis in `text`.
fn matching_paren(text: &str) -> Option<usize> {
    let mut depth = 0_usize;
    for (offset, character) in text.char_indices() {
        match character {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(offset),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// The number of leading whitespace characters of `line`.
fn indentation(line: &str) -> usize {
    line.chars()
        .take_while(|character| character.is_whitespace())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;
    use rstest::rstest;

    fn entry(
        name: &str,
        type_text: Option<&str>,
        description: &str,
        line_index: usize,
    ) -> DocstringEntry {
        DocstringEntry {
            name: name.to_owned(),
            type_text: type_text.map(str::to_owned),
            description: description.to_owned(),
            line_index,
        }
    }

    fn line(index: usize, text: &str) -> DocstringLine {
        DocstringLine {
            index,
            text: text.to_owned(),
        }
    }

    #[test]
    fn parses_google_example_sections_and_entries() {
        let text = "Fetches rows.\n\n    Args:\n        table_handle: An open table.\n        keys (Sequence[str]): Keys of each\n          row to fetch.\n        *args: Extra.\n\n    Returns:\n        A dict mapping keys to rows.\n\n    Raises:\n        IOError: An error occurred.\n    ";
        let expected = [
            DocstringSection {
                kind: DocstringSectionKind::Args,
                header: "Args".to_owned(),
                header_line_index: 2,
                body: vec![
                    line(3, "table_handle: An open table."),
                    line(4, "keys (Sequence[str]): Keys of each"),
                    line(5, "row to fetch."),
                    line(6, "*args: Extra."),
                ],
                entries: vec![
                    entry("table_handle", None, "An open table.", 3),
                    entry(
                        "keys",
                        Some("Sequence[str]"),
                        "Keys of each row to fetch.",
                        4,
                    ),
                    entry("*args", None, "Extra.", 6),
                ],
            },
            DocstringSection {
                kind: DocstringSectionKind::Returns,
                header: "Returns".to_owned(),
                header_line_index: 8,
                body: vec![line(9, "A dict mapping keys to rows.")],
                entries: Vec::new(),
            },
            DocstringSection {
                kind: DocstringSectionKind::Raises,
                header: "Raises".to_owned(),
                header_line_index: 11,
                body: vec![line(12, "IOError: An error occurred.")],
                entries: vec![entry("IOError", None, "An error occurred.", 12)],
            },
        ];
        assert_eq!(parse_google_sections(text), expected);
    }

    #[test]
    fn description_on_next_line_joins_continuation() {
        let text = "Doc.\n\nArgs:\n  keys:\n    A sequence\n    of keys.\n";
        let sections = parse_google_sections(text);
        assert_eq!(
            sections[0].entries,
            [entry("keys", None, "A sequence of keys.", 3)]
        );
    }

    #[rstest]
    #[case::prose_ending_in_colon("Doc.\n\nFor example:\n    x = 1\n")]
    #[case::header_with_trailing_text("Doc.\n\nArgs: none\n")]
    #[case::unknown_header("Doc.\n\nDetails:\n    x: y\n")]
    fn non_header_lines_are_not_sections(#[case] text: &str) {
        assert_eq!(parse_google_sections(text), []);
    }

    #[test]
    fn section_ends_at_dedent() {
        let text =
            "Doc.\n\n    Args:\n        x: X.\n    Trailing prose.\n        y: not an entry.\n";
        let sections = parse_google_sections(text);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].entries, [entry("x", None, "X.", 3)]);
    }

    #[rstest]
    #[case::nested_parens("x (dict[str, tuple(int)]): D.", Some(("x", Some("dict[str, tuple(int)]"), "D.")))]
    #[case::napoleon_optional("x (int, optional): D.", Some(("x", Some("int, optional"), "D.")))]
    #[case::kwargs("**kwargs: D.", Some(("**kwargs", None, "D.")))]
    #[case::dotted_exception("os.error: D.", Some(("os.error", None, "D.")))]
    #[case::empty_description("x:", Some(("x", None, "")))]
    #[case::no_colon("just prose", None)]
    #[case::sentence_with_colon("The value: D.", None)]
    #[case::unclosed_paren("x (int: D.", None)]
    #[case::three_stars("***x: D.", None)]
    fn splits_entry_lines(
        #[case] line: &str,
        #[case] expected: Option<(&str, Option<&str>, &str)>,
    ) {
        assert_eq!(split_entry_line(line), expected);
    }

    #[test]
    fn header_kinds_are_case_insensitive() {
        let sections = parse_google_sections("Doc.\n\nKeyword Args:\n  x: X.\nYIELDS:\n  Rows.\n");
        let kinds: Vec<_> = sections
            .iter()
            .map(|section| (section.kind, section.header.as_str()))
            .collect();
        assert_eq!(
            kinds,
            [
                (DocstringSectionKind::Args, "Keyword Args"),
                (DocstringSectionKind::Yields, "YIELDS")
            ]
        );
    }

    fn function_docstring(source: &str) -> Option<String> {
        let file = ParsedFile::new(source, Language::Python);
        let signatures = super::super::extract_function_signatures(&file);
        signatures[0]
            .docstring()
            .map(|docstring| docstring.text().to_owned())
    }

    #[rstest]
    #[case::plain("def f():\n    \"\"\"Doc.\"\"\"\n", Some("Doc."))]
    #[case::not_first_statement("def f():\n    x = 1\n    \"\"\"Doc.\"\"\"\n", None)]
    #[case::fstring_is_not_a_docstring("def f():\n    f\"\"\"Doc {x}.\"\"\"\n", None)]
    #[case::bytes_is_not_a_docstring("def f():\n    b\"\"\"Doc.\"\"\"\n", None)]
    fn extracts_function_docstring(#[case] source: &str, #[case] expected: Option<&str>) {
        assert_eq!(function_docstring(source).as_deref(), expected);
    }

    #[rstest]
    #[case::typed("bool: True if ready.", Some(("bool", "True if ready.")))]
    #[case::generic_with_space("dict[str, int]: Counts.", Some(("dict[str, int]", "Counts.")))]
    #[case::type_only("int:", Some(("int", "")))]
    #[case::prose_before_colon("Note that: x.", None)]
    #[case::no_space_after_colon("http://x", None)]
    #[case::no_colon("A dict of rows.", None)]
    #[case::empty_prefix(": x", None)]
    fn splits_type_prefix(#[case] text: &str, #[case] expected: Option<(&str, &str)>) {
        let line = DocstringLine {
            index: 1,
            text: text.to_owned(),
        };
        assert_eq!(line.split_type_prefix(), expected);
    }

    #[rstest]
    #[case::plain("x", "x")]
    #[case::args("*args", "args")]
    #[case::kwargs("**kwargs", "kwargs")]
    fn bare_name_strips_stars(#[case] name: &str, #[case] expected: &str) {
        assert_eq!(entry(name, None, "", 0).bare_name(), expected);
    }

    fn with_docstring<T>(source: &str, check: impl FnOnce(&PythonDocstring<'_>) -> T) -> T {
        let file = ParsedFile::new(source, Language::Python);
        let signatures = super::super::extract_function_signatures(&file);
        check(&signatures[0].docstring().expect("docstring"))
    }

    #[test]
    fn summary_accessors_read_the_first_paragraph() {
        let source = "def f():\n    \"\"\"\n    Fetches rows\n    from a table.\n\n    Args:\n        x: X.\n    \"\"\"\n";
        let summaries = with_docstring(source, |doc| {
            (doc.summary().map(str::to_owned), doc.summary_paragraph())
        });
        assert_eq!(
            summaries,
            (
                Some("Fetches rows".to_owned()),
                "Fetches rows from a table.".to_owned()
            )
        );
    }

    #[test]
    fn normalized_ignores_wrapping_case_and_trailing_period() {
        let normalized =
            with_docstring("def f():\n    \"\"\"Runs   the\n    JOB.\"\"\"\n", |doc| {
                doc.normalized()
            });
        assert_eq!(normalized, normalize_docstring_text("runs the job"));
    }

    #[test]
    fn class_docstring_is_extracted() {
        let file = ParsedFile::new("class C:\n    '''Doc.'''\n", Language::Python);
        let classes = super::super::extract_classes(&file);
        assert_eq!(
            classes[0].docstring.as_ref().map(PythonDocstring::text),
            Some("Doc.")
        );
    }

    #[test]
    fn line_node_points_at_entry_source_line() {
        let source =
            "def f(x, y):\n    \"\"\"Doc.\n\n    Args:\n        x: X.\n        y: Y.\"\"\"\n";
        let file = ParsedFile::new(source, Language::Python);
        let signatures = super::super::extract_function_signatures(&file);
        let docstring = signatures[0].docstring().expect("docstring");
        let entry_texts: Vec<String> = docstring.sections()[0]
            .entries
            .iter()
            .map(|entry| docstring.line_node(entry.line_index).text().into_owned())
            .collect();
        assert_eq!(entry_texts, ["x: X.", "y: Y."]);
    }

    #[rstest]
    #[case::escaped_newline("def f(x):\n    \"\"\"Doc.\\n\\nArgs:\\n  x: X.\"\"\"\n")]
    #[case::implicit_concatenation("def f(x):\n    (\"Doc.\\n\\nArgs:\\n\"\n     \"  x: X.\")\n")]
    fn line_node_falls_back_to_docstring_when_lines_differ(#[case] source: &str) {
        let file = ParsedFile::new(source, Language::Python);
        let signatures = super::super::extract_function_signatures(&file);
        let docstring = signatures[0].docstring().expect("docstring");
        let entry_line = docstring.sections()[0].entries[0].line_index;
        assert_eq!(
            docstring.line_node(entry_line).span(),
            docstring.node().span()
        );
    }
}
