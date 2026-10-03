//! Flags Python function parameters annotated with abstract mutable collection types that are never mutated (`mutable-collection-parameter`).

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    AnnotationTraversalDepth, PythonParameterKind, collect_mutable_collection_types,
    extract_function_signatures, is_parameter_mutated_or_escaping,
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
    summary: "Parameter `{name}` of `{function}` is annotated with mutable collection type `{expression}` (`{token}`) but never mutated in `{function}`.",
    rationale: "Annotating a read-only parameter as `MutableSequence`, `MutableMapping`, or `MutableSet` makes its type invariant and prevents callers from passing immutable collections such as `tuple` or `MappingProxyType`.",
    suggestion: "Replace `{token}` in `{name}` with its read-only counterpart from `collections.abc` (`Sequence`, `Mapping`, or `AbstractSet`).",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("mutable-collection-parameter"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::STATIC_TYPING],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags Python function parameters annotated with `MutableSequence`, `MutableMapping`, or `MutableSet` when the function never mutates them.",
            what_it_does: "Flags non-variadic parameters of functions and methods in Python \
                           source files (test files are not checked) whose type annotation uses \
                           `MutableSequence`, `MutableMapping`, or `MutableSet` (at the top level \
                           or inside transparent `|`, `Optional`, `Union`, or `Annotated` \
                           wrappers) when the parameter is only read inside the function body. \
                           Parameters mutated in place (`append`, `extend`, `update`, `add`, \
                           subscript writes, `del`, or augmented assignment), aliased, returned, \
                           yielded, or passed to an unknown function or method are not flagged. \
                           Stub bodies (`...`, `pass`, `raise NotImplementedError`), methods on \
                           `Protocol` or `ABC` classes, dunder methods other than `__init__` and \
                           `__new__`, and functions decorated with `@override`, `@overload`, \
                           `@abstractmethod`, or `@fixture` are exempt.",
            why_is_this_bad: "`MutableSequence`, `MutableMapping`, and `MutableSet` are \
                              invariant in their type arguments and require a mutable container \
                              at call sites. Requiring `MutableSequence[int]` when the function \
                              only iterates or indexes the parameter rejects `tuple[int, ...]`, \
                              `Sequence[int]`, and covariant subtypes (`list[bool]`), and \
                              misleads callers into expecting in-place mutation.\n\n\
                              Use `Sequence`, `Mapping`, or `AbstractSet` (`from collections.abc \
                              import Set as AbstractSet`) whenever the parameter is not mutated \
                              in place.",
            references: &[Reference {
                title: "Python collections.abc — Collections Abstract Base Classes",
                url: "https://docs.python.org/3/library/collections.abc.html",
            }],
            examples: &[Example {
                language: SupportLang::Python,
                flagged: indoc::indoc! {r"
                    from collections.abc import MutableMapping

                    def summarize(counts: MutableMapping[str, int]) -> int:
                        return sum(counts.values())
                "},
                flagged_span: "MutableMapping[str, int]",
                fixed: indoc::indoc! {r"
                    from collections.abc import Mapping

                    def summarize(counts: Mapping[str, int]) -> int:
                        return sum(counts.values())
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
        if signature.is_exempt_from_body_usage_rules() {
            continue;
        }
        for parameter in &signature.parameters {
            if parameter.is_variadic() || parameter.kind == PythonParameterKind::Receiver {
                continue;
            }
            let Some(ref type_node) = parameter.type_node else {
                continue;
            };
            let matched = collect_mutable_collection_types(
                type_node,
                AnnotationTraversalDepth::TransparentWrappersOnly,
            );
            if matched.is_empty() {
                continue;
            }
            if is_parameter_mutated_or_escaping(&signature.node, &parameter.name) {
                continue;
            }
            let token = matched.join(", ");
            let expression = type_node.text();
            diagnostics.push(rule.diagnostic_at_node(
                path,
                type_node,
                &[
                    ("name", &parameter.name),
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
                // E5: Mutating method calls on MutableSequence, MutableMapping, MutableSet
                e5_mutating_sequence_methods => r#"
                    from collections.abc import MutableSequence

                    def append_item(items: MutableSequence[int]) -> None:
                        items.append(1)

                    def sort_items(items: MutableSequence[int]) -> None:
                        items.sort()
                "#,

                e5_mutating_mapping_and_set_methods => r#"
                    from collections.abc import MutableMapping, MutableSet

                    def update_map(counts: MutableMapping[str, int], tags: MutableSet[str]) -> None:
                        counts.setdefault("total", 0)
                        tags.discard("draft")
                "#,

                // E6: Subscript or slice write
                e6_subscript_and_slice_write => r"
                    from collections.abc import MutableSequence

                    def overwrite(items: MutableSequence[int], other: MutableSequence[int]) -> None:
                        items[0] = 1
                        other[1:3] = [2, 3]
                ",

                // E7: Subscript augmented write
                e7_subscript_augmented_write => r"
                    from collections.abc import MutableMapping

                    def increment(counts: MutableMapping[str, int]) -> None:
                        counts['hits'] += 1
                ",

                // E8: Subscript deletion
                e8_subscript_deletion => r"
                    from collections.abc import MutableSequence

                    def drop_first(items: MutableSequence[int]) -> None:
                        del items[0]
                ",

                // E9: Augmented assignment on parameter
                e9_augmented_assignment_on_parameter => r"
                    from collections.abc import MutableSequence, MutableSet

                    def extend_in_place(items: MutableSequence[int], tags: MutableSet[str]) -> None:
                        items += [1]
                        tags |= {'ready'}
                ",

                // E11: Passed to unknown function or method
                e11_passed_to_unknown_callee => r"
                    from collections.abc import MutableSequence

                    def delegate(items: MutableSequence[int], target: list[int]) -> None:
                        helper(items)
                        target.extend(items)
                ",

                // E12: Stored in variable, attribute, or container
                e12_stored_in_attribute_or_container => r"
                    from collections.abc import MutableSequence

                    class Holder:
                        def save(self, items: MutableSequence[int]) -> None:
                            self.items = items
                ",

                // E13: Returned or yielded
                e13_returned_or_yielded => r"
                    from collections.abc import MutableSequence

                    def passthrough(items: MutableSequence[int]) -> MutableSequence[int]:
                        return items
                ",

                // E14: Mutated inside nested closure / function
                e14_mutated_inside_nested_closure => r"
                    from collections.abc import MutableSequence

                    def outer(items: MutableSequence[int]) -> None:
                        def inner() -> None:
                            items.append(1)
                        inner()
                ",

                // E16: Stub body (..., pass, raise NotImplementedError)
                e16_stub_bodies_exempt => r#"
                    from collections.abc import MutableSequence

                    def stub_ellipsis(items: MutableSequence[int]) -> None: ...

                    def stub_pass(items: MutableSequence[int]) -> None:
                        """Docstring."""
                        pass

                    def stub_not_implemented(items: MutableSequence[int]) -> None:
                        raise NotImplementedError("subclass must implement")
                "#,

                // E17: Protocol, ABC, @override, @overload, @abstractmethod
                e17_protocol_and_override_exempt => r"
                    from typing import Protocol, override
                    from collections.abc import MutableSequence

                    class Sink(Protocol):
                        def handle(self, items: MutableSequence[int]) -> None:
                            return None

                    class Impl:
                        @override
                        def handle(self, items: MutableSequence[int]) -> None:
                            print(len(items))
                ",
            ],
            fail: [
                // E1: Read-only iteration & indexing
                e1_readonly_iteration_and_indexing => r"
                    from collections.abc import MutableSequence

                    def first_plus_sum(items: MutableSequence[int]) -> int:
                        return sum(value for value in items) + items[0]
                " => "MutableSequence[int]",

                // E2: Read-only methods on Sequence, Mapping, and Set
                e2_readonly_sequence_methods => r"
                    from collections.abc import MutableSequence

                    def inspect_sequence(items: MutableSequence[int]) -> int:
                        return items.count(1) + items.index(2)
                " => "MutableSequence[int]",

                e2_readonly_mapping_methods => r#"
                    from collections.abc import MutableMapping

                    def inspect_mapping(counts: MutableMapping[str, int]) -> int:
                        return counts.get("a", 0) + len(counts.keys())
                "# => "MutableMapping[str, int]",

                e2_readonly_set_methods => r#"
                    from collections.abc import MutableSet

                    def inspect_set(tags: MutableSet[str]) -> bool:
                        return tags.isdisjoint({"skip"})
                "# => "MutableSet[str]",

                // E3: Non-mutating builtins
                e3_safe_readonly_builtins => r"
                    from collections.abc import MutableSequence

                    def stats(items: MutableSequence[int]) -> int:
                        return (
                            len(items)
                            + max(items)
                            + min(items)
                            + sum(items)
                            + len(sorted(items))
                            + len(list(items))
                            + len(tuple(items))
                            + int(bool(items))
                            + int(any(items))
                            + int(all(items))
                        )
                " => "MutableSequence[int]",

                // E4: enumerate, zip, reversed, iter
                e4_readonly_iteration_wrappers => r"
                    from collections.abc import MutableSequence

                    def weighted_total(items: MutableSequence[int]) -> int:
                        total = 0
                        for index, value in enumerate(reversed(items)):
                            total += index * value
                        return total
                " => "MutableSequence[int]",

                // E10: Nested element mutation (mutates x[0], not outer container x)
                e10_nested_element_mutation_flags_outer => r"
                    from collections.abc import MutableSequence

                    def append_to_first_row(rows: MutableSequence[list[int]]) -> None:
                        rows[0].append(1)
                " => "MutableSequence[list[int]]",

                // E15: Shadowed parameter in nested function
                e15_shadowed_inner_parameter_still_flags_outer => r"
                    from collections.abc import MutableSequence

                    def outer(items: MutableSequence[int]) -> int:
                        def inner(items: MutableSequence[int]) -> None:
                            items.append(1)
                        return len(items)
                " => "MutableSequence[int]",
            ]
        }
    }
);
