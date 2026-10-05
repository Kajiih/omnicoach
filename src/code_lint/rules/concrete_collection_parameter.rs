//! Flags Python function parameters annotated with concrete mutable collection types.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    AnnotationTraversalDepth, CollectionKind, PythonCollectionType, PythonParameterKind,
    collect_collection_types, extract_function_signatures,
    has_unaliased_collections_abc_set_import,
};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::code_lint::policy::read_only_collection_replacements;
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Parameter `{name}` of `{function}` has annotation `{expression}`, which contains concrete collection type `{token}`.",
    rationale: "Concrete mutable collection types such as `list`, `dict`, and `set` are invariant in their type arguments and reject read-only inputs such as `tuple`, `frozenset`, or subtype sequences.",
    suggestion: "Replace `{token}` in `{name}` with the most general `collections.abc` type that supports every operation `{function}` performs on `{name}`, such as `{replacement}` for read-only access or its `Mutable` counterpart for in-place mutation.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("concrete-collection-parameter"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::STATIC_TYPING],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags Python function parameters annotated with concrete mutable collection types such as `list`, `dict`, or `set`.",
            what_it_does: indoc::indoc! {r"
                Flags non-variadic parameters of functions and methods in Python source files (test
                files are not checked) whose type annotation uses a concrete mutable collection
                constructor (`list`, `dict`, `set`, `List`, `Dict`, `Set`, `typing.List`,
                `typing.Dict`, or `typing.Set`, and the `collections` containers `defaultdict`,
                `deque`, `Counter`, and `OrderedDict` with their `typing` aliases), either at the
                top level or inside transparent wrappers (`|`, `Optional`, `Union`, `Annotated`) and
                covariant container positions (`Sequence[list[T]]`, `Mapping[K, list[V]]`,
                `tuple[...]`, `Awaitable[...]`, and `Callable` return types). Unqualified `Set` is
                exempt only when `from collections.abc import Set` is present in the file.
                Contravariant `Callable` parameter lists and the type arguments of invariant
                containers (`list`, `dict`, `MutableSequence`, `MutableMapping`) are not inspected.
                Dunder methods other than `__init__`, `__new__`, and `__call__`, methods on
                `Protocol` or `ABC` classes, and functions decorated with `@override`, `@overload`,
                `@abstractmethod`, `@fixture`, `@<function>.register`, or `@<property>.setter` are
                exempt. String annotations and module aliases (`import typing as t`) are not
                resolved."},
            why_is_this_bad: indoc::indoc! {r"
                In Python's type system, `list`, `dict`, and `set` are invariant in their type
                parameters and require a mutable concrete container at call sites. A function
                annotated with `items: list[str]` rejects callers holding a `tuple[str, ...]`, a
                `Sequence[str]` parameter, or a `list[SubStr]`, forcing defensive `list(...)`
                copies.

                Choose the annotation from what the function does with the parameter: `Iterable` for
                a single pass, `Collection` for `len()`, `in`, or several passes, `Sequence`,
                `Mapping`, or `Set` (imported as `AbstractSet`) for indexed or keyed reads, and
                their `Mutable` counterparts for in-place mutation. The message names the read-only
                counterpart of the flagged type as a starting point; it does not inspect how the
                parameter is used."},
            references: &[Reference {
                title: "PEP 585: Type Hinting Generics In Standard Collections",
                url: "https://peps.python.org/pep-0585/",
            }],
            examples: &[Example {
                language: SupportLang::Python,
                flagged: indoc::indoc! {r"
                    def total_cents(prices: list[int]) -> int:
                        return sum(prices)
                "},
                flagged_span: "list[int]",
                fixed: indoc::indoc! {r"
                    from collections.abc import Sequence

                    def total_cents(prices: Sequence[int]) -> int:
                        return sum(prices)
                "},
            }],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    let abc_set_imported = has_unaliased_collections_abc_set_import(file);
    let mut diagnostics = Vec::new();
    for signature in extract_function_signatures(file) {
        if signature.is_exempt_from_signature_rules() {
            continue;
        }
        for param in &signature.parameters {
            if param.is_variadic() || param.kind == PythonParameterKind::Receiver {
                continue;
            }
            let Some(ref type_node) = param.type_node else {
                continue;
            };
            let matched: Vec<_> = collect_collection_types(
                type_node,
                AnnotationTraversalDepth::CovariantPositions,
                abc_set_imported,
            )
            .into_iter()
            .filter(|collection_type| collection_type.kind == CollectionKind::ConcreteMutable)
            .collect();
            if matched.is_empty() {
                continue;
            }
            let token = PythonCollectionType::joined_paths(&matched);
            let replacement = read_only_collection_replacements(&matched);
            let expression = type_node.text();
            diagnostics.push(rule.diagnostic_at_node(
                path,
                type_node,
                &[
                    ("name", &param.name),
                    ("function", &signature.name),
                    ("expression", expression.as_ref()),
                    ("token", &token),
                    ("replacement", &replacement),
                ],
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
                abstract_collections => r#"
                    from collections.abc import Mapping, Sequence, Set as AbstractSet

                    def process(
                        items: Sequence[int],
                        lookup: Mapping[str, int],
                        tags: AbstractSet[str],
                    ) -> None:
                        pass
                "#,
                unaliased_collections_abc_set_detected => r#"
                    from collections.abc import Set

                    def process(tags: Set[str]) -> None:
                        pass
                "#,
                immutable_concrete_builtins => r#"
                    def process(
                        coords: tuple[int, ...],
                        flags: frozenset[str],
                        payload: bytes,
                        name: str,
                    ) -> None:
                        pass
                "#,
                annotated_metadata_with_list_ignored => r#"
                    from collections.abc import Sequence
                    from typing import Annotated

                    def process(items: Annotated[Sequence[int], list]) -> None:
                        pass
                "#,
                callable_contravariant_parameter_ignored => r#"
                    from collections.abc import Callable

                    def register(callback: Callable[[list[int]], None]) -> None:
                        pass
                "#,
                invariant_mutable_outer_container_ignored => r#"
                    from collections.abc import MutableMapping, MutableSequence

                    def mutate(
                        mapping: MutableMapping[str, list[int]],
                        seq: MutableSequence[list[int]],
                    ) -> None:
                        pass
                "#,
                unknown_custom_generic_ignored => r#"
                    def process(box: CustomBox[list[int]]) -> None:
                        pass
                "#,
                variadic_args_and_kwargs_exempt => r#"
                    def process(*args: list[int], **kwargs: dict[str, int]) -> None:
                        pass
                "#,
                override_exempt => r#"
                    from typing import override

                    class Service(Base):
                        @override
                        def handle(self, items: list[int]) -> None:
                            pass
                "#,
                abstractmethod_exempt => r#"
                    from abc import abstractmethod

                    class Service:
                        @abstractmethod
                        def compute(self, table: dict[str, int]) -> None:
                            pass
                "#,
                property_setter_exempt => r#"
                    class Basket:
                        @property
                        def items(self):
                            return self._items

                        @items.setter
                        def items(self, value: list[int]) -> None:
                            self._items = value
                "#,
                overload_exempt => r#"
                    from typing import overload

                    @overload
                    def parse(raw: list[str]) -> int: ...
                "#,
                pytest_fixture_exempt => r#"
                    import pytest

                    @pytest.fixture
                    def sample_data(seed: list[int]) -> None:
                        pass
                "#,
                protocol_class_exempt => r#"
                    from typing import Protocol

                    class Repository(Protocol):
                        def save_all(self, records: list[str]) -> None: ...
                "#,
                abc_class_exempt => r#"
                    import abc

                    class BaseWorker(abc.ABC):
                        def enqueue(self, tasks: list[str]) -> None:
                            pass
                "#,
                data_model_dunders_exempt => r#"
                    class Group:
                        def __eq__(self, other: list[int]) -> bool:
                            return False

                        def __contains__(self, item: set[str]) -> bool:
                            return False
                "#,
                singledispatch_register_exempt => r#"
                    from functools import singledispatch

                    @singledispatch
                    def render(value: object) -> str:
                        return str(value)

                    @render.register
                    def _(value: list) -> str:
                        return ", ".join(value)
                "#,
                known_gap_string_annotation_not_parsed => r#"
                    def process(items: "list[int]") -> None:
                        pass
                "#,
                known_gap_typing_module_alias_not_resolved => r#"
                    import typing as t

                    def process(items: t.List[int]) -> None:
                        pass
                "#,
            ],
            fail: [
                bare_concrete_list => r#"
                    def process(items: list) -> None:
                        pass
                "# => "list",
                collections_defaultdict => r#"
                    from collections import defaultdict

                    def group(index: defaultdict[str, list[int]]) -> None:
                        pass
                "# => "defaultdict[str, list[int]]",
                qualified_collections_counter => r#"
                    import collections

                    def tally(counts: collections.Counter[str]) -> None:
                        pass
                "# => "collections.Counter[str]",
                typing_deque_alias => r#"
                    from typing import Deque

                    def drain(queue: Deque[int]) -> None:
                        pass
                "# => "Deque[int]",
                pep585_generic_dict => r#"
                    def process(*, mapping: dict[str, int]) -> None:
                        pass
                "# => "dict[str, int]",
                pep484_qualified_typing_set => r#"
                    import typing

                    def process(tags: typing.Set[str]) -> None:
                        pass
                "# => "typing.Set[str]",
                pep484_unqualified_typing_set => r#"
                    from typing import Set

                    def process(extra: Set[int]) -> None:
                        pass
                "# => "Set[int]",
                pep604_union_consolidates_multiple_concrete_types => r#"
                    def process(items: list[int] | set[str] | None = None) -> None:
                        pass
                "# => "list[int] | set[str] | None",
                optional_and_annotated_wrappers => r#"
                    from typing import Annotated, Optional

                    def process(items: Annotated[Optional[list[int]], "meta"]) -> None:
                        pass
                "# => "Annotated[Optional[list[int]], \"meta\"]",
                covariant_nested_mapping_value => r#"
                    from collections.abc import Mapping

                    def process(index: Mapping[str, list[int]]) -> None:
                        pass
                "# => "Mapping[str, list[int]]",
                covariant_callable_return_type => r#"
                    from collections.abc import Callable

                    def process(factory: Callable[[int], list[str]]) -> None:
                        pass
                "# => "Callable[[int], list[str]]",
                init_constructor_is_checked => r#"
                    class Order:
                        def __init__(self, items: list[str]) -> None:
                            self._items = items
                "# => "list[str]",
                call_dunder_is_checked => r#"
                    class Pipeline:
                        def __call__(self, items: list[int]) -> None:
                            pass
                "# => "list[int]",
                new_constructor_is_checked => r#"
                    class Order:
                        def __new__(cls, items: list[str]) -> "Order":
                            return super().__new__(cls)
                "# => "list[str]",
                aliased_collections_abc_set_import_still_flags_typing_set => r#"
                    from collections.abc import Set as AbstractSet
                    from typing import Set

                    def process(tags: Set[str]) -> None:
                        pass
                "# => "Set[str]",
            ],
        },
    }
);
