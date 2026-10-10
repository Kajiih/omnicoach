//! Flags function return annotations that wrap a collection type in `| None`, `Optional`, or `Option`.

use crate::code_lint::ast::python::PythonReturnTypeBranch;
use crate::code_lint::ast::rust::NullableReturnPayload;
use crate::code_lint::ast::{self, AstNode, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, EnforcementMode, Example, ImpactedQuality,
    LanguageDefaults, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Return annotation `{expression}` of `{function}` makes collection type `{token}` nullable.",
    rationale: {
        Python => "Wrapping a collection return type in `| None` or `Optional` creates two representations for an empty result and forces callers to check for `None` before iterating or querying length.",
        Rust => "Wrapping a collection return type in `Option` creates two representations for an empty result and forces callers to unwrap or match before iterating or querying length.",
    },
    suggestion: {
        Python => "Remove `None` from the return annotation of `{function}` and return an empty collection such as `()`, `[]`, `{}`, or `frozenset()` when no elements are present.",
        Rust => "Remove `Option` from the return type of `{function}` and return an empty collection such as `Vec::new()`, `&[]`, `BTreeMap::new()`, or `HashSet::new()` when no elements are present.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("nullable-collection-return"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions {
            enforcement_mode: Some(LanguageDefaults::new(
                EnforcementMode::RequireExplanation,
                &[],
            )),
            options: (),
        },
        classification: Classification {
            topics: &[Topic::STATIC_TYPING],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags function return annotations that wrap a collection type in `| None`, `Optional`, or `Option`.",
            what_it_does: indoc::indoc! {r"
                Flags functions and methods whose return annotation makes a collection type
                nullable, such as `-> Sequence[str] | None` in Python or `-> Option<Vec<String>>` in
                Rust. A union that also admits a non-collection type is not flagged, and
                fixed-length tuples and strings do not count as collections. Methods that cannot
                freely change their signature, such as `@override` methods, `Protocol` and `ABC`
                members, most dunder methods and Rust trait items and trait implementations, are
                skipped."},
            why_is_this_bad: indoc::indoc! {r"
                A collection type (`Sequence`, `Mapping`, `Set`, `list`, `dict`, `Vec<T>`, `&[T]`,
                `BTreeMap<K, V>`, `HashSet<T>`) already has an empty value (`()`, `[]`, `{}`,
                `Vec::new()`, `&[]`) that represents zero elements. Returning `Sequence[T] | None`
                in Python or `Option<Vec<T>>` in Rust splits the empty case across two states,
                forcing every caller to branch on `None` (`for item in get_items() or ():` or
                `if let Some(items) = get_items()`) before iterating, indexing, or querying length.

                Return an empty collection when no items are found so callers can iterate and query
                length unconditionally. A nullable collection return is only needed for a
                three-state contract where `None` means something distinct from zero elements, such
                as a cache miss, an unparsed field, or an omitted filter."},
            known_problems: Some(indoc::indoc! {r"
                Type aliases such as `Names: TypeAlias = list[str]` or `type Names = Vec<String>;`
                are not expanded, so return types that use them are not checked."}),
            references: &[
                Reference {
                    title: "SonarSource RSPEC-1168: Empty arrays and collections should be returned instead of null",
                    url: "https://rules.sonarsource.com/java/RSPEC-1168/",
                },
                Reference {
                    title: "PMD: ReturnEmptyCollectionRatherThanNull",
                    url: "https://pmd.github.io/pmd/pmd_rules_java_design.html#returnemptycollectionratherthannull",
                },
                Reference::NAME_RESOLUTION,
            ],
            examples: &[
                Example {
                    language: Language::Python,
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
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r"
                        fn active_tags(&self) -> Option<&[String]> {
                            self.tags.as_deref()
                        }
                    "},
                    flagged_span: "Option<&[String]>",
                    fixed: indoc::indoc! {r"
                        fn active_tags(&self) -> &[String] {
                            self.tags.as_deref().unwrap_or(&[])
                        }
                    "},
                },
            ],
        },
    },
    // Test helpers often shape a return value to match what an assertion compares: an
    // `Option` mirrors another `Option`, and `None` means "nothing to compare" rather than
    // an empty result.
    target: RuleTarget::SourceOnly,
    check: check_file,
};

const RUST_COLLECTION_TYPES: &[&str] = &[
    "Vec",
    "VecDeque",
    "LinkedList",
    "HashMap",
    "BTreeMap",
    "HashSet",
    "BTreeSet",
    "BinaryHeap",
];

/// A return annotation that makes a collection type nullable.
struct NullableCollectionReturn<'a> {
    /// The function's name.
    function: String,
    /// The return annotation.
    return_type: AstNode<'a>,
    /// The collection types made nullable, joined with `", "`.
    collections: String,
}

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    let findings = match file.lang() {
        Language::Python => python_nullable_collection_returns(file),
        Language::Rust => rust_nullable_collection_returns(file),
    };
    findings
        .iter()
        .map(|finding| {
            let expression = finding.return_type.text();
            rule.diagnostic_at_node(
                path,
                &finding.return_type,
                &[
                    (FUNCTION, &finding.function),
                    (EXPRESSION, expression.as_ref()),
                    (TOKEN, &finding.collections),
                ],
            )
        })
        .collect()
}

const FUNCTION: &str = "function";
const EXPRESSION: &str = "expression";
const TOKEN: &str = "token";

/// Python return annotations whose union contains `None` and only collection branches.
fn python_nullable_collection_returns(file: &ParsedFile) -> Vec<NullableCollectionReturn<'_>> {
    ast::python::extract_function_signatures(file)
        .into_iter()
        .filter(|signature| !signature.is_exempt_from_signature_rules())
        .filter_map(|signature| {
            let return_type = signature.return_type_node?;
            let union = ast::python::return_type_union(&return_type);
            if !union.has_none {
                return None;
            }
            let mut collection_types = Vec::new();
            for branch in &union.branches {
                let type_name = python_collection_branch_type(branch)?;
                if !collection_types.contains(&type_name) {
                    collection_types.push(type_name);
                }
            }
            (!collection_types.is_empty()).then(|| NullableCollectionReturn {
                function: signature.name,
                return_type,
                collections: collection_types.join(", "),
            })
        })
        .collect()
}

fn python_collection_branch_type(branch: &PythonReturnTypeBranch<'_>) -> Option<String> {
    let collection = branch.collection.as_ref()?;
    if let Some(args) = &branch.type_arguments
        && matches!(collection.name.as_str(), "tuple" | "Tuple")
    {
        let is_variadic = args.len() == 2 && args[1].text() == "...";
        if !is_variadic {
            return None;
        }
    }
    Some(collection.path.clone())
}

/// Rust return types that wrap a collection in `Option`, outside tests, traits and trait impls.
fn rust_nullable_collection_returns(file: &ParsedFile) -> Vec<NullableCollectionReturn<'_>> {
    ast::rust::collect_functions(file)
        .into_iter()
        .filter(|function| {
            !function.node.is_in_rust_inline_test() && !function.is_trait_or_trait_impl
        })
        .filter_map(|function| {
            let return_type = function.return_type?;
            let collections = extract_rust_nullable_collection_type(&return_type)?;
            Some(NullableCollectionReturn {
                function: function.name,
                return_type,
                collections,
            })
        })
        .collect()
}

fn extract_rust_nullable_collection_type(return_type_node: &AstNode<'_>) -> Option<String> {
    match ast::rust::nullable_return_payload(return_type_node)? {
        NullableReturnPayload::Slice(slice_type) => Some(slice_type),
        NullableReturnPayload::Generic {
            base,
            path,
            terminal,
        } => is_rust_collection_constructor(&path, &terminal).then_some(base),
    }
}

fn is_rust_collection_constructor(path: &str, terminal: &str) -> bool {
    if !RUST_COLLECTION_TYPES.contains(&terminal) {
        return false;
    }
    path == terminal
        || path.strip_suffix(terminal).is_some_and(|prefix| {
            matches!(
                prefix,
                "vec::"
                    | "std::vec::"
                    | "alloc::vec::"
                    | "collections::"
                    | "std::collections::"
                    | "alloc::collections::"
            )
        })
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
                mixed_collection_with_non_collection_union_exempt => r#"
                    from collections.abc import Sequence
                    from typing import Optional

                    def resolve_target(spec: str) -> str | Sequence[str] | None:
                        return None

                    def parse_value(raw: str) -> Optional[int | list[int]]:
                        return None
                "#,
                nullable_parameter_not_flagged => r#"
                    from collections.abc import Mapping, Sequence

                    def configure(
                        items: Sequence[str] | None = None,
                        mapping: Mapping[str, int] | None = None,
                    ) -> None:
                        pass
                "#,
                nullable_attribute_not_flagged => r#"
                    from collections.abc import Sequence

                    class Config:
                        tags: Sequence[str] | None = None

                        def __init__(self, items: Sequence[str] | None = None) -> None:
                            self.items: Sequence[str] | None = items
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
                iterator_or_none => r#"
                    from collections.abc import Iterator

                    def iter_ids() -> Iterator[int] | None:
                        return None
                "# => "Iterator[int] | None",
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
        Rust => {
            pass: [
                non_nullable_rust_collection_returns => r#"
                    use std::collections::{BTreeMap, HashSet};

                    fn get_users() -> Vec<String> {
                        Vec::new()
                    }

                    fn get_tags(&self) -> &[String] {
                        &[]
                    }

                    fn get_counts() -> BTreeMap<String, usize> {
                        BTreeMap::new()
                    }

                    fn get_flags() -> HashSet<String> {
                        HashSet::new()
                    }
                "#,
                non_collection_rust_option_returns => r#"
                    struct User;

                    fn find_name(user_id: u64) -> Option<String> {
                        None
                    }

                    fn find_slice(raw: &str) -> Option<&str> {
                        None
                    }

                    fn find_user(user_id: u64) -> Option<User> {
                        None
                    }

                    fn parse_pair(raw: &str) -> Option<(String, u32)> {
                        None
                    }

                    fn parse_header(raw: &[u8]) -> Option<[u8; 4]> {
                        None
                    }
                "#,
                collection_of_option_elements_not_flagged => r#"
                    fn get_scores() -> Vec<Option<i32>> {
                        vec![Some(1), None]
                    }

                    fn get_slices<'a>() -> &'a [Option<&'a str>] {
                        &[]
                    }
                "#,
                nullable_parameter_not_flagged => r#"
                    fn configure(items: Option<&[String]>, tags: Option<Vec<String>>) {
                        let _ = (items, tags);
                    }
                "#,
                nullable_struct_field_not_flagged => r#"
                    struct Config {
                        tags: Option<Vec<String>>,
                    }
                "#,
                trait_declaration_exempt => r#"
                    trait TagStore {
                        fn required_tags(&self, user_id: &str) -> Option<Vec<String>>;

                        fn default_tags(&self, user_id: &str) -> Option<Vec<String>> {
                            let _ = user_id;
                            None
                        }
                    }
                "#,
                trait_impl_exempt => r#"
                    struct MemoryStore;

                    impl TagStore for MemoryStore {
                        fn required_tags(&self, user_id: &str) -> Option<Vec<String>> {
                            let _ = user_id;
                            None
                        }
                    }
                "#,
                explained_attributed_rust_function_exempt => r#"
                    // Returns None on cache miss; an empty slice means the user has no tags.
                    #[must_use]
                    fn cached_tags(&self, user_id: &str) -> Option<&[String]> {
                        let _ = user_id;
                        None
                    }
                "#,
                locally_shadowed_vec_exempt => r#"
                    struct Vec<T>(T);

                    fn get_wrapped() -> Option<Vec<String>> {
                        None
                    }
                "#,
            ],
            fail: [
                option_vec_return => r#"
                    fn get_users() -> Option<Vec<String>> {
                        None
                    }
                "# => "Option<Vec<String>>",
                qualified_std_option_hashmap_return => r#"
                    fn get_counts() -> std::option::Option<std::collections::HashMap<String, usize>> {
                        None
                    }
                "# => "std::option::Option<std::collections::HashMap<String, usize>>",
                aliased_hashmap_return => r#"
                    use std::collections::HashMap as Map;

                    fn get_counts() -> Option<Map<String, usize>> {
                        None
                    }
                "# => "Option<Map<String, usize>>",
                option_btreeset_return => r#"
                    use std::collections::BTreeSet;

                    fn get_tags() -> Option<BTreeSet<String>> {
                        None
                    }
                "# => "Option<BTreeSet<String>>",
                option_vecdeque_return => r#"
                    use std::collections::VecDeque;

                    fn get_queue() -> Option<VecDeque<u32>> {
                        None
                    }
                "# => "Option<VecDeque<u32>>",
                option_shared_slice_return => r#"
                    fn get_bytes<'a>(input: &'a [u8]) -> Option<&'a [u8]> {
                        let _ = input;
                        None
                    }
                "# => "Option<&'a [u8]>",
                option_boxed_slice_return => r#"
                    fn get_items() -> Option<Box<[String]>> {
                        None
                    }
                "# => "Option<Box<[String]>>",
                option_arc_slice_return => r#"
                    use std::sync::Arc;

                    fn get_shared() -> Option<Arc<[String]>> {
                        None
                    }
                "# => "Option<Arc<[String]>>",
                option_cow_slice_return => r#"
                    use std::borrow::Cow;

                    fn get_borrowed<'a>() -> Option<Cow<'a, [String]>> {
                        None
                    }
                "# => "Option<Cow<'a, [String]>>",
                result_wrapping_option_vec_return => r#"
                    fn load_users() -> Result<Option<Vec<String>>, std::io::Error> {
                        Ok(None)
                    }
                "# => "Result<Option<Vec<String>>, std::io::Error>",
                poll_wrapping_option_vec_return => r#"
                    use std::task::Poll;

                    fn poll_batch() -> Poll<Option<Vec<u8>>> {
                        Poll::Ready(None)
                    }
                "# => "Poll<Option<Vec<u8>>>",
                inherent_impl_method_option_vec_return => r#"
                    struct Store;

                    impl Store {
                        fn load_tags(&self) -> Option<Vec<String>> {
                            None
                        }
                    }
                "# => "Option<Vec<String>>",
                rust_body_comment_does_not_count_as_header_explanation => r#"
                    #[must_use]
                    fn get_items() -> Option<Vec<String>> {
                        // Internal implementation comment inside the body block.
                        None
                    }
                "# => "Option<Vec<String>>",
            ],
        },
    }
);
