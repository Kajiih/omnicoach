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
    summary: "Parameter `{name}` of `{function}` is annotated with `{expression}` (`{token}`) but only uses `{class}` operations.",
    rationale: "Requiring a narrower collection interface than `{function}` uses restricts callers from passing compatible inputs such as sets, dictionary views, or lazy iterables without materializing a sequence.",
    suggestion: "Replace `{token}` in `{name}` with `{class}` from `collections.abc`, or add a comment on `{function}` explaining why the narrower interface is part of the contract.",
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
                             `sorted`, `list`, `tuple`, `set`, `dict`, `enumerate`, or `zip`).\n\
                           - Suggests `Collection` (for `Sequence` parameters) when the parameter \
                             is checked for length (`len(param)`), membership (`v in param`), \
                             truthiness (`if param:`, `if not param:`, `bool(param)`), or \
                             iterated multiple times (because a single-pass `Iterable` generator \
                             is always truthy and exhausts on the first pass).\n\
                           Parameters that are indexed or sliced (`param[0]`), reversed \
                           (`reversed(param)`), queried via `.index()` or `.count()`, matched \
                           in a `match` statement, unused, or passed to another function or \
                           method are not flagged. Defaults to `require-explanation` mode so \
                           intentional `Sequence` contracts can be documented with a comment.",
            why_is_this_bad: "Annotating a parameter as `Sequence[T]` when the function only \
                              iterates over it once prevents callers from passing a `set[T]`, \
                              `dict.keys()`, `dict.values()`, or a generator expression without \
                              materializing an intermediate `list` or `tuple`.\n\n\
                              Use `Iterable[T]` for single-pass iteration, `Collection[T]` when \
                              `len()`, `in`, truthiness, or multi-pass iteration is required, or \
                              add a comment explaining why `Sequence[T]` is part of the API contract.",
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
            let suggested_class = match capability {
                ParameterCollectionCapability::Unused | ParameterCollectionCapability::Sequence => {
                    continue;
                }
                ParameterCollectionCapability::Collection => {
                    if !matched
                        .iter()
                        .any(|type_name| type_name.as_str() == "Sequence")
                    {
                        continue;
                    }
                    "Collection"
                }
                ParameterCollectionCapability::Iterable => "Iterable",
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
                    ("class", suggested_class),
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
                // F3/F4/F5: Collection parameter that uses Collection operations passes
                f3_f4_f5_collection_parameter_using_collection_ops => r"
                    from collections.abc import Collection

                    def count_matches(items: Collection[int]) -> int:
                        if not items:
                            return 0
                        if 1 in items:
                            return len(items)
                        return min(items) + max(items)
                ",

                // F6: reversed(x) requires Sequence (Iterable and Collection are not Reversible)
                f6_reversed_requires_sequence => r"
                    from collections.abc import Sequence

                    def backwards(items: Sequence[int]) -> list[int]:
                        return list(reversed(items))
                ",

                // F7: .index(v) and .count(v) require Sequence
                f7_index_and_count_require_sequence => r"
                    from collections.abc import Sequence

                    def occurrences(items: Sequence[int]) -> int:
                        return items.count(0) + items.index(1)
                ",

                // F8: Indexing or slicing requires Sequence
                f8_indexing_and_slicing_require_sequence => r"
                    from collections.abc import Sequence

                    def head(items: Sequence[int]) -> int:
                        return items[0]
                ",

                // F9: Sequence match pattern requires Sequence
                f9_sequence_match_pattern => r"
                    from collections.abc import Sequence

                    def first_or_zero(items: Sequence[int]) -> int:
                        match items:
                            case [first, *_rest]:
                                return first
                            case _:
                                return 0
                ",

                // F10: Helper forwarding / escape exempts parameter
                f10_helper_forwarding_and_escape_exempt => r#"
                    from collections.abc import Sequence

                    def forward(items: Sequence[str]) -> str:
                        return ", ".join(items)

                    def delegate(items: Sequence[int]) -> int:
                        return helper(items)
                "#,

                // F11: Explained Sequence / Collection parameter under require-explanation mode
                f11_explained_sequence_parameter_passes => r"
                    from collections.abc import Sequence

                    # Sequence required to preserve deterministic ordering of layers.
                    def build(layers: Sequence[str]) -> list[str]:
                        return [layer.strip() for layer in layers]
                ",

                // F12: Unused parameter, stub body, @override, @overload, Protocol, ABC, @fixture, dunders
                f12_unused_stub_and_contract_exemptions => r"
                    from typing import Protocol, override
                    from collections.abc import Sequence

                    def unused_param(items: Sequence[int]) -> int:
                        return 42

                    def stub_func(items: Sequence[int]) -> None: ...

                    class Runner(Protocol):
                        def run(self, items: Sequence[int]) -> int: ...

                    class ConcreteRunner:
                        @override
                        def run(self, items: Sequence[int]) -> int:
                            return sum(items)
                ",
            ],
            fail: [
                // F1: Single for loop or single comprehension -> suggest Iterable
                f1_single_loop_on_sequence_suggests_iterable => r"
                    from collections.abc import Sequence

                    def sum_loop(items: Sequence[int]) -> int:
                        total = 0
                        for item in items:
                            total += item
                        return total
                " => "Sequence[int]",

                f1_single_comprehension_on_collection_suggests_iterable => r"
                    from collections.abc import Collection

                    def double_all(items: Collection[int]) -> list[int]:
                        return [value * 2 for value in items]
                " => "Collection[int]",

                // F2: Single call to iterable-consuming builtin -> suggest Iterable
                f2_single_pass_iterable_builtin_suggests_iterable => r"
                    from collections.abc import Sequence

                    def total(prices: Sequence[float]) -> float:
                        return sum(prices)
                " => "Sequence[float]",

                // F3: len(x) or v in x on Sequence -> suggest Collection
                f3_len_and_contains_on_sequence_suggest_collection => r"
                    from collections.abc import Sequence

                    def size_if_present(items: Sequence[int]) -> int:
                        if 1 in items:
                            return len(items)
                        return 0
                " => "Sequence[int]",

                // F4: Truthiness check + single iteration on Sequence -> suggest Collection (never Iterable!)
                f4_truthiness_plus_iteration_suggests_collection => r"
                    from collections.abc import Sequence

                    def average_or_zero(prices: Sequence[float]) -> float:
                        if not prices:
                            return 0.0
                        return sum(prices)
                " => "Sequence[float]",

                // F5: Multi-pass iteration (two consumers or nested loop) on Sequence -> suggest Collection (never Iterable!)
                f5_multipass_two_consumers_suggests_collection => r"
                    from collections.abc import Sequence

                    def span(items: Sequence[int]) -> int:
                        return max(items) - min(items)
                " => "Sequence[int]",

                f5_nested_inner_loop_suggests_collection => r"
                    from collections.abc import Iterable, Sequence

                    def nested_loop(items: Sequence[int], rows: Iterable[int]) -> int:
                        total = 0
                        for row in rows:
                            for value in items:
                                total += row * value
                        return total
                " => "Sequence[int]",
            ]
        }
    }
);
