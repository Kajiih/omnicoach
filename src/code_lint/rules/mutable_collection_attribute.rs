//! Flags public Python class and instance attributes annotated with abstract mutable collection types that are never mutated in the class.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    AnnotationTraversalDepth, CollectionKind, PythonCollectionType, collect_class_attributes,
    collect_collection_types, has_unaliased_collections_abc_set_import,
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
    summary: "Attribute `{name}` of `{class}` has annotation `{expression}`, which contains mutable collection type `{token}`, but no method of `{class}` appears to mutate `{name}`.",
    rationale: "A public attribute typed as `MutableSequence`, `MutableMapping`, or `MutableSet` turns assigning a read-only collection (`tuple`, `Sequence`, `Mapping`) into a type error and exposes mutability on the instance.",
    suggestion: "Replace `{token}` on `{name}` with `{replacement}` if code outside `{class}` is not meant to mutate `{name}`, or prefix internal mutable state with `_`.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("mutable-collection-attribute"),
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
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags public Python class and instance attributes annotated with `MutableSequence`, `MutableMapping`, or `MutableSet` when never mutated in the class.",
            what_it_does: indoc::indoc! {r"
                Flags public (non-`_`-prefixed) class attributes (`items: MutableSequence[str]`) and
                `__init__` instance attributes (`self.items: MutableSequence[str]`) in Python source
                files (test files are not checked) whose type annotation uses `MutableSequence`,
                `MutableMapping`, or `MutableSet` (at the top level or inside `ClassVar`, `Final`,
                `Optional`, `Union`, or `|`) when no method of the class mutates `self.<name>` or
                `cls.<name>` in place. Nested types such as `Sequence[MutableMapping[K, V]]` are not
                checked, because mutation of the elements is not tracked. Private attributes
                starting with `_`, attributes mutated in any method of the class, attributes on
                `Protocol` or `ABC` classes, and `TypedDict` keys (a `TypedDict` has no methods to
                mutate them) are not flagged."},
            why_is_this_bad: indoc::indoc! {r"
                On a `@dataclass` or public class interface, annotating a read-only field as
                `MutableSequence`, `MutableMapping`, or `MutableSet` makes its type invariant,
                rejects `tuple` or `Sequence` arguments in synthesized constructors, and exposes a
                mutable container on the instance.

                A mutable public attribute is a contract that outside code may mutate it, so it is a
                deliberate choice. Otherwise, `Sequence`, `Mapping`, or `Set` (imported as
                `AbstractSet`) state a read-only field, and internal mutable state belongs in a
                `_`-prefixed attribute. Only methods of the class are checked for mutation; code
                outside the class is not."},
            references: &[Reference {
                title: "Python collections.abc — Collections Abstract Base Classes",
                url: "https://docs.python.org/3/library/collections.abc.html",
            }],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r"
                    from collections.abc import MutableSequence
                    from dataclasses import dataclass

                    @dataclass(frozen=True, slots=True)
                    class Order:
                        items: MutableSequence[str]
                "},
                flagged_span: "MutableSequence[str]",
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
    let abc_set_imported = has_unaliased_collections_abc_set_import(file);
    let mut diagnostics = Vec::new();
    for attribute in collect_class_attributes(file) {
        if attribute.name.starts_with('_')
            || attribute.is_in_protocol_or_abc
            || attribute.is_mutated_in_class
            || attribute.is_typed_dict_key
        {
            continue;
        }
        let matched: Vec<_> = collect_collection_types(
            &attribute.type_node,
            AnnotationTraversalDepth::TransparentWrappersOnly,
            abc_set_imported,
        )
        .into_iter()
        .filter(|collection_type| collection_type.kind == CollectionKind::AbstractMutable)
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
                readonly_abstract_attributes => r#"
                    from collections.abc import Mapping, Sequence, Set as AbstractSet
                    from dataclasses import dataclass

                    @dataclass(frozen=True)
                    class Config:
                        hosts: Sequence[str]
                        limits: Mapping[str, int]
                        tags: AbstractSet[str]
                "#,
                mutated_in_class_method => r#"
                    from collections.abc import MutableMapping, MutableSequence, MutableSet

                    class Registry:
                        items: MutableSequence[str]
                        counts: MutableMapping[str, int]
                        tags: MutableSet[str]

                        def record(self, item: str) -> None:
                            self.items.append(item)
                            self.counts[item] = 1
                            self.tags |= {item}
                "#,
                private_mutable_attributes_exempt => r#"
                    from collections.abc import MutableSequence

                    class Queue:
                        _items: MutableSequence[str]

                        def __init__(self) -> None:
                            self._buffer: MutableSequence[int] = []
                "#,
                explained_public_mutable_attribute => r#"
                    from collections.abc import MutableSequence

                    class BatchContext:
                        # Callers append emitted events directly to this buffer during traversal.
                        events: MutableSequence[str]
                "#,
                protocol_class_exempt => r#"
                    from collections.abc import MutableSequence
                    from typing import Protocol

                    class HasBuffer(Protocol):
                        buffer: MutableSequence[str]
                "#,
                abc_class_exempt => r#"
                    import abc
                    from collections.abc import MutableSequence

                    class BaseCollector(abc.ABC):
                        items: MutableSequence[str]
                "#,
                typed_dict_keys_exempt => r#"
                    from collections.abc import MutableSequence
                    from typing import TypedDict

                    class Payload(TypedDict):
                        items: MutableSequence[str]
                "#,
                nested_mutable_elements_not_checked => r#"
                    from collections.abc import MutableMapping, Sequence

                    class Table:
                        rows: Sequence[MutableMapping[str, int]]

                        def bump(self) -> None:
                            for row in self.rows:
                                row["hits"] += 1
                "#,
            ],
            fail: [
                unmutated_dataclass_mutable_sequence_attribute => r#"
                    from collections.abc import MutableSequence
                    from dataclasses import dataclass

                    @dataclass(frozen=True)
                    class Order:
                        items: MutableSequence[str]
                "# => "MutableSequence[str]",
                readonly_method_access_still_flags_attribute => r#"
                    from collections.abc import MutableMapping

                    class Summary:
                        counts: MutableMapping[str, int]

                        def total(self) -> int:
                            return sum(self.counts.values())
                "# => "MutableMapping[str, int]",
                init_public_mutable_attribute => r#"
                    from collections.abc import MutableSet

                    class Client:
                        def __init__(self) -> None:
                            self.tags: MutableSet[str] = set()
                "# => "MutableSet[str]",
            ],
        },
    }
);
