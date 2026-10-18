//! Flags docstring `Returns:` and `Yields:` sections that state the result can be `None` without
//! explaining when `None` is returned.

use crate::code_lint::ast::python::{
    DocstringSection, DocstringSectionKind, none_words, nullability_markers, without_byte_ranges,
};
use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "`{section}:` section of `{function}` says `{phrase}` without stating when `None` is returned.",
    rationale: "Callers learn that the result can be `None`, but not what condition produces it, so they have to read the implementation or guess.",
    suggestion: "Replace `{phrase}` with the condition under which `None` is returned, such as `or None if no record matches`.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("unexplained-optional-return"),
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
            summary: "Flags docstring `Returns:` and `Yields:` sections that say `or None` or `can be None` without stating when `None` is returned.",
            what_it_does: indoc::indoc! {r"
                Flags a `Returns:` or `Yields:` section of a function docstring whose prose ends a
                clause with a nullability marker (`… or None.`, `… | None.`, `Can be None.`, `May be
                None.`, `Could be None.`) and does not otherwise mention `None` (`Returns None when
                …`). A leading Napoleon type prefix (`User | None: The matching user.`), `or None`
                followed by more words in the same clause (`or None if no user matches`), `if any`,
                and other docstring sections (`Args:` and `Attributes:` are covered by
                `unexplained-optional-argument`) are not flagged."},
            why_is_this_bad: indoc::indoc! {r"
                The return annotation (`-> User | None`) already tells callers that the result can
                be `None`. What the annotation cannot express is *when* `None` is returned instead
                of raising or returning an empty value: on cache miss, when a record is absent, or
                when a timeout elapses. Repeating `or None.` at the end of `Returns:` restates the
                type without answering the question callers read the docstring to resolve."},
            known_problems: Some(indoc::indoc! {r"
                Removing `or None` silences the rule without adding the missing condition. A
                follow-up sentence that explains the `None` case without repeating the word `None`
                is still flagged."}),
            references: &[Reference {
                title: "Google Python Style Guide §3.8.3: Functions and Methods",
                url: "https://google.github.io/styleguide/pyguide.html#383-functions-and-methods",
            }],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r#"
                    def find_user(user_id: str) -> User | None:
                        """Looks up a user by identifier.

                        Args:
                            user_id: Identifier of the user to look up.

                        Returns:
                            The matching user, or None.
                        """
                "#},
                flagged_span: "The matching user, or None.",
                fixed: indoc::indoc! {r#"
                    def find_user(user_id: str) -> User | None:
                        """Looks up a user by identifier.

                        Args:
                            user_id: Identifier of the user to look up.

                        Returns:
                            The matching user, or None if no user has `user_id`.
                        """
                "#},
            }],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

/// Lowercase words that make a clause-final `None` in `Returns:` / `Yields:` a nullability marker.
const RETURN_NULLABILITY_LEADS: &[&str] = &["or", "|", "can be", "may be", "could be"];

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
            if let Some(marker) = unexplained_return_marker(&section) {
                diagnostics.push(rule.diagnostic_at_node(
                    path,
                    &docstring.line_node(marker.line_index),
                    &[
                        (SECTION, &section.header),
                        (FUNCTION, &signature.name),
                        (PHRASE, &marker.phrase),
                    ],
                ));
            }
        }
    }
    diagnostics
}

struct UnexplainedReturnMarker {
    line_index: usize,
    phrase: String,
}

/// The first nullability marker of `section` and its line index if the section is marked and its
/// remaining text does not mention `None`.
fn unexplained_return_marker(section: &DocstringSection) -> Option<UnexplainedReturnMarker> {
    let description = section.description()?;
    let markers = nullability_markers(&description, RETURN_NULLABILITY_LEADS);
    let first = markers.first()?;
    let line_index = section.line_at_description_offset(first.end.saturating_sub(1))?;
    let phrase = description[first.clone()].to_owned();
    let unmarked = without_byte_ranges(&description, &markers);
    none_words(&unmarked)
        .next()
        .is_none()
        .then_some(UnexplainedReturnMarker { line_index, phrase })
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                unmarked_returns_section => r#"
                    def find_user(user_id: str) -> User | None:
                        """Looks up a user.

                        Returns:
                            The matching user record, if any.
                        """
                "#,
                none_not_at_clause_end_exempt => r#"
                    def find_user(user_id: str) -> User | None:
                        """Looks up a user.

                        Returns:
                            The matching user, or None on cache eviction.
                        """
                "#,
                explained_by_none_exempt => r#"
                    def find_user(user_id: str) -> User | None:
                        """Looks up a user.

                        Returns:
                            The matching user, or None. None means the account was deleted.
                        """
                "#,
                lowercase_none_exempt => r#"
                    def matching_tags(query: str) -> list[str]:
                        """Finds matching tags.

                        Returns:
                            Matching tag names, or none.
                        """
                "#,
                napoleon_nullable_type_prefix_exempt => r#"
                    def find_user(user_id: str):
                        """Looks up a user.

                        Returns:
                            User | None: The matching user.
                        """
                "#,
                other_sections_not_checked => r#"
                    def connect(timeout: float | None = None) -> None:
                        """Opens a connection.

                        Args:
                            timeout: The timeout in seconds, or None.

                        Raises:
                            ValueError: If the host is empty or None.
                        """
                "#,
            ],
            fail: [
                or_none_at_end => r#"
                    def find_user(user_id: str) -> User | None:
                        """Looks up a user.

                        Returns:
                            The matching user, or None.
                        """
                "# => "The matching user, or None.",
                pipe_none_at_end => r#"
                    def find_user(user_id: str) -> User | None:
                        """Looks up a user.

                        Returns:
                            The matching user | None.
                        """
                "# => "The matching user | None.",
                can_be_none_sentence => r#"
                    def find_user(user_id: str) -> User | None:
                        """Looks up a user.

                        Returns:
                            The matching user; can be None.
                        """
                "# => "The matching user; can be None.",
                may_be_none_sentence => r#"
                    def find_user(user_id: str) -> User | None:
                        """Looks up a user.

                        Returns:
                            The matching user. May be None.
                        """
                "# => "The matching user. May be None.",
                could_be_none_sentence => r#"
                    def find_user(user_id: str) -> User | None:
                        """Looks up a user.

                        Returns:
                            The matching user. Could be None.
                        """
                "# => "The matching user. Could be None.",
                backticked_or_none => r#"
                    def find_user(user_id: str) -> User | None:
                        """Looks up a user.

                        Returns:
                            The matching user or `None`.
                        """
                "# => "The matching user or `None`.",
                yields_section_or_none => r#"
                    def iter_chunks(stream: bytes) -> Iterator[bytes | None]:
                        """Streams chunks.

                        Yields:
                            The next chunk, or None.
                        """
                "# => "The next chunk, or None.",
                multiline_returns_ending_or_none => r#"
                    def find_user(user_id: str) -> User | None:
                        """Looks up a user.

                        Returns:
                            The matching user record from the replica
                            or None.
                        """
                "# => "or None.",
                relative_clause_when_still_flagged => r#"
                    def start_time(job_id: str) -> int | None:
                        """Looks up a job start timestamp.

                        Returns:
                            The timestamp when the job started, or None.
                        """
                "# => "The timestamp when the job started, or None.",
                if_any_with_or_none_still_flagged => r#"
                    def find_user(user_id: str) -> User | None:
                        """Looks up a user.

                        Returns:
                            The matching user, if any, or None.
                        """
                "# => "The matching user, if any, or None.",
            ],
        },
    }
);
