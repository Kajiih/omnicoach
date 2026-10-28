//! Flags class attributes and struct or enum fields typed as a positional tuple instead of a named record.
//!
//! Design: `docs/dev/banned_type_annotations/03_design.md`.

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, CountOption, Declaration, Example, ImpactedQuality,
    LanguageDefaults, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const MIN_ELEMENTS: CountOption = CountOption {
    key: "min-elements",
    doc: "Minimum elements in an attribute or field tuple type for it to be flagged.",
    default: LanguageDefaults::new(2, &[]),
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: {
        Python => "Attribute `{name}` of `{class}` is typed as a positional tuple of {count} elements.",
        Rust => "Field `{name}` of `{class}` is typed as a positional tuple of {count} elements.",
    },
    rationale: "A tuple names none of its elements, so every read and write depends on element order and swapping two elements of the same type raises no error.",
    suggestion: {
        Python => "Replace the tuple annotation on `{name}` in `{class}` with a `NamedTuple` or a frozen dataclass whose fields name each element.",
        Rust => "Replace the tuple type on `{name}` in `{class}` with a struct whose fields name each element.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule<CountOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("positional-tuple-attribute"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(MIN_ELEMENTS),
        classification: Classification {
            topics: &[Topic::POSITIONAL_MEANING],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags class attributes and struct or enum fields typed as a positional tuple instead of a named record.",
            what_it_does: indoc::indoc! {r"
                Flags a class-body or instance attribute in Python (`size: tuple[int, int]`,
                `self.size: tuple[int, int]`) or a named `struct` or `enum` variant field in Rust
                (`size: (u32, u32)`) whose type is a fixed-length tuple of at least `min-elements`
                elements. The tuple is also found inside an optional (`| None`, `Optional`,
                `Option`), a union, a Python attribute qualifier (`Annotated`, `ClassVar`, `Final`,
                `InitVar`, `Required`, `NotRequired`, `ReadOnly`), and a Rust borrow or
                smart-pointer wrapper (`&(A, B)`, `Box<(A, B)>`, `Rc<(A, B)>`, `Arc<(A, B)>`,
                `Cow<'_, (A, B)>`).

                A homogeneous `tuple[int, ...]`, a tuple inside a collection (`list[tuple[int, str]]`,
                `Vec<(u32, u32)>`), unnamed tuple-struct fields (`struct Pair(u32, u32)`), and
                attributes of Python `Protocol` and `ABC` classes are not flagged."},
            why_is_this_bad: indoc::indoc! {r"
                Storing a multi-element tuple on a class or struct pushes positional indexing
                (`window.size[0]`, `window.size.1`) into every method and caller that reads or
                updates the field. Swapping two elements of the same type compiles and type-checks,
                and adding a field to the tuple breaks every construction and unpacking site.

                A `NamedTuple` or frozen dataclass (Python) or a struct (Rust) gives each element a
                field name on the attribute itself."},
            known_problems: Some(indoc::indoc! {r"
                - Type aliases (`Pair = tuple[int, int]`, `type Pair = (u32, u32);`) are not
                  expanded.
                - A symmetric pair whose order is its meaning, such as `(min, max)` bounds, is
                  flagged too."}),
            references: &[
                Reference {
                    title: "Python documentation: `dataclasses`",
                    url: "https://docs.python.org/3/library/dataclasses.html",
                },
                Reference {
                    title: "The Rust Programming Language: Defining and Instantiating Structs",
                    url: "https://doc.rust-lang.org/book/ch05-01-defining-structs.html",
                },
                Reference::NAME_RESOLUTION,
            ],
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r"
                        from dataclasses import dataclass

                        @dataclass(frozen=True)
                        class Window:
                            size: tuple[int, int]
                    "},
                    flagged_span: "tuple[int, int]",
                    fixed: indoc::indoc! {r"
                        from dataclasses import dataclass

                        @dataclass(frozen=True)
                        class Dimensions:
                            width: int
                            height: int

                        @dataclass(frozen=True)
                        class Window:
                            size: Dimensions
                    "},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r"
                        struct Window {
                            size: (u32, u32),
                        }
                    "},
                    flagged_span: "(u32, u32)",
                    fixed: indoc::indoc! {r"
                        struct Dimensions {
                            width: u32,
                            height: u32,
                        }

                        struct Window {
                            size: Dimensions,
                        }
                    "},
                },
            ],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

const NAME: &str = "name";
const CLASS: &str = "class";
const COUNT: &str = "count";

fn check_file(
    rule: &CodeRule<CountOption>,
    path: &Path,
    file: &ParsedFile,
    min_elements: usize,
) -> Vec<Diagnostic> {
    ast::collect_attribute_annotations(file)
        .into_iter()
        .filter_map(|attribute| match attribute.positional_tuple_arity {
            Some(arity) if arity >= min_elements => Some(rule.diagnostic_at_node(
                path,
                &attribute.type_node,
                &[
                    (NAME, &attribute.name),
                    (CLASS, &attribute.class),
                    (COUNT, &arity.to_string()),
                ],
            )),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                named_record_attributes => r"
                    from dataclasses import dataclass

                    @dataclass(frozen=True)
                    class Dimensions:
                        width: int
                        height: int

                    @dataclass(frozen=True)
                    class Window:
                        size: Dimensions
                ",
                variadic_tuple_attribute_exempt => r"
                    class History:
                        entries: tuple[str, ...]
                ",
                single_element_tuple_attribute_exempt => r"
                    class Wrapper:
                        value: tuple[int]
                ",
                empty_tuple_attribute_exempt => r"
                    class Empty:
                        unit: tuple[()]
                ",
                tuple_inside_collection_attribute_exempt => r"
                    class Table:
                        rows: list[tuple[int, str]]
                ",
                annotated_metadata_tuple_exempt => r"
                    from typing import Annotated

                    class Config:
                        port: Annotated[int, tuple[int, int]]
                ",
                protocol_class_attribute_exempt => r"
                    from typing import Protocol

                    class HasBounds(Protocol):
                        bounds: tuple[int, int]
                ",
                abc_class_attribute_exempt => r"
                    from abc import ABC

                    class AbstractShape(ABC):
                        bounds: tuple[int, int]
                ",
                module_variable_annotation_exempt => r"
                    DEFAULT_BOUNDS: tuple[int, int] = (0, 100)
                ",
                function_local_variable_annotation_exempt => r"
                    def compute() -> int:
                        pair: tuple[int, int] = (1, 2)
                        return pair[0]
                ",
                local_class_shadows_typing_tuple => r"
                    class Tuple:
                        pass

                    class Holder:
                        pair: Tuple[int, int]
                ",
            ],
            fail: [
                dataclass_tuple_attribute => r"
                    from dataclasses import dataclass

                    @dataclass(frozen=True)
                    class Window:
                        size: tuple[int, int]
                " => "tuple[int, int]",
                typing_tuple_attribute => r"
                    from typing import Tuple

                    class Box3D:
                        dimensions: Tuple[int, int, int]
                " => "Tuple[int, int, int]",
                optional_tuple_attribute => r"
                    class Cursor:
                        position: tuple[int, int] | None = None
                " => "tuple[int, int] | None",
                classvar_tuple_attribute => r"
                    from typing import ClassVar

                    class Grid:
                        default_size: ClassVar[tuple[int, int]] = (10, 10)
                " => "ClassVar[tuple[int, int]]",
                final_tuple_attribute => r"
                    from typing import Final

                    class Origin:
                        coords: Final[tuple[int, int]] = (0, 0)
                " => "Final[tuple[int, int]]",
                annotated_wrapper_tuple_attribute => r"
                    from typing import Annotated

                    class Viewport:
                        bounds: Annotated[tuple[int, int], 'px']
                " => "Annotated[tuple[int, int], 'px']",
                init_var_tuple_attribute => r"
                    from dataclasses import InitVar, dataclass

                    @dataclass
                    class Session:
                        credentials: InitVar[tuple[str, str]]
                " => "InitVar[tuple[str, str]]",
                init_instance_tuple_attribute => r"
                    class Client:
                        def __init__(self, host: str, port: int) -> None:
                            self.endpoint: tuple[str, int] = (host, port)
                " => "tuple[str, int]",
                private_tuple_attribute => r"
                    class Cache:
                        _bounds: tuple[int, int]
                " => "tuple[int, int]",
                typed_dict_tuple_field => r"
                    from typing import NotRequired, TypedDict

                    class EventPayload(TypedDict):
                        coords: NotRequired[tuple[float, float]]
                " => "NotRequired[tuple[float, float]]",
            ],
        },
        Rust => {
            pass: [
                named_struct_fields => r"
                    struct Dimensions {
                        width: u32,
                        height: u32,
                    }

                    struct Window {
                        size: Dimensions,
                    }
                ",
                unit_tuple_field_exempt => r"
                    struct Marker {
                        unit: (),
                    }
                ",
                single_element_tuple_field_exempt => r"
                    struct Wrapper {
                        item: (u32,),
                    }
                ",
                tuple_inside_vec_field_exempt => r"
                    struct History {
                        entries: Vec<(u32, String)>,
                    }
                ",
                tuple_struct_unnamed_fields_exempt => r"
                    struct Pair(u32, u32);
                ",
                inline_test_struct_exempt => r"
                    #[cfg(test)]
                    mod inline_tests {
                        struct Fixture {
                            bounds: (u32, u32),
                        }
                    }
                ",
                local_let_tuple_binding_exempt => r"
                    fn compute() -> u32 {
                        let pair: (u32, u32) = (1, 2);
                        pair.0
                    }
                ",
            ],
            fail: [
                struct_tuple_field => r"
                    struct Window {
                        size: (u32, u32),
                    }
                " => "(u32, u32)",
                option_tuple_field => r"
                    struct Cursor {
                        position: Option<(i32, i32)>,
                    }
                " => "Option<(i32, i32)>",
                borrowed_tuple_field => r"
                    struct View<'a> {
                        bounds: &'a (u32, u32),
                    }
                " => "&'a (u32, u32)",
                boxed_tuple_field => r"
                    struct Node {
                        pair: Box<(u32, u32)>,
                    }
                " => "Box<(u32, u32)>",
                enum_variant_record_tuple_field => r"
                    enum Command {
                        Resize { dimensions: (u32, u32) },
                    }
                " => "(u32, u32)",
            ],
        },
    }
);
