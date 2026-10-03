//! Flags public Python class and instance attributes annotated with concrete mutable collection types without an explanation (`concrete-collection-attribute`).

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    collect_concrete_collection_types, collect_public_class_attributes,
    has_unaliased_collections_abc_set_import,
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
    summary: "Attribute `{name}` of `{class}` is annotated with concrete collection type `{expression}` (`{token}`).",
    rationale: "A public or dataclass attribute typed as `list`, `dict`, or `set` rejects `Sequence`, `Mapping`, or `tuple` arguments in synthesized constructors and exposes internal state to in-place caller mutation.",
    suggestion: "Replace `{token}` on `{name}` with `Sequence`, `Mapping`, or `AbstractSet` from `collections.abc` (or `MutableSequence`, `MutableMapping`, or `MutableSet` when mutated in `{class}`), prefix internal mutable state with `_`, or add a comment explaining why `{name}` uses a concrete collection.",
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
                           Private attributes starting with `_`, attributes on `Protocol` or \
                           `ABC` classes, and attributes with a substantive explanation comment \
                           (under the default `require-explanation` mode) are not flagged.",
            why_is_this_bad: "On a `@dataclass` or public class interface, annotating a field as \
                              `items: list[str]` forces callers constructing the class to pass a \
                              concrete `list` rather than a `tuple` or an upstream `Sequence[str]` \
                              parameter, and exposes a mutable container on the instance even \
                              when `@dataclass(frozen=True)` is used.\n\n\
                              Annotate read-only public fields with `Sequence`, `Mapping`, or \
                              `AbstractSet` (`from collections.abc import Set as AbstractSet`), \
                              and in-place mutated public fields with `MutableSequence`, \
                              `MutableMapping`, or `MutableSet`. If the attribute holds internal \
                              mutable state, prefix its name with `_`; if external callers \
                              intentionally require a concrete collection, document that in a \
                              comment on the attribute.",
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
        let expression = attribute.type_node.text();
        diagnostics.push(rule.diagnostic_at_node(
            path,
            &attribute.type_node,
            &[
                ("name", &attribute.name),
                ("class", &attribute.class_name),
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
                protocol_and_abc_classes_exempt => r#"
                    import abc
                    from typing import Protocol

                    class HasItems(Protocol):
                        items: list[str]

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
