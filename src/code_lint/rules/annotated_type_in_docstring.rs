//! Flags docstring types that repeat a type annotation.
//!
//! Design: `docs/dev/redundant_type_docstring/03_design.md` (proposed there as
//! `repeated-docstring-type`). Unannotated parameters and attributes are never flagged, and a
//! parenthetical that is only `optional` or a `default …` note has no type.

use crate::code_lint::ast::python::{
    DocstringSection, DocstringSectionKind, PythonAttributeDefinition, PythonFunctionSignature,
};
use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::collections::HashSet;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "`{section}:` section states type `{phrase}`, which the code already annotates.",
    rationale: "Type checkers verify the annotation but not the docstring, so the docstring type drifts from it unnoticed and readers cannot tell which one is right.",
    suggestion: "Remove the type from the docstring and keep only what the annotation does not say, such as units, valid ranges or what `None` means.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("annotated-type-in-docstring"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::DOCSTRINGS],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags docstring entries that state a type the code already annotates.",
            what_it_does: indoc::indoc! {r"
                Flags a Google-style `Args:` or `Attributes:` entry written `name (type): …` when
                the parameter or attribute it names has a type annotation, and a `Returns:` or
                `Yields:` section whose first line starts with the annotated type itself, such as
                `bool: True if …` under `-> bool` (for `Yields:`, the item type of an iterator or
                generator annotation also counts). Class docstrings are checked too: `Args:`
                entries against the parameters of `__init__` (or, without an `__init__`, against
                annotated class-body fields), `Attributes:` entries against class-body and
                initializer annotations. Entries for unannotated parameters or attributes are not
                flagged, and neither is a parenthetical that is only `optional` or a `default …`
                note; `(int, optional)` is flagged."},
            why_is_this_bad: indoc::indoc! {r"
                Only the annotation is checked by type checkers; the docstring copy goes stale
                when the annotation changes, and a reader who sees `count (int)` above
                `count: float` cannot tell which is right. Google style asks for a docstring type
                only where the code has no annotation, and for `Returns:` only for type
                information that the annotation does not provide. Use the entry for what the
                annotation cannot say: units, valid ranges, what `None` means."},
            known_problems: Some(indoc::indoc! {r#"
                A `Returns:` or `Yields:` prefix is flagged only when it is textually the
                annotation, so a stale prefix (`int:` under `-> float`) or a spelled-out one
                (`list of str:` under `-> list[str]`) is missed. Projects whose rendered Sphinx
                docs rely on docstring types lose them unless Sphinx renders annotations
                (`autodoc_typehints = "description"`)."#}),
            references: &[
                Reference {
                    title: "Google Python Style Guide §3.8.3: Functions and Methods",
                    url: "https://google.github.io/styleguide/pyguide.html#383-functions-and-methods",
                },
                Reference {
                    title: "pydoclint DOC111",
                    url: "https://jsh9.github.io/pydoclint/violation_codes.html",
                },
            ],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r#"
                    def fetch(url: str, attempts: int) -> bytes:
                        """Fetches `url`, retrying on failure.

                        Args:
                            url: The address to fetch.
                            attempts (int): Maximum number of requests.
                        """
                "#},
                flagged_span: "attempts (int): Maximum number of requests.",
                fixed: indoc::indoc! {r#"
                    def fetch(url: str, attempts: int) -> bytes:
                        """Fetches `url`, retrying on failure.

                        Args:
                            url: The address to fetch.
                            attempts: Maximum number of requests.
                        """
                "#},
            }],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

const SECTION: &str = "section";
const PHRASE: &str = "phrase";

/// A flagged docstring line: its index and the type it states, as written.
type Finding = (usize, String);

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for signature in ast::python::extract_function_signatures(file) {
        let Some(docstring) = signature.docstring() else {
            continue;
        };
        let parameters = annotated_parameter_names(&signature);
        let return_type = signature.return_type_node.as_ref();
        let return_types: Vec<String> = return_type
            .map(|node| node.text().into_owned())
            .into_iter()
            .collect();
        let yield_types: Vec<String> = return_type
            .and_then(ast::python::generator_yield_type)
            .map(|node| node.text().into_owned())
            .into_iter()
            .chain(return_types.iter().cloned())
            .collect();
        for section in docstring.sections() {
            let findings = match section.kind {
                DocstringSectionKind::Args => typed_entries(&section, &parameters),
                DocstringSectionKind::Returns => repeated_prefix(&section, &return_types),
                DocstringSectionKind::Yields => repeated_prefix(&section, &yield_types),
                _ => Vec::new(),
            };
            for (line_index, phrase) in findings {
                diagnostics.push(rule.diagnostic_at_node(
                    path,
                    &docstring.line_node(line_index),
                    &[(SECTION, &section.header), (PHRASE, &phrase)],
                ));
            }
        }
    }
    for class in ast::python::extract_classes(file) {
        let Some(docstring) = &class.docstring else {
            continue;
        };
        let attributes = class.attribute_definitions();
        let annotated_attributes: HashSet<String> = attributes
            .iter()
            .filter(|definition| definition.annotation.is_some())
            .map(|definition| definition.name.clone())
            .collect();
        let constructor_parameters = class.constructor().map_or_else(
            || annotated_field_names(&attributes),
            |constructor| annotated_parameter_names(&constructor),
        );
        for section in docstring.sections() {
            let findings = match section.kind {
                DocstringSectionKind::Args => typed_entries(&section, &constructor_parameters),
                DocstringSectionKind::Attributes => typed_entries(&section, &annotated_attributes),
                _ => Vec::new(),
            };
            for (line_index, phrase) in findings {
                diagnostics.push(rule.diagnostic_at_node(
                    path,
                    &docstring.line_node(line_index),
                    &[(SECTION, &section.header), (PHRASE, &phrase)],
                ));
            }
        }
    }
    diagnostics
}

/// The names of the parameters of `signature` that have a type annotation.
fn annotated_parameter_names(signature: &PythonFunctionSignature<'_>) -> HashSet<String> {
    signature
        .parameters
        .iter()
        .filter(|parameter| parameter.type_node.is_some())
        .map(|parameter| parameter.name.clone())
        .collect()
}

/// The annotated class-body fields among `attributes`: the parameters of the `__init__` that a
/// dataclass-like class generates.
fn annotated_field_names(attributes: &[PythonAttributeDefinition<'_>]) -> HashSet<String> {
    attributes
        .iter()
        .filter(|definition| definition.is_in_class_body && definition.annotation.is_some())
        .map(|definition| definition.name.clone())
        .collect()
}

/// The entries of `section` that state a type for a name in `annotated` (matched without a
/// `*` / `**` prefix).
fn typed_entries(section: &DocstringSection, annotated: &HashSet<String>) -> Vec<Finding> {
    section
        .entries
        .iter()
        .filter(|entry| annotated.contains(entry.bare_name()))
        .filter_map(|entry| {
            let type_text = entry.declared_type()?;
            Some((entry.line_index, format!("({type_text})")))
        })
        .collect()
}

/// The first line of `section` if its `type:` prefix equals one of `annotations`, ignoring
/// whitespace.
fn repeated_prefix(section: &DocstringSection, annotations: &[String]) -> Vec<Finding> {
    let Some(line) = section.body.first() else {
        return Vec::new();
    };
    let Some(prefix) = line.split_type_prefix().map(|typed| typed.type_text) else {
        return Vec::new();
    };
    let prefix_text = without_whitespace(prefix);
    let repeats_annotation = annotations
        .iter()
        .any(|annotation| without_whitespace(annotation) == prefix_text);
    if repeats_annotation {
        vec![(line.index, prefix.to_owned())]
    } else {
        Vec::new()
    }
}

/// `text` with all whitespace removed.
fn without_whitespace(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                untyped_entries_under_annotations => r#"
                    class Fetcher:
                        """Fetches pages.

                        Args:
                            url: Page to fetch.

                        Attributes:
                            url: Page being fetched.
                        """

                        def __init__(self, url: str) -> None:
                            self.url: str = url

                        def fetch(self, attempts: int) -> bool:
                            """Fetches the page.

                            Args:
                                attempts: Maximum number of requests.

                            Returns:
                                True if the page was fetched.
                            """
                "#,
                unannotated_parameter_exempt => r#"
                    def scale(factor):
                        """Scales the drawing.

                        Args:
                            factor (float): Scale factor.
                        """
                "#,
                entry_naming_no_parameter_exempt => r#"
                    def connect(**options: object) -> None:
                        """Connects to the server.

                        Keyword Args:
                            timeout (float): Seconds before giving up.
                        """
                "#,
                parenthetical_without_type_exempt => r#"
                    def scale(factor: float = 1.0, offset: float = 0.0) -> None:
                        """Scales the drawing.

                        Args:
                            factor (optional): Scale factor.
                            offset (default: 0): Shift applied after scaling.
                        """
                "#,
                unannotated_attribute_exempt => r#"
                    class Client:
                        """Network client.

                        Attributes:
                            retries (int): Attempts per request.
                        """

                        retries = 3
                "#,
                differing_returns_prefix_exempt => r#"
                    def count_names(rows: list[str]) -> Mapping[str, int]:
                        """Counts names.

                        Returns:
                            dict: Counts by name.
                        """
                "#,
                returns_without_annotation_exempt => r#"
                    def count(rows):
                        """Counts rows.

                        Returns:
                            int: The row count.
                        """
                "#,
                unannotated_constructor_parameter_exempt => r#"
                    class Counter:
                        """Counts events.

                        Args:
                            count (int): Initial count.
                        """

                        def __init__(self, count):
                            self.count: int = count
                "#,
                parenthetical_in_continuation_line => r#"
                    def scale(factor: float) -> None:
                        """Scales the drawing.

                        Args:
                            factor: Scale factor, applied to both axes
                              (float) before rounding.
                        """
                "#,
            ],
            fail: [
                function_args_entry => r#"
                    def fetch(url: str, attempts: int) -> bytes:
                        """Fetches `url`, retrying on failure.

                        Args:
                            url: The address to fetch.
                            attempts (int): Maximum number of requests.
                        """
                "# => "attempts (int): Maximum number of requests.",
                variadic_entry_without_stars => r#"
                    def log(*values: str) -> None:
                        """Logs values.

                        Args:
                            values (str): Values to log.
                        """
                "# => "values (str): Values to log.",
                starred_variadic_entry => r#"
                    def configure(**options: str) -> None:
                        """Configures the client.

                        Args:
                            **options (str): Option values by name.
                        """
                "# => "**options (str): Option values by name.",
                nested_parenthesis_type => r#"
                    def register(callback: Callable[[int], str]) -> None:
                        """Registers a callback.

                        Args:
                            callback (Callable[[int], str]): Called with the event code.
                        """
                "# => "callback (Callable[[int], str]): Called with the event code.",
                type_with_optional_marker => r#"
                    def scale(factor: float = 1.0) -> None:
                        """Scales the drawing.

                        Args:
                            factor (float, optional): Scale factor.
                        """
                "# => "factor (float, optional): Scale factor.",
                type_name_starting_with_default => r#"
                    def merge(counts: defaultdict[str, int]) -> None:
                        """Merges counts.

                        Args:
                            counts (defaultdict[str, int]): Counts by name.
                        """
                "# => "counts (defaultdict[str, int]): Counts by name.",
                disagreeing_parenthetical => r#"
                    def wait(seconds: int) -> None:
                        """Waits.

                        Args:
                            seconds (str): Time to wait.
                        """
                "# => "seconds (str): Time to wait.",
                init_docstring_args => r#"
                    class Client:
                        def __init__(self, host: str) -> None:
                            """Initializes the client.

                            Args:
                                host (str): Server name.
                            """
                            self.host = host
                "# => "host (str): Server name.",
                class_docstring_args_against_init => r#"
                    class Client:
                        """Network client.

                        Args:
                            host (str): Server name.
                        """

                        def __init__(self, host: str) -> None:
                            self.host = host
                "# => "host (str): Server name.",
                dataclass_args_without_init => r#"
                    @dataclass
                    class Point:
                        """A 2-D point.

                        Args:
                            x (float): Horizontal coordinate.
                        """

                        x: float
                "# => "x (float): Horizontal coordinate.",
                attributes_against_class_body => r#"
                    @dataclass
                    class Point:
                        """A 2-D point.

                        Attributes:
                            x (float): Horizontal coordinate.
                        """

                        x: float
                "# => "x (float): Horizontal coordinate.",
                attributes_against_self_annotation => r#"
                    class Tally:
                        """Running tally.

                        Attributes:
                            total (int): Sum so far.
                        """

                        def __init__(self) -> None:
                            self.total: int = 0
                "# => "total (int): Sum so far.",
                returns_prefix_equal_to_annotation => r#"
                    def count_by_name(rows: list[str]) -> dict[str, int]:
                        """Counts rows by name.

                        Returns:
                            dict[str,int]: Counters by name.
                        """
                "# => "dict[str,int]: Counters by name.",
                yields_prefix_equal_to_item_type => r#"
                    def rows(table: str) -> Iterator[str]:
                        """Streams rows.

                        Yields:
                            str: Each row.
                        """
                "# => "str: Each row.",
            ],
        },
    }
);
