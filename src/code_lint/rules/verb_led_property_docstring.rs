//! Flags `@property` getter docstrings whose summary opens with a call verb such as `Returns` or
//! `Gets`.

use crate::code_lint::ast::python::leading_verb;
use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Docstring of property `{function}` starts with `{phrase}`.",
    rationale: "A `@property` is read as an attribute (`obj.{function}`), not called as a method, so a leading verb describes a call that does not exist at the use site.",
    suggestion: "Replace the leading `{phrase}` with a noun phrase that describes the attribute value, such as `The active session` or `True if the connection is open`.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("verb-led-property-docstring"),
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
            summary: "Flags `@property` getter docstrings whose summary opens with a verb such as `Returns` or `Gets`.",
            what_it_does: indoc::indoc! {r"
                Flags a `@property` or `@cached_property` getter whose docstring summary starts with
                `Return`, `Returns`, `Get`, `Gets`, `Fetch`, `Fetches`, `Retrieve`, `Retrieves`,
                `Computes`, or `Calculates` (case-insensitive). Regular methods, `@<prop>.setter`
                and `@<prop>.deleter` accessors, and noun phrases such as `Return code of the
                process`, `Returns on equity`, or `Get request sent by the client` are not
                flagged."},
            why_is_this_bad: indoc::indoc! {r#"
                Callers read a `@property` as an attribute (`dataset.full_name`), not as a method
                call. The Google Python Style Guide therefore asks property docstrings to use the
                same noun-phrase style as attribute docstrings (`"""The project-qualified dataset
                name."""`) rather than verb-led function summaries (`"""Returns the
                project-qualified dataset name."""`)."#},
            known_problems: Some(indoc::indoc! {r"
                Only the fixed verb list is checked, so less common opening verbs (`Builds …`,
                `Reads …`) are not flagged. A `Return …` or `Get …` noun phrase whose second word
                is outside the fixed follower list is flagged. Custom property-like descriptors
                outside `@property` and `@cached_property` are not recognized."}),
            references: &[
                Reference {
                    title: "Google Python Style Guide §3.8.3: Functions and Methods",
                    url: "https://google.github.io/styleguide/pyguide.html#383-functions-and-methods",
                },
                Reference {
                    title: "Ruff D401: non-imperative-mood (property noun-phrase convention)",
                    url: "https://docs.astral.sh/ruff/rules/non-imperative-mood/",
                },
            ],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r#"
                    class Dataset:
                        """A dataset in a project."""

                        @property
                        def full_name(self) -> str:
                            """Returns the project-qualified dataset name."""
                            return f"{self._project}.{self._name}"
                "#},
                flagged_span: r#""""Returns the project-qualified dataset name.""""#,
                fixed: indoc::indoc! {r#"
                    class Dataset:
                        """A dataset in a project."""

                        @property
                        def full_name(self) -> str:
                            """The project-qualified dataset name."""
                            return f"{self._project}.{self._name}"
                "#},
            }],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

/// Opening verbs that describe a method call rather than an attribute value.
const PROPERTY_VERBS: &[&str] = &[
    "return",
    "returns",
    "get",
    "gets",
    "fetch",
    "fetches",
    "retrieve",
    "retrieves",
    "computes",
    "calculates",
];
/// Opening words in `PROPERTY_VERBS` that can also start a noun phrase.
const NOUN_STEMS: &[&str] = &["return", "returns", "get"];
/// Words that make a `Return` / `Returns` / `Get` opening a noun phrase (`Return code of …`,
/// `Returns on equity.`).
const NOUN_FOLLOWERS: &[&str] = &[
    "code",
    "codes",
    "value",
    "values",
    "type",
    "types",
    "status",
    "request",
    "requests",
    "address",
    "addresses",
    "path",
    "paths",
    "rate",
    "rates",
    "on",
    "of",
    "from",
];

const FUNCTION: &str = "function";
const PHRASE: &str = "phrase";

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for signature in ast::python::extract_function_signatures(file) {
        if !signature.is_property_getter() {
            continue;
        }
        let Some(docstring) = signature.docstring() else {
            continue;
        };
        let Some(summary_line) = docstring.summary_line() else {
            continue;
        };
        if let Some(verb) = leading_verb(
            &summary_line.text,
            PROPERTY_VERBS,
            NOUN_STEMS,
            NOUN_FOLLOWERS,
        ) {
            diagnostics.push(rule.diagnostic_at_node(
                path,
                &docstring.line_node(summary_line.index),
                &[(FUNCTION, &signature.name), (PHRASE, verb)],
            ));
        }
    }
    diagnostics
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                noun_phrase_property_docstring => r#"
                    class Connection:
                        @property
                        def is_open(self) -> bool:
                            """True if the underlying socket is open."""
                            return self._open

                        @property
                        def compute_units(self) -> int:
                            """Compute units allocated to the task."""
                            return self._units
                "#,
                regular_method_with_returns_exempt => r#"
                    class Connection:
                        def session_id(self) -> str:
                            """Returns the active session identifier."""
                            return self._session_id
                "#,
                property_accessors_exempt => r#"
                    class Connection:
                        @property
                        def timeout(self) -> float:
                            """The socket timeout in seconds."""
                            return self._timeout

                        @timeout.setter
                        def timeout(self, value: float) -> None:
                            """Returns nothing; sets the socket timeout."""
                            self._timeout = value

                        @timeout.deleter
                        def timeout(self) -> None:
                            """Gets rid of the custom timeout."""
                            del self._timeout
                "#,
                undocumented_property_exempt => r#"
                    class Connection:
                        @property
                        def is_open(self) -> bool:
                            return self._open
                "#,
                return_noun_phrase_exempt => r#"
                    class CompletedProcess:
                        @property
                        def status(self) -> int:
                            """Return-code of the child process."""
                            return self._status

                        @property
                        def request(self) -> Request:
                            """Get request sent by the client."""
                            return self._request

                        @property
                        def peer_address(self) -> str:
                            """Return address of the peer."""
                            return self._peer_address

                        @property
                        def equity_returns(self) -> float:
                            """Returns on invested capital."""
                            return self._roe
                "#,
                longer_word_starting_with_verb_exempt => r#"
                    class BatchResult:
                        @property
                        def keys(self) -> list[bytes]:
                            """Returned keys in wire order."""
                            return self._keys
                "#,
                only_first_word_checked => r#"
                    class ProbeResult:
                        @property
                        def is_healthy(self) -> bool:
                            """True if the probe returns zero."""
                            return self._code == 0
                "#,
            ],
            fail: [
                returns_summary => r#"
                    class Client:
                        @property
                        def session(self) -> str:
                            """Returns the active session."""
                            return self._session
                "# => r#""""Returns the active session.""""#,
                return_summary => r#"
                    class Client:
                        @property
                        def session(self) -> str:
                            """Return the active session."""
                            return self._session
                "# => r#""""Return the active session.""""#,
                gets_summary => r#"
                    class Client:
                        @property
                        def session(self) -> str:
                            """Gets the active session."""
                            return self._session
                "# => r#""""Gets the active session.""""#,
                fetches_summary => r#"
                    class Client:
                        @property
                        def session(self) -> str:
                            """Fetches the active session."""
                            return self._session
                "# => r#""""Fetches the active session.""""#,
                retrieves_summary => r#"
                    class Client:
                        @property
                        def session(self) -> str:
                            """Retrieves the active session."""
                            return self._session
                "# => r#""""Retrieves the active session.""""#,
                computes_summary => r#"
                    class Payload:
                        @property
                        def byte_size(self) -> int:
                            """Computes the total byte count."""
                            return len(self._raw)
                "# => r#""""Computes the total byte count.""""#,
                calculates_summary => r#"
                    class Payload:
                        @property
                        def byte_size(self) -> int:
                            """Calculates the total byte count."""
                            return len(self._raw)
                "# => r#""""Calculates the total byte count.""""#,
                cached_property_returns_summary => r#"
                    from functools import cached_property

                    class Table:
                        @cached_property
                        def schema(self) -> str:
                            """Returns the parsed schema."""
                            return self._raw_schema
                "# => r#""""Returns the parsed schema.""""#,
                multiline_docstring_opening_on_next_line => r#"
                    class Client:
                        @property
                        def session(self) -> str:
                            """
                            Returns the active session.

                            More details about the session.
                            """
                            return self._session
                "# => "Returns the active session.",
            ],
        },
    }
);
