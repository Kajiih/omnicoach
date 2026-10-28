//! Flags function and method parameters typed as a positional tuple instead of a named record.
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
    doc: "Minimum elements in a parameter tuple type for it to be flagged.",
    default: LanguageDefaults::new(2, &[]),
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Parameter `{name}` of `{function}` is typed as a positional tuple of {count} elements.",
    rationale: "A tuple names none of its elements, so callers pass values by position and swapping two elements of the same type raises no error.",
    suggestion: {
        Python => "Replace the tuple annotation on `{name}` in `{function}` with a `NamedTuple` or a frozen dataclass whose fields name each element.",
        Rust => "Replace the tuple type on `{name}` in `{function}` with a struct whose fields name each element.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule<CountOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("positional-tuple-parameter"),
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
            summary: "Flags function and method parameters typed as a positional tuple instead of a named record.",
            what_it_does: indoc::indoc! {r"
                Flags a function or method parameter whose type annotation is a fixed-length tuple
                of at least `min-elements` elements: `tuple[float, float]` in Python or
                `(f64, f64)` in Rust. The tuple is also found inside an optional (`| None`,
                `Optional`, `Option`), a union, `Annotated`, and a Rust borrow or smart-pointer
                wrapper (`&(A, B)`, `&mut (A, B)`, `Box<(A, B)>`, `Rc<(A, B)>`, `Arc<(A, B)>`,
                `Cow<'_, (A, B)>`).

                A homogeneous `tuple[int, ...]` and a tuple inside a collection or callable
                (`list[tuple[int, str]]`, `Vec<(u32, u32)>`, `Callable[[tuple[int, int]], None]`)
                are not flagged. Signatures imposed from outside are skipped: Python `@override`,
                `@overload`, `@abstractmethod`, fixtures, `@register`, property setters, data-model
                dunders other than `__init__`, `__new__` and `__call__`, and `Protocol` / `ABC`
                members, and Rust trait items and trait implementations."},
            why_is_this_bad: indoc::indoc! {r"
                A tuple parameter states what each element is, not what it means:
                `point: tuple[float, float]` and `bounds: (u32, u32)` leave callers to remember
                which position is which. Every call site constructs the tuple by position, so
                swapping two elements of the same type compiles and type-checks, and adding an
                element breaks every caller.

                A `NamedTuple` or frozen dataclass (Python) or a struct (Rust) puts the field names
                in the signature, where callers, readers and type checkers see them."},
            known_problems: Some(indoc::indoc! {r"
                - Type aliases (`Pair = tuple[int, int]`, `type Pair = (u32, u32);`) are not
                  expanded.
                - A symmetric pair whose order is its meaning, such as `(min, max)` bounds, is
                  flagged too."}),
            references: &[
                Reference {
                    title: "Python documentation: `typing.NamedTuple`",
                    url: "https://docs.python.org/3/library/typing.html#typing.NamedTuple",
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
                        def inside_box(point: tuple[float, float], limit: float) -> bool:
                            return point[0] <= limit and point[1] <= limit
                    "},
                    flagged_span: "tuple[float, float]",
                    fixed: indoc::indoc! {r"
                        from typing import NamedTuple

                        class Point(NamedTuple):
                            x: float
                            y: float

                        def inside_box(point: Point, limit: float) -> bool:
                            return point.x <= limit and point.y <= limit
                    "},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r"
                        fn inside_box(point: (f64, f64), limit: f64) -> bool {
                            point.0 <= limit && point.1 <= limit
                        }
                    "},
                    flagged_span: "(f64, f64)",
                    fixed: indoc::indoc! {r"
                        struct Point {
                            x: f64,
                            y: f64,
                        }

                        fn inside_box(point: Point, limit: f64) -> bool {
                            point.x <= limit && point.y <= limit
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
const FUNCTION: &str = "function";
const COUNT: &str = "count";

fn check_file(
    rule: &CodeRule<CountOption>,
    path: &Path,
    file: &ParsedFile,
    min_elements: usize,
) -> Vec<Diagnostic> {
    ast::collect_parameter_annotations(file)
        .into_iter()
        .filter_map(|parameter| match parameter.positional_tuple_arity {
            Some(arity) if arity >= min_elements => Some(rule.diagnostic_at_node(
                path,
                &parameter.type_node,
                &[
                    (NAME, &parameter.name),
                    (FUNCTION, &parameter.function),
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
                named_record_parameter => r"
                    from typing import NamedTuple

                    class Point(NamedTuple):
                        x: float
                        y: float

                    def inside_box(point: Point, limit: float) -> bool:
                        return point.x <= limit and point.y <= limit
                ",
                variadic_tuple_exempt => r"
                    def total(values: tuple[int, ...]) -> int:
                        return sum(values)
                ",
                single_element_tuple_exempt => r"
                    def unwrap_one(item: tuple[int]) -> int:
                        return item[0]
                ",
                empty_tuple_exempt => r"
                    def accept_empty(unit: tuple[()]) -> None:
                        pass
                ",
                tuple_inside_collection_exempt => r"
                    from collections.abc import Sequence

                    def load_pairs(entries: Sequence[tuple[str, int]]) -> int:
                        return len(entries)
                ",
                callable_tuple_argument_exempt => r"
                    from collections.abc import Callable

                    def run_callback(handler: Callable[[tuple[int, int]], None]) -> None:
                        handler((1, 2))
                ",
                annotated_metadata_tuple_exempt => r"
                    from typing import Annotated

                    def check_value(value: Annotated[int, tuple[int, int]]) -> int:
                        return value
                ",
                variadic_args_exempt => r"
                    def collect_all(*args: tuple[int, int]) -> int:
                        return len(args)
                ",
                variadic_kwargs_exempt => r"
                    def collect_named(**kwargs: tuple[int, int]) -> int:
                        return len(kwargs)
                ",
                receiver_parameter_exempt => r"
                    class Pair:
                        def inspect(self: tuple[int, int]) -> int:
                            return self[0]
                ",
                override_method_exempt => r"
                    from typing import override

                    class SubProcessor(BaseProcessor):
                        @override
                        def handle(self, pair: tuple[str, int]) -> None:
                            pass
                ",
                overload_signature_exempt => r"
                    from typing import overload

                    @overload
                    def convert(pair: tuple[str, int]) -> str: ...

                    def convert(pair: object) -> str:
                        return str(pair)
                ",
                abstractmethod_exempt => r"
                    from abc import abstractmethod

                    class Handler:
                        @abstractmethod
                        def handle(self, pair: tuple[str, int]) -> None: ...
                ",
                property_setter_exempt => r"
                    class Box:
                        @property
                        def size(self) -> int:
                            return 0

                        @size.setter
                        def size(self, value: tuple[int, int]) -> None:
                            pass
                ",
                pytest_fixture_exempt => r"
                    import pytest

                    @pytest.fixture
                    def sample_grid(bounds: tuple[int, int]) -> int:
                        return bounds[0]
                ",
                protocol_method_exempt => r"
                    from typing import Protocol

                    class Renderer(Protocol):
                        def draw(self, size: tuple[int, int]) -> None: ...
                ",
                abc_method_exempt => r"
                    from abc import ABC

                    class BaseRenderer(ABC):
                        def draw(self, size: tuple[int, int]) -> None:
                            pass
                ",
                data_model_dunder_exempt => r"
                    class Vector:
                        def __eq__(self, other: tuple[int, int]) -> bool:
                            return False
                ",
                singledispatch_register_exempt => r"
                    @process.register
                    def _handle_pair(pair: tuple[int, int]) -> None:
                        pass
                ",
                local_class_shadows_typing_tuple => r"
                    class Tuple:
                        pass

                    def handle(pair: Tuple[int, int]) -> None:
                        pass
                ",
            ],
            fail: [
                two_element_tuple_parameter => r"
                    def inside_box(point: tuple[float, float], limit: float) -> bool:
                        return point[0] <= limit
                " => "tuple[float, float]",
                three_element_typing_tuple_parameter => r"
                    from typing import Tuple

                    def volume(dimensions: Tuple[int, int, int]) -> int:
                        return dimensions[0]
                " => "Tuple[int, int, int]",
                keyword_only_tuple_parameter => r"
                    def connect(*, endpoint: tuple[str, int]) -> None:
                        pass
                " => "tuple[str, int]",
                positional_only_tuple_parameter => r"
                    def distance(origin: tuple[float, float], /) -> float:
                        return origin[0]
                " => "tuple[float, float]",
                optional_pep604_tuple_parameter => r"
                    def relocate(target: tuple[int, int] | None = None) -> None:
                        pass
                " => "tuple[int, int] | None",
                typing_optional_and_annotated_tuple_parameter => r"
                    from typing import Annotated, Optional, Tuple

                    def resize(bounds: Annotated[Optional[Tuple[int, int]], 'px']) -> None:
                        pass
                " => "Annotated[Optional[Tuple[int, int]], 'px']",
                init_constructor_tuple_parameter => r"
                    class Window:
                        def __init__(self, size: tuple[int, int]) -> None:
                            self.width, self.height = size
                " => "tuple[int, int]",
                new_constructor_tuple_parameter => r"
                    class Window:
                        def __new__(cls, size: tuple[int, int]) -> 'Window':
                            return super().__new__(cls)
                " => "tuple[int, int]",
                call_dunder_tuple_parameter => r"
                    class Transform:
                        def __call__(self, point: tuple[float, float]) -> float:
                            return point[0]
                " => "tuple[float, float]",
                aliased_typing_tuple_parameter => r"
                    import typing as t

                    def locate(coords: t.Tuple[int, int]) -> int:
                        return coords[0]
                " => "t.Tuple[int, int]",
            ],
        },
        Rust => {
            pass: [
                struct_parameter => r"
                    struct Point {
                        x: f64,
                        y: f64,
                    }

                    fn inside_box(point: Point, limit: f64) -> bool {
                        point.x <= limit && point.y <= limit
                    }
                ",
                unit_tuple_exempt => r"
                    fn accept_unit(unit: ()) {}
                ",
                single_element_tuple_exempt => r"
                    fn accept_one(item: (u32,)) -> u32 {
                        item.0
                    }
                ",
                tuple_inside_vec_exempt => r"
                    fn sum_pairs(pairs: Vec<(u32, u32)>) -> usize {
                        pairs.len()
                    }
                ",
                tuple_inside_slice_exempt => r"
                    fn count_pairs(pairs: &[(u32, u32)]) -> usize {
                        pairs.len()
                    }
                ",
                trait_declaration_parameter_exempt => r"
                    trait Canvas {
                        fn resize(&mut self, size: (u32, u32));
                    }
                ",
                trait_impl_parameter_exempt => r"
                    impl Canvas for Screen {
                        fn resize(&mut self, size: (u32, u32)) {}
                    }
                ",
                inline_test_parameter_exempt => r"
                    #[cfg(test)]
                    mod inline_tests {
                        fn helper(pair: (u32, u32)) -> u32 {
                            pair.0 + pair.1
                        }
                    }
                ",
            ],
            fail: [
                two_element_tuple_parameter => r"
                    fn inside_box(point: (f64, f64), limit: f64) -> bool {
                        point.0 <= limit && point.1 <= limit
                    }
                " => "(f64, f64)",
                borrowed_tuple_parameter => r"
                    fn area(size: &(u32, u32)) -> u32 {
                        size.0 * size.1
                    }
                " => "&(u32, u32)",
                mutable_borrowed_tuple_parameter => r"
                    fn swap_in_place(pair: &mut (u32, u32)) {
                        std::mem::swap(&mut pair.0, &mut pair.1);
                    }
                " => "&mut (u32, u32)",
                option_tuple_parameter => r"
                    fn relocate(target: Option<(i32, i32)>) -> i32 {
                        target.map_or(0, |pair| pair.0)
                    }
                " => "Option<(i32, i32)>",
                boxed_tuple_parameter => r"
                    fn consume_boxed(pair: Box<(u32, u32)>) -> u32 {
                        pair.0
                    }
                " => "Box<(u32, u32)>",
                destructured_tuple_parameter => r"
                    fn add_pair((left, right): (u32, u32)) -> u32 {
                        left + right
                    }
                " => "(u32, u32)",
                inherent_method_tuple_parameter => r"
                    impl Window {
                        fn resize(&mut self, size: (u32, u32)) {}
                    }
                " => "(u32, u32)",
            ],
        },
    }
);
