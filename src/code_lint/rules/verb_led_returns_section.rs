//! Flags docstring `Returns:` and `Yields:` sections that start with the verb they repeat.
//!
//! Design: `docs/dev/redundant_returns_docstring/03_design.md`. Only the opening word of the
//! section counts; the summary line (Google allows a `Returns …` summary) and other sections are
//! not read.

use crate::code_lint::ast::python::{DocstringLine, DocstringSectionKind};
use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "`{section}:` section of `{function}` starts with `{phrase}`.",
    rationale: "The section header already says that a value is returned, so the leading verb repeats it and the text describes the call again instead of the value the caller receives.",
    suggestion: "Remove the leading `{phrase}` so the section starts with a noun phrase that describes the value, such as `True if the device is ready` or `A dict mapping keys to rows`.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("verb-led-returns-section"),
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
            summary: "Flags docstring `Returns:` and `Yields:` sections that start with the verb `Returns` or `Yields`.",
            what_it_does: indoc::indoc! {r"
                Flags a `Returns:` or `Yields:` section of a function docstring whose first word is
                `Return`, `Returns`, `Yield` or `Yields`, in any case, including after a type prefix
                such as `bool:`. Only the opening word counts; the summary line and other sections
                are not checked, so a docstring that starts with `Returns …` and has no section is
                fine. `Return code …`, `Return value …` and similar noun phrases are not flagged."},
            why_is_this_bad: indoc::indoc! {r"
                Rendered documentation reads `Returns: Returns True if …`. Google style asks the
                section to describe the semantics of the returned value; a leading verb turns it
                into a second summary of the call."},
            known_problems: Some(indoc::indoc! {r"
                Restatements that do not start with the verb (`This function returns …`, `The
                return value is …`) are not flagged. A noun reading outside the fixed list is
                flagged (`Return address of the peer.`). A header followed by text on the same line
                (`Returns: The count.`) is not parsed as a section and is not checked."}),
            references: &[Reference {
                title: "Google Python Style Guide §3.8.3: Functions and Methods",
                url: "https://google.github.io/styleguide/pyguide.html#383-functions-and-methods",
            }],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r#"
                    def is_ready(device_id: str) -> bool:
                        """Checks a device.

                        Returns:
                            Returns True if the device is ready.
                        """
                "#},
                flagged_span: "Returns True if the device is ready.",
                fixed: indoc::indoc! {r#"
                    def is_ready(device_id: str) -> bool:
                        """Checks a device.

                        Returns:
                            True if the device is ready.
                        """
                "#},
            }],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

/// Opening words that repeat the section header.
const SECTION_VERBS: &[&str] = &["return", "returns", "yield", "yields"];
/// Bare `Return` / `Yield` openings that read as a noun.
const NOUN_STEMS: &[&str] = &["return", "yield"];
/// Words that make a bare `Return` / `Yield` opening a noun phrase (`Return code of …`).
const NOUN_FOLLOWERS: &[&str] = &[
    "code", "codes", "value", "values", "type", "types", "status",
];

const SECTION: &str = "section";
const FUNCTION: &str = "function";
const PHRASE: &str = "phrase";

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for signature in ast::python::extract_function_signatures(file) {
        let Some(docstring) = signature.docstring() else {
            continue;
        };
        for section in docstring.sections() {
            if !matches!(
                section.kind,
                DocstringSectionKind::Returns | DocstringSectionKind::Yields
            ) {
                continue;
            }
            let Some(opening_line) = section.body.first() else {
                continue;
            };
            if let Some(verb) = opening_verb(opening_line) {
                diagnostics.push(rule.diagnostic_at_node(
                    path,
                    &docstring.line_node(opening_line.index),
                    &[
                        (SECTION, &section.header),
                        (FUNCTION, &signature.name),
                        (PHRASE, verb),
                    ],
                ));
            }
        }
    }
    diagnostics
}

/// The opening word of `line` (after a `type:` prefix) if it is a section verb read as a verb.
fn opening_verb(line: &DocstringLine) -> Option<&str> {
    let text = line
        .split_type_prefix()
        .map_or(line.text.as_str(), |typed| typed.description);
    let word = leading_word(text);
    let lowercase = word.to_ascii_lowercase();
    if !SECTION_VERBS.contains(&lowercase.as_str()) {
        return None;
    }
    let next_word = leading_word(text[word.len()..].trim_start()).to_ascii_lowercase();
    let is_noun_phrase =
        NOUN_STEMS.contains(&lowercase.as_str()) && NOUN_FOLLOWERS.contains(&next_word.as_str());
    (!is_noun_phrase).then_some(word)
}

/// The run of alphanumeric characters at the start of `text` (empty if it starts otherwise).
fn leading_word(text: &str) -> &str {
    let end = text
        .find(|character: char| !character.is_alphanumeric())
        .unwrap_or(text.len());
    &text[..end]
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                noun_phrase_sections => r#"
                    def fetch_rows(keys: list[str]) -> dict[str, str]:
                        """Fetches rows.

                        Returns:
                            A dict mapping keys to rows.
                        """

                    def iterate_rows() -> Iterator[str]:
                        """Iterates rows.

                        Yields:
                            Each row, in key order.
                        """
                "#,
                return_noun_phrase_exempt => r#"
                    def run_command(argv: list[str]) -> int:
                        """Runs a command.

                        Returns:
                            Return code of the process.
                        """
                "#,
                longer_word_starting_with_verb_exempt => r#"
                    def fetch_rows(keys: list[str]) -> dict[bytes, str]:
                        """Fetches rows.

                        Returns:
                            Returned keys are always bytes.
                        """
                "#,
                only_first_body_line_checked => r#"
                    def fetch_rows(keys: list[str]) -> dict[str, str]:
                        """Fetches rows.

                        Returns:
                            A dict of rows.
                            Returns an empty dict when nothing matches.
                        """
                "#,
                only_first_word_checked => r#"
                    def is_ready(device_id: str) -> bool:
                        """Checks a device.

                        Returns:
                            True if ready; returns False otherwise.
                        """
                "#,
                summary_line_not_checked => r#"
                    def fetch_row(key: str) -> tuple[str, ...]:
                        """Returns the row from Bigtable as a tuple of strings."""
                "#,
                other_sections_not_checked => r#"
                    def register(callback: Callable[[], int]) -> None:
                        """Registers a callback.

                        Args:
                            callback: Returns the new value.
                        """
                "#,
            ],
            fail: [
                returns_section_starts_with_returns => r#"
                    def is_ready(device_id: str) -> bool:
                        """Checks a device.

                        Returns:
                            Returns True if the device is ready.
                        """
                "# => "Returns True if the device is ready.",
                yields_section_starts_with_yields => r#"
                    def iterate_rows() -> Iterator[str]:
                        """Iterates rows.

                        Yields:
                            Yields each row.
                        """
                "# => "Yields each row.",
                yields_section_starts_with_returns => r#"
                    def iterate_rows() -> Iterator[str]:
                        """Iterates rows.

                        Yields:
                            Returns each row.
                        """
                "# => "Returns each row.",
                lowercase_verb => r#"
                    def owner(alloc_id: str) -> str:
                        """Looks up an owner.

                        Returns:
                            returns the alloc owner.
                        """
                "# => "returns the alloc owner.",
                verb_after_type_prefix => r#"
                    def is_ready(device_id: str):
                        """Checks a device.

                        Returns:
                            bool: Returns True if ready.
                        """
                "# => "bool: Returns True if ready.",
                singular_return_header => r#"
                    def count_rows(table: str) -> int:
                        """Counts rows.

                        Return:
                            Returns the count.
                        """
                "# => "Returns the count.",
                second_section_checked => r#"
                    def stream_rows(table: str) -> Iterator[str]:
                        """Streams rows.

                        Raises:
                            KeyError: Returns nothing; the table is missing.

                        Yields:
                            Yields each row.
                        """
                "# => "Yields each row.",
                method_docstring => r#"
                    class Store:
                        def count(self) -> int:
                            """Counts entries.

                            Returns:
                                Returns the number of entries.
                            """
                "# => "Returns the number of entries.",
                bare_return_verb_without_noun => r#"
                    def first_row(table: str) -> str:
                        """Reads a row.

                        Returns:
                            Return the first row.
                        """
                "# => "Return the first row.",
            ],
        },
    }
);
