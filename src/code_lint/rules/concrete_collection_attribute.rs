//! Flags public Python class and instance attributes annotated with concrete mutable collection types.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    AnnotationTraversalDepth, CollectionKind, PythonCollectionType, collect_class_attributes,
    collect_collection_types,
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
    summary: "Attribute `{name}` of `{class}` has annotation `{expression}`, which contains concrete collection type `{token}`.",
    rationale: "A public attribute typed as `list`, `dict`, or `set` turns assigning a `tuple`, `Sequence`, or `Mapping` (including through a dataclass constructor) into a type error and exposes internal state to in-place caller mutation.",
    suggestion: "Replace `{token}` on `{name}` with the `collections.abc` type that states what users of `{class}` may do with it, such as `{replacement}` for read-only access or its `Mutable` counterpart for in-place mutation, or prefix internal state with `_`.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("concrete-collection-attribute"),
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
            summary: "Flags public Python class and instance attributes annotated with concrete mutable collection types such as `list`, `dict`, or `set`.",
            what_it_does: indoc::indoc! {r"
                Flags public class and instance attributes whose annotation uses a concrete mutable
                collection type, such as `items: list[str]` in a class body or
                `self.counts: dict[str, int]` in `__init__`, including inside covariant wrappers
                such as `Sequence[list[str]]`. Attributes of `Protocol` and `ABC` classes are
                skipped."},
            why_is_this_bad: indoc::indoc! {r"
                On a `@dataclass` or public class interface, annotating a field as
                `items: list[str]` forces callers constructing the class to pass a concrete `list`
                rather than a `tuple` or an upstream `Sequence[str]` parameter, and exposes a
                mutable container on the instance even when `@dataclass(frozen=True)` is used.

                Choose the annotation as a contract: `Sequence`, `Mapping`, or `Set` (imported as
                `AbstractSet`) for read-only fields, and their `Mutable` counterparts for fields
                mutated in place. Internal mutable state belongs in a `_`-prefixed attribute. A
                public concrete collection is a deliberate exception. The message names the
                read-only counterpart of the flagged type; it does not check how the attribute is
                used."},
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
                    from dataclasses import dataclass

                    @dataclass(frozen=True, slots=True)
                    class Order:
                        items: list[str]
                "},
                flagged_span: "list[str]",
                fixed: indoc::indoc! {r"
                    from collections.abc import Sequence
                    from dataclasses import dataclass

                    @dataclass(frozen=True, slots=True)
                    class Order:
                        items: Sequence[str]
                "},
            }],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for attribute in collect_class_attributes(file) {
        if attribute.name.starts_with('_') || attribute.is_in_protocol_or_abc {
            continue;
        }
        let matched: Vec<_> = collect_collection_types(
            &attribute.type_node,
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
        let expression = attribute.type_node.text();
        diagnostics.push(rule.diagnostic_at_node(
            path,
            &attribute.type_node,
            &[
                ("name", &attribute.name),
                ("class", &attribute.class_name),
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
                abstract_read_only_attributes => r#"
                    from collections.abc import Mapping, Sequence, Set as AbstractSet
                    from dataclasses import dataclass

                    @dataclass(frozen=True)
                    class Config:
                        hosts: Sequence[str]
                        limits: Mapping[str, int]
                        tags: AbstractSet[str]
                "#,
                abstract_mutable_attributes => r#"
                    from collections.abc import MutableSequence

                    class Buffer:
                        items: MutableSequence[int]
                "#,
                immutable_concrete_attributes => r#"
                    class Point:
                        coords: tuple[int, ...]
                "#,
                unaliased_collections_abc_set_detected => r#"
                    from collections.abc import Set

                    class Config:
                        tags: Set[str]
                "#,
                private_attributes_exempt => r#"
                    class Cache:
                        _entries: dict[str, int]

                        def __init__(self) -> None:
                            self._history: list[str] = []
                "#,
                explained_public_concrete_attribute => r#"
                    class WorkerState:
                        # Callers append pending job identifiers directly to this concrete list.
                        pending_jobs: list[str]
                "#,
                module_variable_ignored => r#"
                    MODULE_ITEMS: list[int] = []
                "#,
                function_local_variable_ignored => r#"
                    def compute() -> None:
                        local_items: list[int] = [1, 2]
                "#,
                protocol_class_exempt => r#"
                    from typing import Protocol

                    class HasItems(Protocol):
                        items: list[str]
                "#,
                abc_class_exempt => r#"
                    import abc

                    class AbstractStore(abc.ABC):
                        records: dict[str, int]
                "#,
            ],
            fail: [
                dataclass_concrete_list_attribute => r#"
                    from dataclasses import dataclass

                    @dataclass(frozen=True)
                    class Order:
                        items: list[str]
                "# => "list[str]",
                unqualified_typing_set_attribute => r#"
                    from typing import Set

                    class Config:
                        tags: Set[str]
                "# => "Set[str]",
                classvar_concrete_set_attribute => r#"
                    from typing import ClassVar

                    class Service:
                        DEFAULT_TAGS: ClassVar[set[str]]
                "# => "ClassVar[set[str]]",
                init_public_instance_attribute => r#"
                    class Client:
                        def __init__(self) -> None:
                            self.endpoints: list[str] = []
                "# => "list[str]",
                mutated_public_concrete_attribute_still_flagged_for_mutable_abc => r#"
                    class Bag:
                        items: list[str]

                        def add(self, item: str) -> None:
                            self.items.append(item)
                "# => "list[str]",
                covariant_nested_sequence_of_dicts => r#"
                    from collections.abc import Sequence

                    class Table:
                        rows: Sequence[dict[str, int]]
                "# => "Sequence[dict[str, int]]",
                typed_dict_concrete_key_still_flagged => r#"
                    from typing import TypedDict

                    class Payload(TypedDict):
                        items: list[str]
                "# => "list[str]",
            ],
        },
    }
);
