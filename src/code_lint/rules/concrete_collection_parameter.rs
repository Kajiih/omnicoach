//! Flags Python function parameters annotated with concrete mutable collection types (`concrete-collection-parameter`).

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    PythonFunctionSignature, PythonParameterKind, collect_concrete_collection_types,
    extract_function_signatures, has_exempt_signature_decorator, is_exempt_dunder_method,
    is_in_protocol_or_abc_class,
};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Parameter `{name}` of `{function}` is annotated with concrete collection type `{expression}` (`{token}`).",
    rationale: "Concrete mutable collection types such as `list`, `dict`, and `set` are invariant in their type arguments and reject read-only inputs such as `tuple`, `frozenset`, or subtype sequences.",
    suggestion: "Replace `{token}` in `{name}` with a read-only abstract collection from `collections.abc` (`Sequence`, `Mapping`, or `AbstractSet`), or with `MutableSequence`, `MutableMapping`, or `MutableSet` when `{function}` mutates `{name}` in place.",
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
            what_it_does: "Flags non-variadic parameters of functions and methods in Python \
                           source files (test files are not checked) whose type annotation uses \
                           a concrete mutable collection constructor (`list`, `dict`, `set`, \
                           `typing.List`, `typing.Dict`, or `typing.Set`), either at the top \
                           level or inside transparent wrappers (`|`, `Optional`, `Union`, \
                           `Annotated`) and covariant container positions (`Sequence[list[T]]`, \
                           `Mapping[K, list[V]]`, `tuple[...]`, `Awaitable[...]`, and `Callable` \
                           return types). Contravariant `Callable` parameter lists and invariant \
                           `MutableSequence` or `MutableMapping` type arguments are not \
                           flagged. Dunder methods other than `__init__` and `__new__`, methods \
                           on `Protocol` or `ABC` classes, and functions decorated with \
                           `@override`, `@overload`, `@abstractmethod`, or `@fixture` are exempt.",
            why_is_this_bad: "In Python's type system, `list`, `dict`, and `set` are invariant \
                              in their type parameters and require a mutable concrete container \
                              at call sites. A function annotated with `items: list[str]` \
                              rejects callers holding a `tuple[str, ...]`, a `Sequence[str]` \
                              parameter, or a `list[SubStr]`, forcing defensive `list(...)` \
                              copies.\n\n\
                              Annotate read-only parameters with `Sequence`, `Mapping`, or `Set` \
                              from `collections.abc` (or `Collection` / `Iterable`), and \
                              in-place mutating parameters with `MutableSequence`, \
                              `MutableMapping`, or `MutableSet`.",
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

fn is_exempt_function(signature: &PythonFunctionSignature<'_>) -> bool {
    is_exempt_dunder_method(&signature.name)
        || has_exempt_signature_decorator(&signature.node)
        || is_in_protocol_or_abc_class(&signature.node)
}

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for signature in extract_function_signatures(file) {
        if is_exempt_function(&signature) {
            continue;
        }
        for param in &signature.parameters {
            if param.is_variadic() || param.kind == PythonParameterKind::Receiver {
                continue;
            }
            let Some(ref type_node) = param.type_node else {
                continue;
            };
            let matched = collect_concrete_collection_types(type_node);
            if matched.is_empty() {
                continue;
            }
            let token = matched.join(", ");
            let expression = type_node.text();
            diagnostics.push(rule.diagnostic_at_node(
                path,
                type_node,
                &[
                    ("name", &param.name),
                    ("function", &signature.name),
                    ("expression", expression.as_ref()),
                    ("token", &token),
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
                    from collections.abc import Mapping, Sequence, Set
                    from typing import AbstractSet

                    def process(
                        items: Sequence[int],
                        lookup: Mapping[str, int],
                        tags: Set[str],
                        legacy_tags: AbstractSet[str],
                    ) -> None:
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
                override_overload_abstract_fixture_exempt => r#"
                    from abc import abstractmethod
                    from typing import overload, override
                    import pytest

                    class Service:
                        @override
                        def handle(self, items: list[int]) -> None:
                            pass

                        @abstractmethod
                        def compute(self, table: dict[str, int]) -> None:
                            pass

                    @overload
                    def parse(raw: list[str]) -> int: ...

                    @pytest.fixture
                    def sample_data(seed: list[int]) -> None:
                        pass
                "#,
                protocol_and_abc_classes_exempt => r#"
                    import abc
                    from typing import Protocol

                    class Repository(Protocol):
                        def save_all(self, records: list[str]) -> None: ...

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
            ],
            fail: [
                bare_concrete_list => r#"
                    def process(items: list) -> None:
                        pass
                "# => "list",
                pep585_generic_dict => r#"
                    def process(*, mapping: dict[str, int]) -> None:
                        pass
                "# => "dict[str, int]",
                pep484_qualified_typing_set => r#"
                    import typing

                    def process(tags: typing.Set[str]) -> None:
                        pass
                "# => "typing.Set[str]",
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
            ],
        },
    }
);
