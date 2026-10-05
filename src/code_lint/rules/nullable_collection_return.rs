//! Flags Python function return annotations that wrap a collection type in `| None` or `Optional`.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    collect_nullable_collection_return_types, extract_function_signatures,
};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, EnforcementMode, Example, ImpactedQuality,
    LanguageDefaults, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Return annotation `{expression}` of `{function}` makes collection type `{token}` nullable.",
    rationale: "Wrapping a collection return type in `| None` or `Optional` creates two representations for an empty result and forces callers to check for `None` before iterating or querying length.",
    suggestion: "Remove `None` from the return annotation of `{function}` and return an empty collection such as `()`, `[]`, `{}`, or `frozenset()` when no elements are present.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("nullable-collection-return"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python],
        options: RuleOptions {
            enforcement_mode: Some(LanguageDefaults::new(
                EnforcementMode::RequireExplanation,
                &[],
            )),
            options: (),
        },
        classification: Classification {
            topics: &[Topic::STATIC_TYPING],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags Python function return annotations that wrap a collection type in `| None` or `Optional`.",
            what_it_does: "Flags functions and methods in Python source files (test files are \
                           not checked) whose return annotation (or awaited return type inside \
                           `Awaitable[...]` or `Coroutine[Any, Any, ...]`, or underlying type \
                           inside `Annotated[..., ...]`) is a union containing `None` (`| None`, \
                           `Optional[...]`, or `Union[..., None]`) in which every non-`None` \
                           branch is a collection type (`Sequence`, `MutableSequence`, `Mapping`, \
                           `MutableMapping`, `Set`, `AbstractSet`, `MutableSet`, `Collection`, \
                           `Iterable`, `Reversible`, `list`, `List`, `dict`, `Dict`, `set`, \
                           `frozenset`, `FrozenSet`, `deque`, `Deque`, `defaultdict`, \
                           `DefaultDict`, `Counter`, `OrderedDict`, bare `tuple` / `Tuple`, or \
                           variadic `tuple[T, ...]` / `Tuple[T, ...]`). Fixed-length record \
                           tuples (`tuple[int, str] | None`), unions that mix a collection with \
                           a non-collection type (`str | Sequence[str] | None`), and \
                           non-nullable collections of nullable elements (`Sequence[int | None]`) \
                           are not flagged. Dunder methods other than `__init__`, `__new__`, and \
                           `__call__`, methods on `Protocol` or `ABC` classes, and functions \
                           decorated with `@override`, `@overload`, `@abstractmethod`, \
                           `@fixture`, or `@<function>.register` are exempt.",
            why_is_this_bad: "A collection type (`Sequence`, `Mapping`, `Set`, `Iterable`, \
                              `list`, `dict`, `set`, `tuple[T, ...]`) already has an empty value \
                              (`()`, `[]`, `{}`, `frozenset()`) that represents zero elements. \
                              Returning `Sequence[T] | None` or `Optional[list[T]]` splits the \
                              empty case across `None` and `()`, forcing every caller to branch \
                              on `None` (`for item in get_items() or ():`) before iterating, \
                              indexing, or calling `len()`.\n\n\
                              Return an empty collection when no items are found so callers can \
                              iterate and query length unconditionally. A nullable collection \
                              return is only needed for a three-state contract where `None` \
                              means something distinct from zero elements, such as a cache miss, \
                              an unparsed field, or an omitted filter.",
            references: &[
                Reference {
                    title: "SonarSource RSPEC-1168: Empty arrays and collections should be returned instead of null",
                    url: "https://rules.sonarsource.com/java/RSPEC-1168/",
                },
                Reference {
                    title: "PMD: ReturnEmptyCollectionRatherThanNull",
                    url: "https://pmd.github.io/pmd/pmd_rules_java_design.html#returnemptycollectionratherthannull",
                },
            ],
            examples: &[Example {
                language: SupportLang::Python,
                flagged: indoc::indoc! {r"
                    from collections.abc import Sequence

                    def active_tags(self) -> Sequence[str] | None:
                        return self._tags
                "},
                flagged_span: "Sequence[str] | None",
                fixed: indoc::indoc! {r"
                    from collections.abc import Sequence

                    def active_tags(self) -> Sequence[str]:
                        return self._tags or ()
                "},
            }],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for signature in extract_function_signatures(file) {
        if signature.is_exempt_from_signature_rules() {
            continue;
        }
        let Some(ref return_type_node) = signature.return_type_node else {
            continue;
        };
        let matched = collect_nullable_collection_return_types(return_type_node);
        if matched.is_empty() {
            continue;
        }
        let token = matched.join(", ");
        let expression = return_type_node.text();
        diagnostics.push(rule.diagnostic_at_node(
            path,
            return_type_node,
            &[
                ("function", &signature.name),
                ("expression", expression.as_ref()),
                ("token", &token),
            ],
        ));
    }

    diagnostics
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                non_nullable_collection_returns => r#"
                    from collections.abc import Iterable, Mapping, Sequence, Set as AbstractSet

                    def get_users() -> Sequence[str]:
                        return ("alice",)

                    def get_counts() -> Mapping[str, int]:
                        return {"a": 1}

                    def get_tags() -> AbstractSet[str]:
                        return {"v1"}

                    def stream_ids() -> Iterable[int]:
                        return (1, 2)

                    def get_coords() -> tuple[int, ...]:
                        return (1, 2)

                    def get_flags() -> frozenset[str]:
                        return frozenset()
                "#,
                non_collection_nullable_returns => r#"
                    from typing import Optional, Union

                    class User:
                        pass

                    def find_name(user_id: int) -> str | None:
                        return None

                    def find_count(user_id: int) -> int | None:
                        return None

                    def find_user(user_id: int) -> Optional[User]:
                        return None

                    def parse_token(raw: str) -> Union[int, str, None]:
                        return None
                "#,
                collection_of_nullable_elements => r#"
                    from collections.abc import Mapping, Sequence

                    def get_scores() -> Sequence[int | None]:
                        return (1, None)

                    def get_buckets() -> Mapping[str, list[int] | None]:
                        return {"a": None}

                    def get_samples() -> tuple[float | None, ...]:
                        return (None,)
                "#,
                fixed_length_record_tuple_nullable => r#"
                    from typing import Optional, Tuple

                    def parse_header(line: str) -> tuple[str, int] | None:
                        return None

                    def split_flag(raw: str) -> Tuple[bool, str] | None:
                        return None

                    def parse_single(raw: str) -> Optional[tuple[int]]:
                        return None
                "#,
                mixed_collection_and_scalar_union_with_none => r#"
                    from collections.abc import Sequence
                    from typing import Optional

                    def resolve_target(spec: str) -> str | Sequence[str] | None:
                        return None

                    def parse_value(raw: str) -> Optional[int | list[int]]:
                        return None

                    def resolve_pair(raw: str) -> tuple[int, str] | list[int] | None:
                        return None
                "#,
                nullable_parameters_and_attributes_not_flagged => r#"
                    from collections.abc import Mapping, Sequence

                    class Config:
                        tags: Sequence[str] | None = None

                        def __init__(
                            self,
                            items: Sequence[str] | None = None,
                            mapping: Mapping[str, int] | None = None,
                        ) -> None:
                            self.items: Sequence[str] | None = items
                            self.mapping = mapping
                "#,
                callable_with_nullable_collection_parameter_or_return_not_flagged => r#"
                    from collections.abc import Callable, Sequence

                    def make_loader() -> Callable[[int], Sequence[str] | None]:
                        raise NotImplementedError

                    def find_callback() -> Callable[[Sequence[str]], None] | None:
                        return None
                "#,
                explained_by_preceding_header_comment => r#"
                    from collections.abc import Sequence

                    # Returns None on cache miss; an empty sequence means the user has no tags.
                    def cached_tags(user_id: str) -> Sequence[str] | None:
                        return None
                "#,
                explained_on_decorated_function_header => r#"
                    class Cache:
                        # Returns None when the key has expired; an empty list means zero events.
                        @staticmethod
                        def cached_batch(key: str) -> list[int] | None:
                            return None
                "#,
                protocol_class_exempt => r#"
                    from collections.abc import Sequence
                    from typing import Protocol

                    class TagStore(Protocol):
                        def get_tags(self, user_id: str) -> Sequence[str] | None: ...
                "#,
                abc_class_exempt => r#"
                    import abc
                    from collections.abc import Sequence

                    class BaseStore(abc.ABC):
                        def get_tags(self, user_id: str) -> Sequence[str] | None:
                            return None
                "#,
                abstractmethod_exempt => r#"
                    import abc
                    from collections.abc import Sequence

                    class Store:
                        @abc.abstractmethod
                        def get_tags(self, user_id: str) -> Sequence[str] | None:
                            raise NotImplementedError
                "#,
                override_exempt => r#"
                    from collections.abc import Sequence
                    from typing import override

                    class MemoryStore(Store):
                        @override
                        def get_tags(self, user_id: str) -> Sequence[str] | None:
                            return None
                "#,
                overload_exempt => r#"
                    from collections.abc import Sequence
                    from typing import overload

                    @overload
                    def fetch(x: int) -> Sequence[int] | None: ...
                "#,
                pytest_fixture_exempt => r#"
                    from collections.abc import Sequence
                    import pytest

                    @pytest.fixture
                    def sample_items() -> Sequence[int] | None:
                        return None
                "#,
                singledispatch_register_exempt => r#"
                    from collections.abc import Sequence

                    @process.register
                    def _(value: int) -> Sequence[str] | None:
                        return None
                "#,
                data_model_dunder_exempt => r#"
                    from collections.abc import Sequence

                    class Dynamic:
                        def __getattr__(self, name: str) -> Sequence[str] | None:
                            return None
                "#,
            ],
            fail: [
                pep604_sequence_or_none => r#"
                    from collections.abc import Sequence

                    def get_users() -> Sequence[str] | None:
                        return None
                "# => "Sequence[str] | None",
                none_on_left_of_pep604_union => r#"
                    from collections.abc import Sequence

                    def get_users() -> None | Sequence[str]:
                        return None
                "# => "None | Sequence[str]",
                typing_optional_list => r#"
                    from typing import Optional

                    def get_users() -> Optional[list[str]]:
                        return None
                "# => "Optional[list[str]]",
                qualified_typing_union_mapping_none => r#"
                    from collections.abc import Mapping
                    import typing

                    def get_counts() -> typing.Union[Mapping[str, int], None]:
                        return None
                "# => "typing.Union[Mapping[str, int], None]",
                abstract_set_or_none => r#"
                    from typing import AbstractSet

                    def get_tags() -> AbstractSet[str] | None:
                        return None
                "# => "AbstractSet[str] | None",
                collections_abc_set_or_none => r#"
                    from collections.abc import Set

                    def get_tags() -> Set[str] | None:
                        return None
                "# => "Set[str] | None",
                frozenset_or_none => r#"
                    def get_flags() -> frozenset[str] | None:
                        return None
                "# => "frozenset[str] | None",
                iterable_or_none => r#"
                    from collections.abc import Iterable

                    def stream_ids() -> Iterable[int] | None:
                        return None
                "# => "Iterable[int] | None",
                collections_deque_or_none => r#"
                    from collections import deque

                    def get_queue() -> deque[int] | None:
                        return None
                "# => "deque[int] | None",
                variadic_homogeneous_tuple_or_none => r#"
                    def get_coords() -> tuple[int, ...] | None:
                        return None
                "# => "tuple[int, ...] | None",
                bare_unparameterized_collection_or_none => r#"
                    def get_items() -> tuple | None:
                        return None
                "# => "tuple | None",
                multi_collection_union_with_none => r#"
                    def get_items() -> list[str] | tuple[str, ...] | None:
                        return None
                "# => "list[str] | tuple[str, ...] | None",
                async_awaitable_and_annotated_wrappers => r#"
                    from collections.abc import Awaitable, Sequence
                    from typing import Annotated

                    def fetch_items() -> Annotated[Awaitable[Sequence[str] | None], "meta"]:
                        raise NotImplementedError
                "# => "Annotated[Awaitable[Sequence[str] | None], \"meta\"]",
                coroutine_return_type_wrapper => r#"
                    from collections.abc import Coroutine

                    def fetch_items() -> Coroutine[object, object, dict[str, int] | None]:
                        raise NotImplementedError
                "# => "Coroutine[object, object, dict[str, int] | None]",
                body_comment_does_not_count_as_header_explanation => r#"
                    from collections.abc import Sequence

                    @staticmethod
                    def get_items() -> Sequence[str] | None:
                        # Internal implementation comment inside the body block.
                        return None
                "# => "Sequence[str] | None",
            ],
        },
    }
);
