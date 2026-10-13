//! Flags functions named like a yes/no question (`is_*`, `has_*`, …) that do not return a boolean.
//!
//! Design: `docs/dev/return_contracts/03_design.md`.

use crate::code_lint::ast::{self, ParsedFile, ReturnShape};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Function `{function}` is named as a predicate but returns `{expression}`.",
    rationale: {
        Python => "Callers test a predicate with `if`, so a `None`, `0` or empty result reads as `False`, and `bool | None` merges its third state into `False`.",
        Rust => "A name such as `is_*` or `has_*` promises a yes or no answer, so callers misread a returned value, a third `None` state or a missing result.",
    },
    suggestion: {
        Python => "Narrow the return type of `{function}` to `bool` (or `TypeGuard`), or rename it after what it returns: a noun or `find_*` for a looked-up value, `validate_*` when it raises instead of answering.",
        Rust => "Narrow the return type of `{function}` to `bool` (or `Result<bool, E>` when the check can fail), or rename it after what it returns: a noun or `find_*` for a looked-up value, `validate_*` when it returns `Result<(), E>`.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("non-boolean-predicate"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::NAMING],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags functions named like a yes/no question that do not return a boolean.",
            what_it_does: indoc::indoc! {r"
                Flags a function or method whose name, without leading underscores, starts with
                `is_`, `has_`, `can_`, `should_`, `supports_`, `allows_`, `requires_`, `needs_`,
                `contains_`, `starts_with_`, `ends_with_` or `matches_`, or is exactly `contains`,
                `starts_with`, `ends_with` or `matches`, when its return type is not a boolean.

                Booleans are `bool`, Python `TypeGuard[T]` / `TypeIs[T]`, and Rust
                `Result<bool, E>` (also through aliases such as `io::Result`) and `Poll<bool>`.
                `Option<bool>` and `bool | None` are flagged. A missing return type is flagged as
                returning nothing (`()` in Rust; in Python, when the body never returns a value),
                but an unannotated Python function that returns a value is not checked.
                Signatures imposed from outside are skipped: Python `@override`, `@overload`,
                `@abstractmethod`, data-model dunders and `Protocol` / `ABC` members, and Rust trait
                items and trait implementations."},
            why_is_this_bad: indoc::indoc! {r"
                A predicate name tells callers to write `if is_active(id):`. When the function
                returns a value instead (`has_report() -> Option<u32>`), the caller either discards
                it or, in Python, treats a `0` id or an empty result as `False`. A three-state
                `Option<bool>` / `bool | None` hides its third state behind a yes/no name: `if not
                x` treats `None` as `False`. A predicate that returns nothing and raises on failure
                is a validator, and `if is_valid(x):` is then always false.

                Return `bool` when callers only need the answer, keeping `Result<bool, E>` in Rust
                when the check can fail. Otherwise name the function after what it returns:
                `report_id`, `find_report`, `activation_age`, an enum such as `override_kind()` for
                three states, or `validate_*` for a check that fails loudly."},
            known_problems: Some(indoc::indoc! {r"
                - Type aliases of `bool` and other boolean-like types (`numpy.bool_`) are flagged.
                - Any Rust type whose last segment ends in `Result` is treated as a `Result`."}),
            references: &[
                Reference {
                    title: "Rust standard library: `std::fs::exists` returns `io::Result<bool>`",
                    url: "https://doc.rust-lang.org/std/fs/fn.exists.html",
                },
                Reference {
                    title: "PEP 647: User-Defined Type Guards",
                    url: "https://peps.python.org/pep-0647/",
                },
                Reference::NAME_RESOLUTION,
            ],
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r"
                        def has_report(reports: dict[str, int], name: str) -> int | None:
                            return reports.get(name)
                    "},
                    flagged_span: "has_report",
                    fixed: indoc::indoc! {r"
                        def find_report_id(reports: dict[str, int], name: str) -> int | None:
                            return reports.get(name)
                    "},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r"
                        fn has_report(reports: &HashMap<String, u32>, name: &str) -> Option<u32> {
                            reports.get(name).copied()
                        }
                    "},
                    flagged_span: "has_report",
                    fixed: indoc::indoc! {r"
                        fn report_id(reports: &HashMap<String, u32>, name: &str) -> Option<u32> {
                            reports.get(name).copied()
                        }
                    "},
                },
            ],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

const FUNCTION: &str = "function";
const EXPRESSION: &str = "expression";

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    ast::collect_function_returns(file)
        .into_iter()
        .filter(|function| {
            !matches!(function.shape, ReturnShape::Boolean | ReturnShape::Unknown)
                && ast::is_predicate_name(&function.name)
        })
        .map(|function| {
            rule.diagnostic_at_node(
                path,
                &function.name_node,
                &[
                    (FUNCTION, &function.name),
                    (EXPRESSION, &function.declared_type_text()),
                ],
            )
        })
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                boolean_predicates => r"
                    class Store:
                        def is_empty(self) -> bool:
                            return not self.items

                        def _has_key(self, key: str) -> bool:
                            return key in self.items

                        def contains(self, key: str) -> bool:
                            return key in self.items

                        async def can_connect(self) -> bool:
                            return True
                ",
                type_guard_exempt => r"
                    from typing import TypeGuard, TypeIs

                    def is_str_list(values: list[object]) -> TypeGuard[list[str]]:
                        return all(isinstance(value, str) for value in values)

                    def is_int(value: object) -> TypeIs[int]:
                        return isinstance(value, int)
                ",
                prefix_without_separator_exempt => r"
                    def island(name: str) -> str:
                        return name

                    def hash_key(name: str) -> int:
                        return hash(name)

                    def requires(package: str) -> list[str]:
                        return [package]
                ",
                unannotated_value_return_exempt => r"
                    def is_ready(job):
                        return job.state
                ",
                stub_body_exempt => r"
                    def is_ready(job):
                        ...
                ",
                imposed_signature_exempt => r"
                    from typing import Protocol, override

                    class Job(BaseJob):
                        @override
                        def is_ready(self) -> int:
                            return 1

                    class Readiness(Protocol):
                        def is_ready(self) -> int: ...
                ",
            ],
            fail: [
                optional_value_return => r"
                    def has_report(reports: dict[str, int], name: str) -> int | None:
                        return reports.get(name)
                " => "has_report",
                three_state_boolean => r"
                    from typing import Optional

                    def is_implicit_override(source: str) -> Optional[bool]:
                        return None
                " => "is_implicit_override",
                validator_annotated_none => r"
                    def is_valid(value: int) -> None:
                        if value < 0:
                            raise ValueError(value)
                " => "is_valid",
                validator_unannotated => r"
                    def is_valid(value):
                        if value < 0:
                            raise ValueError(value)
                " => "is_valid",
                private_predicate_returning_value => r"
                    class Resolver:
                        def _is_active(self, package: str) -> str:
                            return package
                " => "_is_active",
                verb_predicate_returning_count => r"
                    def contains(text: str, word: str) -> int:
                        return text.count(word)
                " => "contains",
            ],
        },
        Rust => {
            pass: [
                boolean_predicates => r"
                    use std::task::Poll;

                    fn is_empty(items: &[u32]) -> bool {
                        items.is_empty()
                    }

                    fn has_key(path: &str) -> std::io::Result<bool> {
                        std::fs::exists(path)
                    }

                    fn should_run(tracker: &Tracker) -> CargoResult<bool> {
                        tracker.due()
                    }

                    fn is_ready(&self) -> Poll<bool> {
                        Poll::Ready(true)
                    }
                ",
                prefix_without_separator_exempt => r"
                    fn island() -> u8 {
                        0
                    }

                    fn hash_key(name: &str) -> u64 {
                        name.len() as u64
                    }

                    fn requires() -> Vec<String> {
                        Vec::new()
                    }
                ",
                trait_contract_exempt => r"
                    trait Activity {
                        fn is_active(&self) -> Option<u32>;
                    }

                    impl Activity for Package {
                        fn is_active(&self) -> Option<u32> {
                            None
                        }
                    }
                ",
                inline_test_exempt => r"
                    #[test]
                    fn is_valid_rejects_negative_numbers() {
                        assert!(true);
                    }

                    #[cfg(test)]
                    mod checks {
                        fn has_fixture() -> Option<u32> {
                            None
                        }
                    }
                ",
            ],
            fail: [
                optional_value_return => r"
                    fn has_report(&self, name: &str) -> Option<u32> {
                        self.reports.get(name).copied()
                    }
                " => "has_report",
                three_state_boolean => r"
                    fn is_implicit_override(&self) -> Option<bool> {
                        None
                    }
                " => "is_implicit_override",
                validator_returning_nothing => r"
                    fn is_valid(value: i32) {
                        assert!(value >= 0);
                    }
                " => "is_valid",
                validator_returning_unit_result => r"
                    fn is_valid(value: i32) -> Result<(), String> {
                        if value < 0 { Err(value.to_string()) } else { Ok(()) }
                    }
                " => "is_valid",
            ],
        },
    }
);
