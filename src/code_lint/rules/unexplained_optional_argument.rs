//! Flags docstring argument and attribute descriptions that call a value optional or nullable
//! without saying what `None` or omission does.
//!
//! Design: `docs/dev/docstring_optional_arg/03_design.md`. Only the `Args:`-kind and
//! `Attributes:` sections of function and class docstrings are read; markers and explanations
//! are matched as plain text. The signature is read only to exempt omission markers on a
//! parameter whose default is not `None`.

use crate::code_lint::ast::python::{
    DocstringEntry, DocstringSectionKind, PythonDocstring, PythonParameterInfo, is_clause_end,
    is_clause_start, none_words, nullability_markers, without_byte_ranges,
};
use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::ops::Range;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Description of `{name}` says `{phrase}` but not what a `None` or omitted value does.",
    rationale: "Callers learn that the value can be `None` or left out, but not what the code does then, so they read the implementation or guess.",
    suggestion: "Replace `{phrase}` with what a `None` or omitted `{name}` does, such as `If None, reads the default config file`.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("unexplained-optional-argument"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::DOCSTRINGS],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags docstring argument and attribute descriptions that call a value optional or nullable without saying what `None` or omission does.",
            what_it_does: indoc::indoc! {r"
                Flags an entry of a function or class docstring's `Args:` section (or `Keyword
                Args:`, `Parameters:`, …) or `Attributes:` section whose description says the
                value is optional (`Optional …`, `(Optional) …`, `Optional.`, `… (optional).`) or
                nullable (`… or None.`, `… | None.`, `Can be None.`, `Defaults to None.`) and
                does not otherwise mention `None` or what happens when the value is omitted (`If
                None, …`, `If omitted, …`, `by default …`). A type slot that is exactly
                `(optional)` counts as `Optional`. `or None` followed by more words (`or None to
                read from stdin`), `if any`, `Optionally …` and `optional` after the first word
                are not flagged. An `Optional` marker on a parameter whose default is not `None`
                (`retries: int = 3`) is not flagged either: the signature states the default.
                The Napoleon slot `(int, optional)` is not read: it is a type, the finding of
                `annotated-type-in-docstring`, whose fix leaves `(optional)`, which this rule
                then flags."},
            why_is_this_bad: indoc::indoc! {r#"
                The signature already says that a parameter has a default or accepts `None`;
                the docstring is the only place that can say what that means: wait forever or
                use a library timeout, the root node or a detached one. Google's guide asks a
                docstring to give "enough information to write a call to the function without
                reading the function's code"."#},
            known_problems: Some(indoc::indoc! {r"
                Removing the marker silences the rule without adding the missing meaning. A
                marker on a value whose absence is self-explanatory (`Optional callback run on
                completion.`) is flagged; `optional` after the first word (`An optional
                callback`) is not. `Attributes:` entries and `**kwargs` keys are never matched
                with a signature, so their `Optional` markers are flagged even when the value
                has a default."}),
            references: &[Reference {
                title: "Google Python Style Guide §3.8.3: Functions and Methods",
                url: "https://google.github.io/styleguide/pyguide.html#383-functions-and-methods",
            }],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r#"
                    def connect(host: str, timeout: float | None = None) -> Connection:
                        """Opens a connection to `host`.

                        Args:
                            host: Server name or address.
                            timeout: Optional timeout in seconds.
                        """
                "#},
                flagged_span: "timeout: Optional timeout in seconds.",
                fixed: indoc::indoc! {r#"
                    def connect(host: str, timeout: float | None = None) -> Connection:
                        """Opens a connection to `host`.

                        Args:
                            host: Server name or address.
                            timeout: Seconds to wait for the handshake. If None, waits indefinitely.
                        """
                "#},
            }],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

const OPTIONAL: &str = "optional";
const NONE: &str = "None";
/// Lowercase words that make a clause-final `None` a nullability marker (`or None.`).
const NULLABILITY_LEADS: &[&str] = &[
    "or",
    "|",
    "can be",
    "may be",
    "could be",
    "defaults to",
    "default:",
    "default is",
    "default",
];
/// Lowercase phrases that say what an omitted value does.
const OMISSION_PHRASES: &[&str] = &[
    "if omitted",
    "when omitted",
    "if not ",
    "when not ",
    "unless ",
    "if unset",
    "if absent",
    "if missing",
    "if empty",
    "by default",
    "defaults to",
    "default:",
    "default is",
    "otherwise",
    "falls back",
];

const NAME: &str = "name";
const PHRASE: &str = "phrase";

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for signature in ast::python::extract_function_signatures(file) {
        if let Some(docstring) = signature.docstring() {
            check_docstring(
                rule,
                path,
                &docstring,
                &signature.parameters,
                &mut diagnostics,
            );
        }
    }
    for class in ast::python::extract_classes(file) {
        let Some(docstring) = &class.docstring else {
            continue;
        };
        let constructor_parameters = class
            .constructor()
            .map(|constructor| constructor.parameters)
            .unwrap_or_default();
        check_docstring(
            rule,
            path,
            docstring,
            &constructor_parameters,
            &mut diagnostics,
        );
    }
    diagnostics
}

/// Reports the unexplained entries of the `Args:` and `Attributes:` sections of `docstring`;
/// `parameters` belong to the documented signature and are matched with `Args:` entries.
fn check_docstring(
    rule: &CodeRule,
    path: &Path,
    docstring: &PythonDocstring<'_>,
    parameters: &[PythonParameterInfo<'_>],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for section in docstring.sections() {
        let section_parameters: &[PythonParameterInfo<'_>] = match section.kind {
            DocstringSectionKind::Args => parameters,
            DocstringSectionKind::Attributes => &[],
            _ => continue,
        };
        for entry in &section.entries {
            let has_value_default = section_parameters.iter().any(|parameter| {
                parameter.name == entry.bare_name()
                    && parameter
                        .default_node
                        .is_some_and(|default| default.text() != NONE)
            });
            if let Some(phrase) = unexplained_marker(entry, has_value_default) {
                diagnostics.push(rule.diagnostic_at_node(
                    path,
                    &docstring.line_node(entry.line_index),
                    &[(NAME, &entry.name), (PHRASE, &phrase)],
                ));
            }
        }
    }
}

/// The first marker of `entry`, as written, if the entry is marked and its description does not
/// explain what `None` or omission does. Omission markers are ignored when `has_value_default`.
fn unexplained_marker(entry: &DocstringEntry, has_value_default: bool) -> Option<String> {
    let description = entry.description.as_str();
    let mut markers = nullability_markers(description, NULLABILITY_LEADS);
    let type_slot_marker = if has_value_default {
        None
    } else {
        markers.extend(omission_markers(description));
        entry.optional_marker().map(|slot| format!("({slot})"))
    };
    markers.sort_by_key(|marker| marker.start);
    let phrase = type_slot_marker.or_else(|| {
        markers
            .first()
            .map(|marker| description[marker.clone()].to_owned())
    })?;
    let unmarked = without_byte_ranges(description, &markers);
    (!is_explained(&unmarked)).then_some(phrase)
}

/// Byte ranges of the omission markers in `description`: a leading `Optional`, a clause that is
/// only `Optional`, and a clause-final `(optional)`, in any case.
fn omission_markers(description: &str) -> Vec<Range<usize>> {
    let lowercase = description.to_ascii_lowercase();
    let mut markers: Vec<Range<usize>> = leading_optional(&lowercase).into_iter().collect();
    for (start, _) in lowercase.match_indices(OPTIONAL) {
        let end = start + OPTIONAL.len();
        let (before, after) = (&lowercase[..start], &lowercase[end..]);
        if before.ends_with('(')
            && let Some(after_paren) = after.strip_prefix(')')
            && is_clause_end(after_paren)
        {
            markers.push(start - 1..end + 1);
        } else if is_clause_start(before) && is_clause_end(after) {
            markers.push(start..end);
        }
    }
    markers
}

/// The range of a leading `optional` in `lowercase` that is followed by a non-letter, including
/// an enclosing `(…)` or `[…]` (`(optional) …`).
fn leading_optional(lowercase: &str) -> Option<Range<usize>> {
    let opened = lowercase.strip_prefix(['(', '[']);
    let after = opened.unwrap_or(lowercase).strip_prefix(OPTIONAL)?;
    if after.starts_with(char::is_alphabetic) {
        return None;
    }
    let is_closed = opened.is_some() && after.starts_with([')', ']']);
    Some(0..lowercase.len() - after.len() + usize::from(is_closed))
}

/// Returns true if `text` mentions `None` or says what an omitted value does.
fn is_explained(text: &str) -> bool {
    let lowercase = text.to_ascii_lowercase();
    none_words(text).next().is_some()
        || OMISSION_PHRASES
            .iter()
            .any(|phrase| lowercase.contains(phrase))
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                unmarked_descriptions => r#"
                    def connect(host: str, parent: Node | None = None, retries: int = 3,
                                callback: Callable[[], None] | None = None) -> None:
                        """Opens a connection.

                        Args:
                            host: Server name or address.
                            parent: The parent node, if any.
                            retries: Number of attempts. Defaults to 3.
                            callback: An optional callback run on completion.
                        """
                "#,
                none_not_at_clause_end_exempt => r#"
                    def read(path: str | None = None) -> str:
                        """Reads a file.

                        Args:
                            path: A file path, or None to read from stdin.
                        """
                "#,
                explained_by_none_exempt => r#"
                    def wait(timeout: float | None = None) -> None:
                        """Waits for the job.

                        Args:
                            timeout: Optional timeout in seconds. If None, waits forever.
                        """
                "#,
                explained_by_omission_phrase_exempt => r#"
                    def fetch(retries: int | None = None) -> None:
                        """Fetches the page.

                        Args:
                            retries: Optional retry count; by default the client's setting.
                        """
                "#,
                lowercase_none_exempt => r#"
                    def keep(tags: list[str]) -> None:
                        """Filters tags.

                        Args:
                            tags: Tags to keep, or none.
                        """
                "#,
                optional_not_first_word_exempt => r#"
                    def validate(strict: bool) -> None:
                        """Validates a record.

                        Args:
                            strict: Whether a missing field is optional.
                        """
                "#,
                optionally_exempt => r#"
                    def render(sort: bool) -> None:
                        """Renders the table.

                        Args:
                            sort: Optionally sorts the output.
                        """
                "#,
                napoleon_optional_type_slot_exempt => r#"
                    def fetch(retries=None):
                        """Fetches the page.

                        Args:
                            retries (int, optional): Number of attempts before giving up.
                        """
                "#,
                other_sections_not_checked => r#"
                    def lookup(key: str) -> str | None:
                        """Looks up a row.

                        Returns:
                            The row, or None.

                        Raises:
                            KeyError: If the key is malformed or None.
                        """
                "#,
                omission_marker_with_value_default_exempt => r#"
                    def fetch(retries: int = 3) -> None:
                        """Fetches the page.

                        Args:
                            retries: Optional number of retries.
                        """
                "#,
                constructor_value_default_exempt => r#"
                    class Client:
                        """A client.

                        Args:
                            retries: Optional number of retries.
                        """

                        def __init__(self, retries: int = 3) -> None:
                            self.retries = retries
                "#,
                continuation_line_with_colon_not_an_entry => r#"
                    def fetch(keys: list[str]) -> None:
                        """Fetches rows.

                        Args:
                            keys: Keys to fetch.
                                Format: Optional prefix then id.
                        """
                "#,
                module_docstring_not_checked => r#"
                    """Client settings.

                    Attributes:
                        timeout: Optional timeout in seconds.
                    """
                "#,
            ],
            fail: [
                leading_optional => r#"
                    def wait(timeout: float | None = None) -> None:
                        """Waits for the job.

                        Args:
                            timeout: Optional timeout.
                        """
                "# => "timeout: Optional timeout.",
                optional_followed_by_colon => r#"
                    def wait(timeout: float | None = None) -> None:
                        """Waits for the job.

                        Args:
                            timeout: Optional: the timeout.
                        """
                "# => "timeout: Optional: the timeout.",
                parenthesized_leading_optional => r#"
                    def wait(timeout: float | None = None) -> None:
                        """Waits for the job.

                        Args:
                            timeout: (Optional) The timeout.
                        """
                "# => "timeout: (Optional) The timeout.",
                optional_sentence => r#"
                    def wait(timeout: float | None = None) -> None:
                        """Waits for the job.

                        Args:
                            timeout: The timeout. Optional.
                        """
                "# => "timeout: The timeout. Optional.",
                trailing_parenthesized_optional => r#"
                    def wait(timeout: float | None = None) -> None:
                        """Waits for the job.

                        Args:
                            timeout: The timeout (optional).
                        """
                "# => "timeout: The timeout (optional).",
                or_none => r#"
                    def attach(parent: Node | None = None) -> None:
                        """Attaches the node.

                        Args:
                            parent: The parent node or None.
                        """
                "# => "parent: The parent node or None.",
                or_backticked_none => r#"
                    def attach(parent: Node | None = None) -> None:
                        """Attaches the node.

                        Args:
                            parent: The parent node or `None`.
                        """
                "# => "parent: The parent node or `None`.",
                pipe_none => r#"
                    def scale(factor: float | None = None) -> None:
                        """Scales the image.

                        Args:
                            factor: A float | None.
                        """
                "# => "factor: A float | None.",
                can_be_none => r#"
                    def attach(parent: Node | None = None) -> None:
                        """Attaches the node.

                        Args:
                            parent: The parent; can be None.
                        """
                "# => "parent: The parent; can be None.",
                defaults_to_none => r#"
                    def run(param2: str | None = None) -> None:
                        """Runs the job.

                        Args:
                            param2: The second parameter. Defaults to None.
                        """
                "# => "param2: The second parameter. Defaults to None.",
                description_on_next_line => r#"
                    def wait(timeout: float | None = None) -> None:
                        """Waits for the job.

                        Args:
                            timeout:
                                Optional timeout.
                        """
                "# => "timeout:",
                marker_on_wrapped_line => r#"
                    def attach(parent: Node | None = None) -> None:
                        """Attaches the node.

                        Args:
                            parent: The parent node
                                or None.
                        """
                "# => "parent: The parent node",
                keyword_args_entry => r#"
                    def connect(**kwargs: str) -> None:
                        """Opens a connection.

                        Keyword Args:
                            proxy: Optional proxy URL.
                        """
                "# => "proxy: Optional proxy URL.",
                class_attributes_entry => r#"
                    class Client:
                        """A client.

                        Attributes:
                            retries: Optional number of retries.
                        """

                        def __init__(self, retries: int = 3) -> None:
                            self.retries = retries
                "# => "retries: Optional number of retries.",
                class_args_entry => r#"
                    class Client:
                        """A client.

                        Args:
                            timeout: Optional timeout.
                        """

                        def __init__(self, timeout: float | None = None) -> None:
                            self.timeout = timeout
                "# => "timeout: Optional timeout.",
                optional_type_slot => r#"
                    def count(limit=None):
                        """Counts rows.

                        Args:
                            limit (optional): The maximum count.
                        """
                "# => "limit (optional): The maximum count.",
                uppercase_header => r#"
                    def wait(timeout: float | None = None) -> None:
                        """Waits for the job.

                        ARGS:
                            timeout: Optional timeout.
                        """
                "# => "timeout: Optional timeout.",
            ],
        },
    }
);
