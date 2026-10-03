//! Flags Python function parameters annotated with `Sequence` or `Collection` when a broader `Collection` or `Iterable` interface suffices (`specific-collection-parameter`).

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    ParameterCollectionCapability, PythonParameterKind, analyze_parameter_collection_capability,
    collect_specific_collection_types, extract_function_signatures,
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
    summary: "Parameter `{name}` of `{function}` has annotation `{expression}`, but `{function}` appears to need only `{replacement}` operations on `{name}`.",
    rationale: "Requiring a narrower collection interface than `{function}` uses rejects compatible inputs, such as sets and dictionary views where a `Collection` suffices or generators where an `Iterable` suffices, unless callers first copy them into a sequence.",
    suggestion: "Replace `{token}` in `{name}` with `{replacement}` unless the narrower interface is a deliberate part of the contract of `{function}`.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("specific-collection-parameter"),
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
            summary: "Flags Python function parameters annotated with `Sequence` or `Collection` when the body only uses `Collection` or `Iterable` operations.",
            what_it_does: "Flags non-variadic parameters of functions and methods in Python \
                           source files (test files are not checked) whose type annotation uses \
                           `Sequence` or `Collection` (at the top level or inside transparent \
                           `|`, `Optional`, `Union`, or `Annotated` wrappers) when every use of \
                           the parameter inside the function body is satisfied by a broader \
                           abstract interface:\n\
                           - Suggests `Iterable` when the parameter is only iterated once at \
                             top-level depth (`for x in param`, a single comprehension, or a \
                             single iterable builtin such as `sum`, `min`, `max`, `any`, `all`, \
                             `sorted`, `list`, `tuple`, `set`, `frozenset`, `dict`, `enumerate`, \
                             `zip`, `iter`, `map`, or `filter`).\n\
                           - Suggests `Collection` (for `Sequence` parameters) when the parameter \
                             is checked for length (`len(param)`), membership (`v in param`), \
                             truthiness (`if param:`, `if not param:`, `bool(param)`), or \
                             iterated multiple times (because a single-pass `Iterable` generator \
                             is always truthy and exhausts on the first pass).\n\
                           Parameters that are indexed or sliced (`param[0]`), reversed \
                           (`reversed(param)`), queried via `.index()` or `.count()`, matched \
                           in a `match` statement, unused, or passed to another function or \
                           method are not flagged. Stub bodies, methods on `Protocol` or `ABC` \
                           classes, dunder methods other than `__init__`, `__new__`, and \
                           `__call__`, and functions decorated with `@override`, `@overload`, \
                           `@abstractmethod`, `@fixture`, `@<function>.register`, or \
                           `@<property>.setter` are exempt.",
            why_is_this_bad: "Annotating a parameter as `Sequence[T]` when the function only \
                              iterates over it once prevents callers from passing a `set[T]`, \
                              `dict.keys()`, `dict.values()`, or a generator expression without \
                              materializing an intermediate `list` or `tuple`.\n\n\
                              `Iterable[T]` states single-pass iteration and `Collection[T]` \
                              states `len()`, `in`, truthiness, or multi-pass iteration. Keep \
                              `Sequence[T]` only as a deliberate contract, for example to \
                              reserve indexing for a later version. The suggested interface \
                              comes from a syntactic analysis of the function body.",
            references: &[Reference {
                title: "Python collections.abc — Collections Abstract Base Classes",
                url: "https://docs.python.org/3/library/collections.abc.html",
            }],
            examples: &[Example {
                language: SupportLang::Python,
                flagged: indoc::indoc! {r"
                    from collections.abc import Sequence

                    def total_price(prices: Sequence[float]) -> float:
                        return sum(prices)
                "},
                flagged_span: "Sequence[float]",
                fixed: indoc::indoc! {r"
                    from collections.abc import Iterable

                    def total_price(prices: Iterable[float]) -> float:
                        return sum(prices)
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
            let matched = collect_specific_collection_types(type_node);
            if matched.is_empty() {
                continue;
            }
            let capability =
                analyze_parameter_collection_capability(&signature.node, &parameter.name);
            let replacement = match capability {
                ParameterCollectionCapability::Unused | ParameterCollectionCapability::Sequence => {
                    continue;
                }
                ParameterCollectionCapability::Collection => {
                    let annotates_sequence = matched
                        .iter()
                        .any(|type_path| type_path.rsplit('.').next() == Some("Sequence"));
                    if !annotates_sequence {
                        continue;
                    }
                    "collections.abc.Collection"
                }
                ParameterCollectionCapability::Iterable => "collections.abc.Iterable",
            };
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
                    ("replacement", replacement),
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
                collection_parameter_with_len => r"
                    from collections.abc import Collection

                    def size(items: Collection[int]) -> int:
                        return len(items)
                ",

                collection_parameter_with_membership => r"
                    from collections.abc import Collection

                    def has_one(items: Collection[int]) -> bool:
                        return 1 in items
                ",

                collection_parameter_with_truthiness_check => r"
                    from collections.abc import Collection

                    def total_or_zero(items: Collection[int]) -> int:
                        if not items:
                            return 0
                        return sum(items)
                ",

                collection_parameter_with_multiple_passes => r"
                    from collections.abc import Collection

                    def span(items: Collection[int]) -> int:
                        return max(items) - min(items)
                ",

                // Iterable and Collection are not Reversible.
                reversed_requires_sequence => r"
                    from collections.abc import Sequence

                    def backwards(items: Sequence[int]) -> list[int]:
                        return list(reversed(items))
                ",

                index_and_count_require_sequence => r"
                    from collections.abc import Sequence

                    def occurrences(items: Sequence[int]) -> int:
                        return items.count(0) + items.index(1)
                ",

                indexing_requires_sequence => r"
                    from collections.abc import Sequence

                    def head(items: Sequence[int]) -> int:
                        return items[0]
                ",

                sequence_match_pattern_requires_sequence => r"
                    from collections.abc import Sequence

                    def first_or_zero(items: Sequence[int]) -> int:
                        match items:
                            case [first, *_rest]:
                                return first
                            case _:
                                return 0
                ",

                passed_to_method_exempt => r#"
                    from collections.abc import Sequence

                    def forward(items: Sequence[str]) -> str:
                        return ", ".join(items)
                "#,

                passed_to_unknown_function_exempt => r"
                    from collections.abc import Sequence

                    def delegate(items: Sequence[int]) -> int:
                        return helper(items)
                ",

                explained_sequence_parameter => r"
                    from collections.abc import Sequence

                    # Sequence required to preserve deterministic ordering of layers.
                    def build(layers: Sequence[str]) -> list[str]:
                        return [layer.strip() for layer in layers]
                ",

                unused_parameter_not_flagged => r"
                    from collections.abc import Sequence

                    def unused_param(items: Sequence[int]) -> int:
                        return 42
                ",

                protocol_class_exempt => r"
                    from typing import Protocol
                    from collections.abc import Sequence

                    class Runner(Protocol):
                        def run(self, items: Sequence[int]) -> int:
                            return sum(items)
                ",

                abc_class_exempt => r"
                    import abc
                    from collections.abc import Sequence

                    class BaseRunner(abc.ABC):
                        def run(self, items: Sequence[int]) -> int:
                            return sum(items)
                ",

                override_exempt => r"
                    from typing import override
                    from collections.abc import Sequence

                    class ConcreteRunner(Base):
                        @override
                        def run(self, items: Sequence[int]) -> int:
                            return sum(items)
                ",

                overload_exempt => r"
                    from typing import overload
                    from collections.abc import Sequence

                    @overload
                    def run(items: Sequence[int]) -> int:
                        return sum(items)
                ",

                abstractmethod_exempt => r"
                    from abc import abstractmethod
                    from collections.abc import Sequence

                    class Runner:
                        @abstractmethod
                        def run(self, items: Sequence[int]) -> int:
                            return sum(items)
                ",

                data_model_dunder_exempt => r"
                    from collections.abc import Sequence

                    class Vector:
                        def __eq__(self, other: Sequence[int]) -> bool:
                            return sum(other) == 0
                ",
            ],
            fail: [
                single_loop_on_sequence_suggests_iterable => r"
                    from collections.abc import Sequence

                    def sum_loop(items: Sequence[int]) -> int:
                        total = 0
                        for item in items:
                            total += item
                        return total
                " => "Sequence[int]",

                single_comprehension_on_collection_suggests_iterable => r"
                    from collections.abc import Collection

                    def double_all(items: Collection[int]) -> list[int]:
                        return [value * 2 for value in items]
                " => "Collection[int]",

                single_pass_iterable_builtin_suggests_iterable => r"
                    from collections.abc import Sequence

                    def total(prices: Sequence[float]) -> float:
                        return sum(prices)
                " => "Sequence[float]",

                len_and_contains_on_sequence_suggest_collection => r"
                    from collections.abc import Sequence

                    def size_if_present(items: Sequence[int]) -> int:
                        if 1 in items:
                            return len(items)
                        return 0
                " => "Sequence[int]",

                // A truthiness check needs `__len__`, so never Iterable.
                truthiness_plus_iteration_suggests_collection => r"
                    from collections.abc import Sequence

                    def average_or_zero(prices: Sequence[float]) -> float:
                        if not prices:
                            return 0.0
                        return sum(prices)
                " => "Sequence[float]",

                // A second pass would exhaust a one-shot iterator, so never Iterable.
                multipass_two_consumers_suggests_collection => r"
                    from collections.abc import Sequence

                    def span(items: Sequence[int]) -> int:
                        return max(items) - min(items)
                " => "Sequence[int]",

                nested_inner_loop_suggests_collection => r"
                    from collections.abc import Iterable, Sequence

                    def nested_loop(items: Sequence[int], rows: Iterable[int]) -> int:
                        total = 0
                        for row in rows:
                            for value in items:
                                total += row * value
                        return total
                " => "Sequence[int]",

                qualified_typing_sequence_suggests_collection => r"
                    import typing

                    def size(items: typing.Sequence[int]) -> int:
                        return len(items)
                " => "typing.Sequence[int]",
            ]
        }
    }
);
