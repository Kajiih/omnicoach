//! Flags public Python class and instance attributes annotated with concrete collection types.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    AnnotationTraversalDepth, CollectionAbstraction, PythonCollectionType,
    collect_collection_types, extract_classes,
};
use crate::code_lint::contract::{ProjectCodeRule, RuleTarget};
use crate::code_lint::policy::{is_pydantic_model, read_only_collection_replacements};
use crate::code_lint::project::ProjectContext;
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Attribute `{name}` of `{class}` has annotation `{expression}`, which contains concrete collection type `{token}`.",
    rationale: "A public attribute typed as `list`, `dict`, `set`, `tuple[T, ...]`, or `frozenset` turns assigning another compatible collection (such as a `tuple`, `list`, `Sequence`, or `Mapping`, including through a dataclass constructor) into a type error, and mutable containers also expose internal state to in-place caller mutation.",
    suggestion: "Replace `{token}` on `{name}` with the `collections.abc` type that states what users of `{class}` may do with it, such as `{replacement}` for read-only access or its `Mutable` counterpart for in-place mutation, or prefix internal state with `_`.",
};

/// The rule's declaration.
pub const RULE: ProjectCodeRule<(), &'static [Example]> = ProjectCodeRule {
    declaration: Declaration {
        name: RuleName("concrete-collection-attribute"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::STATIC_TYPING],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags public Python class and instance attributes annotated with concrete collection types such as `list`, `dict`, `set`, `tuple`, or `frozenset`.",
            what_it_does: indoc::indoc! {r"
                Flags public class and instance attributes whose annotation uses a concrete
                collection type, such as `items: list[str]`, `items: tuple[str, ...]`, or
                `tags: frozenset[str]` in a class body or `self.counts: dict[str, int]` in
                `__init__`, including inside covariant wrappers such as `Sequence[list[str]]`.
                Fixed-length record tuples (`tuple[int, str]`, `tuple[int]`, `tuple[()]`), variadic
                class-object tuples (`tuple[type[Exception], ...]`), class-body fields of Pydantic
                `BaseModel` subclasses (where concrete collection types control runtime validation
                and coercion), and attributes of `Protocol` and `ABC` classes are skipped."},
            why_is_this_bad: indoc::indoc! {r"
                On a `@dataclass` or public class interface, annotating a field as
                `items: list[str]` or `items: tuple[str, ...]` forces callers constructing the
                class to pass that exact concrete type rather than any `Sequence[str]`, and a
                mutable container (`list`, `dict`, `set`) also exposes internal state to in-place
                mutation even when `@dataclass(frozen=True)` is used.

                Choose the annotation as a contract: `Sequence`, `Mapping`, or `Set` (imported as
                `AbstractSet`) for read-only fields, and their `Mutable` counterparts for fields
                mutated in place. Internal state belongs in a `_`-prefixed attribute. A public
                concrete collection is a deliberate exception. The message names the read-only
                counterpart of the flagged type; it does not check how the attribute is used."},
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

fn check_file(
    rule: &ProjectCodeRule<(), &'static [Example]>,
    path: &Path,
    file: &ParsedFile,
    project: &ProjectContext<'_>,
    (): (),
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let classes = extract_classes(file);
    let current_module = project.python_module_of_path(path);
    for (index, class) in classes.iter().enumerate() {
        if class.is_interface() {
            continue;
        }
        let is_pydantic = is_pydantic_model(current_module.as_deref(), &classes, index, project);
        for attribute in class.annotated_attributes() {
            if attribute.name.starts_with('_') || (is_pydantic && attribute.is_in_class_body) {
                continue;
            }
            let matched: Vec<_> = collect_collection_types(
                &attribute.type_node,
                AnnotationTraversalDepth::CovariantPositions,
            )
            .into_iter()
            .filter(|collection_type| {
                collection_type.abstraction == CollectionAbstraction::Concrete
            })
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
                    ("class", &class.name),
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
                fixed_length_tuple_attributes_exempt => r#"
                    from typing import Tuple

                    class Point:
                        coords: tuple[int, str]
                        legacy_coords: Tuple[int, str]
                        singleton: tuple[int]
                "#,
                empty_tuple_attribute_exempt => r#"
                    class Empty:
                        empty: tuple[()]
                "#,
                class_info_type_tuple_attributes_exempt => r#"
                    from typing import Type, Tuple

                    class RetryPolicy:
                        retryable: tuple[type[Exception], ...]
                        ignored: tuple[type[ValueError] | type[KeyError], ...]
                        legacy: Tuple[Type[Exception], ...]
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
                pydantic_base_model_exempt => r#"
                    from pydantic import BaseModel

                    class OrderConfig(BaseModel):
                        items: list[str]
                        tags: frozenset[str]
                "#,
                local_pydantic_base_model_subclass_exempt => r#"
                    from pydantic import BaseModel

                    class FrozenModel(BaseModel):
                        pass

                    class OrderConfig(FrozenModel):
                        items: list[str]
                        weights: tuple[float, ...]
                "#,
                imported_pydantic_base_model_subclass_exempt => {
                    "pkg/base.py" => r#"
                        from pydantic import BaseModel

                        class FrozenModel(BaseModel):
                            pass
                    "#,
                    "pkg/models.py" => r#"
                        from pkg.base import FrozenModel

                        class OrderConfig(FrozenModel):
                            items: list[str]
                            tags: set[str]
                    "#,
                },
                src_layout_tests_import_pydantic_base_model_subclass_exempt => {
                    "pyproject.toml" => r#"
                        [project]
                        name = "pkg"
                    "#,
                    "src/pkg/__init__.py" => "",
                    "src/pkg/base.py" => r#"
                        from pydantic import BaseModel

                        class FrozenModel(BaseModel):
                            pass
                    "#,
                    "tests/test_models.py" => r#"
                        from pkg.base import FrozenModel

                        class OrderConfig(FrozenModel):
                            items: list[str]
                    "#,
                },
            ],
            fail: [
                dataclass_concrete_list_attribute => r#"
                    from dataclasses import dataclass

                    @dataclass(frozen=True)
                    class Order:
                        items: list[str]
                "# => "list[str]",
                pydantic_init_instance_attribute_still_flagged => r#"
                    from pydantic import BaseModel

                    class OrderConfig(BaseModel):
                        def __init__(self) -> None:
                            self.cached_items: list[str] = []
                "# => "list[str]",
                shadowed_local_base_model_still_flagged => r#"
                    class BaseModel:
                        pass

                    class OrderConfig(BaseModel):
                        items: list[str]
                "# => "list[str]",
                imported_non_pydantic_base_model_still_flagged => {
                    "pkg/base.py" => r#"
                        class BaseModel:
                            pass
                    "#,
                    "pkg/models.py" => r#"
                        from pkg.base import BaseModel

                        class OrderConfig(BaseModel):
                            items: list[str]
                    "#,
                } => ("pkg/models.py", "list[str]"),
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
                variable_length_tuple_attribute => r#"
                    class Point:
                        coords: tuple[int, ...]
                "# => "tuple[int, ...]",
                bare_tuple_attribute => r#"
                    class Point:
                        coords: tuple
                "# => "tuple",
                frozenset_attribute => r#"
                    class Config:
                        tags: frozenset[str]
                "# => "frozenset[str]",
            ],
        },
    }
);
