//! Flags type annotations matching project-configured composite type patterns.
//!
//! Design: `docs/dev/banned_type_annotations/03_design.md`.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::code_lint::semantic::type_annotations::find_banned_type_annotations;
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::collections::HashSet;
use std::path::Path;

const BANNED: ListOption = ListOption {
    kind: ListKind::Deny,
    doc: "Type annotation patterns flagged when used, optionally followed by `=> <replacement>`.",
    default: FilterListDefaults {
        base: &[],
        extend: &[],
        remove: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Type annotation `{expression}` uses `{token}`.",
    rationale: "An ad-hoc composite type annotation couples signatures and fields to a raw representation instead of the domain type or protocol meant for that contract.",
    suggestion: "Replace `{token}` in `{expression}` with {replacement}.",
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("adhoc-type-annotation"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(BANNED),
        classification: Classification {
            topics: &[Topic::STATIC_TYPING],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags type annotations matching project-configured composite type patterns.",
            what_it_does: indoc::indoc! {r#"
                Flags parameter, return, and class/struct/enum field type annotations (and their
                nested type arguments) that match any configured pattern in `banned` or
                `extend-banned`.

                Each entry has the form `"<pattern>"` or `"<pattern> => <replacement>"`, such as
                `"tuple[Width, Height] => Dimensions"`,
                `"Generator[_, None, None] => collections.abc.Iterator"`, or
                `"(Width, Height) => Dimensions"`. Patterns are matched structurally: `_` matches
                any single type argument, `...` matches a literal ellipsis in variadic
                `tuple[T, ...]` or `Callable[..., R]` annotations, unions (`A | B`, `Optional[A]`,
                `Union[A, B]`) match regardless of branch order, and type constructors are resolved
                through the file's imports."#},
            why_is_this_bad: indoc::indoc! {r"
                Recurring composite annotations such as `tuple[Width, Height]` or
                `Generator[T, None, None]` encode a domain concept or protocol by repeating its
                raw structural pieces across signatures. Naming the domain record (`Dimensions`) or
                narrower protocol (`Iterator[T]`) in one place makes signatures self-documenting
                and lets the representation evolve without updating every annotation."},
            known_problems: Some(indoc::indoc! {r"
                Type aliases are not expanded before matching, so an alias hiding a banned shape is
                only checked at its own definition if it appears in an inspected annotation."}),
            references: &[Reference::NAME_RESOLUTION],
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r"
                        def resize(canvas: Canvas, target: tuple[Width, Height]) -> None:
                            canvas.apply(target)
                    "},
                    flagged_span: "tuple[Width, Height]",
                    fixed: indoc::indoc! {r"
                        def resize(canvas: Canvas, target: Dimensions) -> None:
                            canvas.apply(target)
                    "},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r"
                        fn resize(canvas: &mut Canvas, target: (Width, Height)) {
                            canvas.apply(target);
                        }
                    "},
                    flagged_span: "(Width, Height)",
                    fixed: indoc::indoc! {r"
                        fn resize(canvas: &mut Canvas, target: Dimensions) {
                            canvas.apply(target);
                        }
                    "},
                },
            ],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

const EXPRESSION: &str = "expression";
const TOKEN: &str = "token";
const REPLACEMENT: &str = "replacement";

fn check_file(
    rule: &CodeRule<ListOption>,
    path: &Path,
    file: &ParsedFile,
    banned: &HashSet<String>,
) -> Vec<Diagnostic> {
    find_banned_type_annotations(file, banned)
        .into_iter()
        .map(|matched| {
            rule.diagnostic_at_node(
                path,
                &matched.node,
                &[
                    (EXPRESSION, &matched.expression),
                    (TOKEN, &matched.token),
                    (REPLACEMENT, &matched.replacement),
                ],
            )
        })
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    banned: &[
        "tuple[Width, Height] => Dimensions",
        "tuple[Bound | None, Bound | None] => ValueRange",
        "tuple[Segment, ...] => RoutePath",
        "dict[str, object] => TypedDict",
        "Callable[..., httpx.Response] => ResponseFactory",
        "Generator[_, None, None] => collections.abc.Iterator",
        "(Width, Height) => Dimensions",
        "(Option<Bound>, Option<Bound>) => ValueRange",
        "Result<_, String> => AppError",
    ],
    {
        Python => {
            pass: [
                domain_types_used => r"
                    from collections.abc import Iterator

                    def resize(canvas: Canvas, target: Dimensions) -> Iterator[int]:
                        return canvas.apply(target)
                ",
                different_tuple_element_types => r"
                    def scale(factors: tuple[float, float]) -> None:
                        pass
                ",
                fixed_length_segment_tuple_exempt => r"
                    def pair_segments(pair: tuple[Segment, Segment]) -> None:
                        pass
                ",
                unbanned_generator_send_type => r"
                    from collections.abc import Generator

                    def stream() -> Generator[int, str, bool]:
                        raise NotImplementedError
                ",
                override_method_exempt => r"
                    from typing import override

                    class SubCanvas(Canvas):
                        @override
                        def resize(self, target: tuple[Width, Height]) -> None:
                            pass
                ",
                protocol_class_exempt => r"
                    from typing import Protocol

                    class Resizable(Protocol):
                        size: tuple[Width, Height]
                ",
                local_variable_annotation_exempt => r"
                    def compute() -> None:
                        target: tuple[Width, Height] = (Width(1), Height(2))
                ",
                annotated_metadata_ignored => r"
                    from typing import Annotated

                    def resize(width: Annotated[int, tuple[Width, Height]]) -> None:
                        pass
                ",
                local_class_shadowing_stdlib_generator_exempt => r"
                    class Generator:
                        pass

                    def run(stream: Generator[int, None, None]) -> None:
                        pass
                ",
                unrelated_response_module_exempt => r"
                    from collections.abc import Callable
                    import urllib.response

                    class Client:
                        factory: Callable[..., urllib.response.Response]
                ",
            ],
            fail: [
                dimensions_tuple_parameter => r"
                    def resize(canvas: Canvas, target: tuple[Width, Height]) -> None:
                        canvas.apply(target)
                " => "tuple[Width, Height]",
                typing_tuple_dimensions_return => r"
                    from typing import Tuple

                    def measure() -> Tuple[Width, Height]:
                        return Width(1), Height(2)
                " => "Tuple[Width, Height]",
                value_range_reversed_union_order_attribute => r"
                    class Axis:
                        bounds: tuple[None | Bound, Bound | None]
                " => "tuple[None | Bound, Bound | None]",
                value_range_typing_optional_return => r"
                    from typing import Optional, Tuple

                    def axis_bounds() -> Tuple[Optional[Bound], Optional[Bound]]:
                        return None, None
                " => "Tuple[Optional[Bound], Optional[Bound]]",
                variadic_segment_tuple_return => r"
                    def split_route() -> tuple[Segment, ...]:
                        raise NotImplementedError
                " => "tuple[Segment, ...]",
                untyped_string_dict_parameter => r"
                    def configure(settings: dict[str, object]) -> None:
                        pass
                " => "dict[str, object]",
                callable_ellipsis_qualified_return_attribute => r"
                    from collections.abc import Callable
                    from httpx import Response

                    class Client:
                        factory: Callable[..., Response]
                " => "Callable[..., Response]",
                generator_yield_only_wildcard_return => r"
                    from collections.abc import Generator

                    def iter_chunks() -> Generator[bytes, None, None]:
                        raise NotImplementedError
                " => "Generator[bytes, None, None]",
                qualified_collections_abc_generator_parameter => r"
                    import collections.abc

                    def consume(stream: collections.abc.Generator[int, None, None]) -> None:
                        pass
                " => "collections.abc.Generator[int, None, None]",
                nested_inside_list_parameter => r"
                    def batch_resize(targets: list[tuple[Width, Height]]) -> None:
                        pass
                " => "tuple[Width, Height]",
                instance_attribute_in_method => r"
                    class Window:
                        def __init__(self, width: Width, height: Height) -> None:
                            self.size: tuple[Width, Height] = (width, height)
                " => "tuple[Width, Height]",
                annotated_wrapped_tuple_parameter => r#"
                    from typing import Annotated

                    def resize(target: Annotated[tuple[Width, Height], "px"]) -> None:
                        pass
                "# => "tuple[Width, Height]",
            ],
        },
        Rust => {
            pass: [
                domain_structs_used => r"
                    fn resize(canvas: &mut Canvas, target: Dimensions) {
                        canvas.apply(target);
                    }
                ",
                different_tuple_types => r"
                    fn scale(factors: (f64, f64)) -> f64 {
                        factors.0 * factors.1
                    }
                ",
                typed_error_result_exempt => r"
                    fn load() -> Result<Config, AppError> {
                        todo!()
                    }
                ",
                trait_impl_exempt => r"
                    impl Resizable for Canvas {
                        fn resize(&mut self, target: (Width, Height)) {}
                    }
                ",
                inline_test_exempt => r"
                    #[cfg(test)]
                    mod inline_tests {
                        fn helper(target: (Width, Height)) {}
                    }
                ",
                local_let_binding_exempt => r"
                    fn compute(w: Width, h: Height) {
                        let target: (Width, Height) = (w, h);
                    }
                ",
            ],
            fail: [
                dimensions_tuple_parameter => r"
                    fn resize(canvas: &mut Canvas, target: (Width, Height)) {
                        canvas.apply(target);
                    }
                " => "(Width, Height)",
                dimensions_tuple_return => r"
                    fn measure() -> (Width, Height) {
                        (Width(1), Height(2))
                    }
                " => "(Width, Height)",
                dimensions_tuple_struct_field => r"
                    struct Window {
                        size: (Width, Height),
                    }
                " => "(Width, Height)",
                value_range_option_tuple_return => r"
                    fn axis_bounds() -> (Option<Bound>, Option<Bound>) {
                        (None, None)
                    }
                " => "(Option<Bound>, Option<Bound>)",
                string_error_result_return => r"
                    fn load() -> Result<Config, String> {
                        todo!()
                    }
                " => "Result<Config, String>",
                nested_vec_of_dimensions_parameter => r"
                    fn batch_resize(targets: Vec<(Width, Height)>) {
                        let _ = targets;
                    }
                " => "(Width, Height)",
                enum_variant_dimensions_field => r"
                    enum Event {
                        Resized { size: (Width, Height) },
                    }
                " => "(Width, Height)",
            ],
        },
    }
);
