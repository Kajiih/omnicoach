//! Flags Python function return annotations that use concrete mutable collection types.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    AnnotationTraversalDepth, CollectionKind, PythonCollectionType, collect_collection_types,
    extract_function_signatures,
};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::code_lint::policy::read_only_collection_replacements;
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, EnforcementMode, Example, ImpactedQuality,
    LanguageDefaults, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Return annotation `{expression}` of `{function}` contains concrete collection type `{token}`.",
    rationale: "Returning an invariant concrete collection type such as `list`, `dict`, or `set` exposes mutability across the boundary and forces callers holding a `Sequence` or `Mapping` to copy before returning.",
    suggestion: "Replace `{token}` in the return annotation of `{function}` with the `collections.abc` type that states what callers may do with the result, such as `{replacement}` for a read-only result or its `Mutable` counterpart for a result callers mutate.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("concrete-collection-return"),
        template: &TEMPLATE,
        languages: &[Language::Python],
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
            summary: "Flags Python function return annotations using concrete mutable collection types such as `list`, `dict`, or `set`.",
            what_it_does: indoc::indoc! {r"
                Flags functions and methods whose return annotation uses a concrete mutable
                collection type, such as `-> list[str]`, including inside covariant wrappers such
                as `Sequence[list[str]]`. Methods that cannot freely change their signature, such
                as `@override` methods, `Protocol` and `ABC` members and most dunder methods, are
                skipped."},
            why_is_this_bad: indoc::indoc! {r"
                Returning a concrete `list`, `dict`, or `set` exposes internal state to in-place
                caller mutation and locks the implementation into returning an invariant mutable
                container even when it could otherwise return a cached `tuple`, a `Sequence` view,
                or a parameter directly without copying.

                Choose the return type as a contract: `Sequence`, `Mapping`, or `Set` (imported as
                `AbstractSet`) for read-only results, and their `Mutable` counterparts for results
                callers are meant to mutate. A concrete collection is a deliberate exception, for
                example when callers rely on `list.sort()`. The message names the read-only
                counterpart of the flagged type; it does not know how callers use the result."},
            known_problems: Some(indoc::indoc! {r"
                Type aliases such as `Names: TypeAlias = list[str]` are not expanded, so annotations
                that use them are not checked."}),
            references: &[
                Reference {
                    title: "PEP 585: Type Hinting Generics In Standard Collections",
                    url: "https://peps.python.org/pep-0585/",
                },
                Reference::NAME_RESOLUTION,
            ],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r"
                    def active_tags(self) -> list[str]:
                        return self._tags
                "},
                flagged_span: "list[str]",
                fixed: indoc::indoc! {r"
                    from collections.abc import Sequence

                    def active_tags(self) -> Sequence[str]:
                        return self._tags
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
        let matched: Vec<_> = collect_collection_types(
            return_type_node,
            AnnotationTraversalDepth::CovariantPositions,
        )
        .into_iter()
        .filter(|collection_type| collection_type.kind == CollectionKind::ConcreteMutable)
        .collect();
        if matched.is_empty() {
            continue;
        }
        let token = PythonCollectionType::joined_paths(&matched);
        let replacement = read_only_collection_replacements(&matched);
        let expression = return_type_node.text();
        diagnostics.push(rule.diagnostic_at_node(
            path,
            return_type_node,
            &[
                ("function", &signature.name),
                ("expression", expression.as_ref()),
                ("token", &token),
                ("replacement", &replacement),
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
                abstract_return_types => r#"
                    from collections.abc import Iterable, Mapping, Sequence, Set as AbstractSet

                    def get_users() -> Sequence[str]:
                        return ["alice"]

                    def get_counts() -> Mapping[str, int]:
                        return {"a": 1}

                    def get_tags() -> AbstractSet[str]:
                        return {"v1"}

                    def stream_ids() -> Iterable[int]:
                        return [1, 2]
                "#,
                unaliased_collections_abc_set_detected => r#"
                    from collections.abc import Set

                    def get_tags() -> Set[str]:
                        return {"v1"}
                "#,
                immutable_concrete_return_types => r#"
                    def get_coords() -> tuple[int, ...]:
                        return (1, 2)

                    def get_flags() -> frozenset[str]:
                        return frozenset()
                "#,
                explained_by_preceding_header_comment => r#"
                    # Callers sort and append to the returned buffer in place.
                    def make_buffer() -> list[str]:
                        return []
                "#,
                explained_on_decorated_function_header => r#"
                    class Builder:
                        # Callers mutate the returned list directly to assemble batches.
                        @staticmethod
                        def make_batch() -> list[int]:
                            return []
                "#,
                protocol_class_exempt => r#"
                    from typing import Protocol

                    class P(Protocol):
                        def items(self) -> list[str]: ...
                "#,
                abstractmethod_exempt => r#"
                    import abc

                    class Base:
                        @abc.abstractmethod
                        def keys(self) -> set[str]:
                            pass
                "#,
                override_exempt => r#"
                    from typing import override

                    class Impl(Base):
                        @override
                        def keys(self) -> set[str]:
                            return set()
                "#,
                data_model_dunder_exempt => r#"
                    class Impl:
                        def __dir__(self) -> list[str]:
                            return []
                "#,
                overload_exempt => r#"
                    from typing import overload

                    @overload
                    def fetch(x: int) -> list[int]: ...
                "#,
            ],
            fail: [
                unexplained_concrete_list_return => r#"
                    def get_users() -> list[str]:
                        return ["alice"]
                "# => "list[str]",
                ordered_dict_return => r#"
                    from collections import OrderedDict

                    def ranking() -> OrderedDict[str, int]:
                        return OrderedDict()
                "# => "OrderedDict[str, int]",
                unqualified_typing_set_return => r#"
                    from typing import Set

                    def get_tags() -> Set[str]:
                        return {"a"}
                "# => "Set[str]",
                union_and_optional_concrete_return => r#"
                    from typing import Optional

                    def get_config() -> Optional[dict[str, int]]:
                        return None
                "# => "Optional[dict[str, int]]",
                covariant_nested_awaitable_return => r#"
                    from collections.abc import Awaitable

                    def fetch_tags() -> Awaitable[set[str]]:
                        raise NotImplementedError
                "# => "Awaitable[set[str]]",
                body_comment_does_not_count_as_header_explanation => r#"
                    @staticmethod
                    def get_items() -> list[str]:
                        # Internal implementation comment inside the body block.
                        return ["a"]
                "# => "list[str]",
            ],
        },
    }
);
