//! Flags public Python class and instance attributes annotated with concrete mutable collection types (`concrete-collection-attribute`).

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    collect_concrete_collection_types, collect_public_class_attributes,
    has_unaliased_collections_abc_set_import, read_only_collection_replacements,
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
    summary: "Attribute `{name}` of `{class}` has annotation `{expression}`, which contains concrete collection type `{token}`.",
    rationale: "A public attribute typed as `list`, `dict`, or `set` turns assigning a `tuple`, `Sequence`, or `Mapping` (including through a dataclass constructor) into a type error and exposes internal state to in-place caller mutation.",
    suggestion: "Replace `{token}` on `{name}` with the `collections.abc` type that states what users of `{class}` may do with it, such as `{replacement}` for read-only access or its `Mutable` counterpart for in-place mutation, or prefix internal state with `_`.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("concrete-collection-attribute"),
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
            summary: "Flags public Python class and instance attributes annotated with concrete mutable collection types such as `list`, `dict`, or `set`.",
            what_it_does: "Flags public (non-`_`-prefixed) class attributes (`items: list[str]`) \
                           and `__init__` instance attributes (`self.items: list[str]`) in \
                           Python source files (test files are not checked) whose type \
                           annotation uses a concrete mutable collection constructor (`list`, \
                           `dict`, `set`, `List`, `Dict`, `Set`, `typing.List`, `typing.Dict`, \
                           or `typing.Set`), including inside `ClassVar`, `Final`, `Optional`, \
                           `Union`, `|`, and covariant containers. Unqualified `Set` is exempt \
                           only when `from collections.abc import Set` is present in the file. \
                           Private attributes starting with `_` and attributes on `Protocol` or \
                           `ABC` classes are not flagged. In `require-explanation` mode, a \
                           comment on or above the attribute line excuses the finding.",
            why_is_this_bad: "On a `@dataclass` or public class interface, annotating a field as \
                              `items: list[str]` forces callers constructing the class to pass a \
                              concrete `list` rather than a `tuple` or an upstream `Sequence[str]` \
                              parameter, and exposes a mutable container on the instance even \
                              when `@dataclass(frozen=True)` is used.\n\n\
                              Choose the annotation as a contract: `Sequence`, `Mapping`, or `Set` \
                              (imported as `AbstractSet`) for read-only fields, and their \
                              `Mutable` counterparts for fields mutated in place. Internal mutable \
                              state belongs in a `_`-prefixed attribute. A public concrete \
                              collection is a deliberate exception. The message names the \
                              read-only counterpart of the flagged type; it does not check how \
                              the attribute is used.",
            references: &[Reference {
                title: "PEP 585: Type Hinting Generics In Standard Collections",
                url: "https://peps.python.org/pep-0585/",
            }],
            examples: &[Example {
                language: SupportLang::Python,
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
    let abc_set_imported = has_unaliased_collections_abc_set_import(file);
    let mut diagnostics = Vec::new();
    for attribute in collect_public_class_attributes(file) {
        let matched = collect_concrete_collection_types(&attribute.type_node, abc_set_imported);
        if matched.is_empty() {
            continue;
        }
        let token = matched.join(", ");
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
                abstract_and_immutable_attributes => r#"
                    from collections.abc import Mapping, MutableSequence, Sequence, Set as AbstractSet
                    from dataclasses import dataclass

                    @dataclass(frozen=True)
                    class Config:
                        hosts: Sequence[str]
                        limits: Mapping[str, int]
                        tags: AbstractSet[str]
                        buffer: MutableSequence[int]
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
                module_and_function_local_variables_ignored => r#"
                    MODULE_ITEMS: list[int] = []

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
            ],
        },
    }
);
