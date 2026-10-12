//! Flags public attributes missing from the `Attributes:` section of a detailed class docstring.
//!
//! Design: `docs/dev/class_docstring/03_design.md` and `03b_attribute_options.md` (option A:
//! constructor arguments stored on `self` are attributes too). Only the class docstring counts;
//! an `Attributes:` section in the `__init__` docstring does not.

use crate::code_lint::ast::python::DocstringSectionKind;
use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::collections::HashSet;
use std::path::Path;

const ALLOWED: ListOption = ListOption {
    kind: ListKind::Allow,
    doc: "Attribute names never required under `Attributes:`, such as framework configuration.",
    default: FilterListDefaults {
        base: &["model_config"],
        extend: &[],
        remove: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Public attribute `{name}` of `{class}` is not listed under `Attributes:` in the class docstring.",
    rationale: "Readers learn an instance's state from the class docstring, so an unlisted attribute is only discoverable by reading every assignment in the class.",
    suggestion: "Add a `{name}:` entry with its description under `Attributes:` in the class docstring of `{class}`, not in the docstring of `__init__`.",
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("undocumented-public-attribute"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(ALLOWED),
        classification: Classification {
            topics: &[Topic::DOCSTRINGS],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Requires detailed Python class docstrings to list every public attribute under `Attributes:`.",
            what_it_does: indoc::indoc! {r"
                When a class docstring has more than one non-blank line, every public attribute
                needs an entry under `Attributes:` in that docstring. Attributes are the names
                assigned in the class body (dataclass, NamedTuple, attrs and Pydantic fields,
                constants, `ClassVar`s) and the `self.<name>` assignments in `__init__`,
                `__post_init__`, `__attrs_post_init__` and `model_post_init`, including
                constructor arguments stored on `self`. An `Attributes:` section in the
                `__init__` docstring does not count. Not attributes: `_`-prefixed names, names
                the class defines with `def` (properties, methods), members of an Enum class,
                `dataclasses.InitVar` pseudo-fields, and the names in `allowed`. Each missing
                name is reported at its first assignment. Constructor parameters themselves
                are not checked here."},
            why_is_this_bad: indoc::indoc! {r"
                The class docstring is what class hover and generated documentation show. An
                attribute missing from it is part of the instance's contract that readers can
                only find by reading every assignment in the class. Google style lists public
                attributes, excluding properties, under `Attributes:` in the class docstring,
                including those initialized from constructor arguments."},
            known_problems: Some(indoc::indoc! {r"
                Re-assigning an inherited attribute in a subclass initializer is flagged even
                when the base class documents it. Subclasses of a project Enum are not
                recognized as enums. Attributes first created in other methods are not seen.
                NumPy and Sphinx docstrings are flagged, since only Google-style sections are
                read. A one-line class docstring is not checked, so collapsing the docstring
                silences the rule."}),
            references: &[Reference {
                title: "Google Python Style Guide §3.8.4: Classes",
                url: "https://google.github.io/styleguide/pyguide.html#384-classes",
            }],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r#"
                    class SampleClass:
                        """Summary of class here.

                        Attributes:
                            likes_spam: A boolean indicating if we like SPAM or not.
                        """

                        def __init__(self, likes_spam: bool = False) -> None:
                            self.likes_spam = likes_spam
                            self.eggs = 0
                "#},
                flagged_span: "self.eggs",
                fixed: indoc::indoc! {r#"
                    class SampleClass:
                        """Summary of class here.

                        Attributes:
                            likes_spam: A boolean indicating if we like SPAM or not.
                            eggs: An integer count of the eggs we have laid.
                        """

                        def __init__(self, likes_spam: bool = False) -> None:
                            self.likes_spam = likes_spam
                            self.eggs = 0
                "#},
            }],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

const NAME: &str = "name";
const CLASS: &str = "class";

fn check_file(
    rule: &CodeRule<ListOption>,
    path: &Path,
    file: &ParsedFile,
    allowed: &HashSet<String>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for class in ast::python::extract_classes(file) {
        let Some(docstring) = &class.docstring else {
            continue;
        };
        if docstring.non_blank_line_count() <= 1 || class.is_enum() {
            continue;
        }
        let documented = docstring.section_entry_names(DocstringSectionKind::Attributes);
        for definition in class.attribute_definitions() {
            let name = definition.name.as_str();
            let is_exempt = name.starts_with('_')
                || class.defines_function(name)
                || definition.is_init_var
                || allowed.contains(name);
            if !is_exempt && !documented.contains(name) {
                diagnostics.push(rule.diagnostic_at_node(
                    path,
                    &definition.node,
                    &[(NAME, name), (CLASS, &class.name)],
                ));
            }
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
                every_attribute_listed => r#"
                    class Fetcher:
                        """Fetches pages with retries.

                        Attributes:
                            timeout: Seconds before a fetch is abandoned.
                            url: Page being fetched.
                            retries: Attempts per fetch, at least 1.
                            attempts: Attempts made so far.
                        """

                        timeout: float = 3.0

                        def __init__(self, url: str, retries: int) -> None:
                            """Initializes the fetcher.

                            Args:
                                url: Page to fetch.
                                retries: Attempts per fetch.
                            """
                            self.url = url
                            self.retries = max(retries, 1)
                            self.attempts = 0
                "#,
                summary_only_docstring_exempt => r#"
                    @dataclass
                    class Point:
                        """A 2-D point."""

                        x: float
                "#,
                underscore_prefixed_names_exempt => r#"
                    class Cache:
                        """In-memory cache.

                        Entries expire after one hour.
                        """

                        __slots__ = ("_entries",)

                        def __init__(self) -> None:
                            self._entries = {}
                "#,
                names_defined_by_def_exempt => r#"
                    class Square:
                        """A square.

                        Attributes:
                            side: Side length.
                        """

                        def __init__(self, side: float) -> None:
                            self.side = side
                            self.area = side * side

                        @property
                        def area(self) -> float:
                            return self.side * self.side

                        @area.setter
                        def area(self, value: float) -> None:
                            self.side = value**0.5
                "#,
                enum_members_exempt => r#"
                    class Color(Enum):
                        """Display colors.

                        Values follow the terminal palette.
                        """

                        RED = 1
                "#,
                allowed_names_exempt => r#"
                    class User(BaseModel):
                        """A registered user.

                        Attributes:
                            name: Display name.
                        """

                        model_config = ConfigDict(frozen=True)
                        name: str
                "#,
                nested_scope_assignments_exempt => r#"
                    class Linker:
                        """Links nodes.

                        Attributes:
                            peers: Linked nodes.
                        """

                        def __init__(self) -> None:
                            self.peers = []

                            def bind():
                                self.leak = 1

                            self.peers.append(bind)
                "#,
                other_method_assignments_exempt => r#"
                    class Client:
                        """Network client.

                        Attributes:
                            host: Server name.
                        """

                        def __init__(self, host: str) -> None:
                            self.host = host

                        def connect(self) -> None:
                            self.socket = open_socket(self.host)
                "#,
                init_var_pseudo_field_exempt => r#"
                    @dataclass
                    class Account:
                        """A bank account.

                        Attributes:
                            balance: Current balance.
                        """

                        seed: InitVar[int]
                        balance: int = 0
                "#,
            ],
            fail: [
                undocumented_constructor_attribute => r#"
                    class SampleClass:
                        """Summary of class here.

                        Attributes:
                            likes_spam: A boolean indicating if we like SPAM or not.
                        """

                        def __init__(self, likes_spam: bool = False) -> None:
                            self.likes_spam = likes_spam
                            self.eggs = 0
                "# => "self.eggs",
                stored_argument_documented_only_under_args => r#"
                    class Fetcher:
                        """Fetches pages.

                        Args:
                            url: Page to fetch.

                        Attributes:
                            attempts: Attempts made so far.
                        """

                        def __init__(self, url: str) -> None:
                            self.url = url
                            self.attempts = 0
                "# => "self.url",
                attributes_section_in_init_docstring_not_counted => r#"
                    class Fetcher:
                        """Fetches pages.

                        Retries failed requests.
                        """

                        def __init__(self, url: str) -> None:
                            """Initializes the fetcher.

                            Attributes:
                                url: Page being fetched.
                            """
                            self.url = url
                "# => "self.url",
                dataclass_field => r#"
                    @dataclass
                    class Point:
                        """A 2-D point.

                        Attributes:
                            x: Horizontal coordinate.
                        """

                        x: float
                        y: float
                "# => "y",
                class_body_constant => r#"
                    class Limits:
                        """Request limits.

                        Applied per client.
                        """

                        MAX_RETRIES = 3
                "# => "MAX_RETRIES",
                post_init_attribute => r#"
                    @dataclass
                    class Box:
                        """A box.

                        Attributes:
                            width: Width in meters.
                        """

                        width: float

                        def __post_init__(self) -> None:
                            self.volume = self.width**3
                "# => "self.volume",
                tuple_unpacking_target => r#"
                    class Range:
                        """A numeric range.

                        Attributes:
                            high: Upper bound.
                        """

                        def __init__(self, bounds: tuple[int, int]) -> None:
                            self.low, self.high = bounds
                "# => "self.low",
                nested_class_checked => r#"
                    def build():
                        class Node:
                            """A tree node.

                            Holds one value.
                            """

                            value: int
                "# => "value",
            ],
        },
    }
);
