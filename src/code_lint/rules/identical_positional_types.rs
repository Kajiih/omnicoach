//! Enforces keyword-only parameters when a function has multiple positional parameters of identical type (`identical-positional-types`).

use crate::code_lint::ast::python::{
    PythonFunctionSignature, PythonParameterInfo, extract_function_signatures, has_decorator,
};
use crate::code_lint::ast::{AstNode, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, CountOption, Declaration, Example, ImpactedQuality,
    LanguageDefaults, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::path::Path;

const MIN_POSITIONAL_PARAMETERS: CountOption = CountOption {
    key: "min-positional-parameters",
    doc: "Minimum positional parameters, excluding `self` and `cls`, for a function to be checked.",
    default: LanguageDefaults::new(3, &[]),
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Function `{function}` has several positional parameters of the same type ({duplicates}).",
    rationale: "When positional parameters share a type, a caller can swap the arguments (`transfer(target_id, source_id)`) and no type checker will notice.",
    suggestion: "Insert a keyword-only separator `*` in `{function}` (`def {function}(*, ...)`) so callers pass these arguments by name.",
};

/// The rule's declaration.
pub const RULE: CodeRule<CountOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("identical-positional-types"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python],
        options: RuleOptions::code_rule(MIN_POSITIONAL_PARAMETERS),
        classification: Classification {
            topics: &[Topic::STATIC_TYPING, Topic::POSITIONAL_MEANING],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags Python functions whose positional parameters share a type annotation.",
            what_it_does: "Flags a function in Python source files (test files are not \
                           checked) that has at least `min-positional-parameters` positional \
                           parameters of which two or more have the same type annotation. A \
                           leading `self` or `cls`, keyword-only parameters (after `*` or \
                           `*args`), `*args` and `**kwargs` are not counted. Annotations are \
                           compared as written, so `dict[str, int]` and `dict[str, float]` \
                           differ, and unannotated parameters count toward the minimum but never \
                           match each other. Dunder methods other than `__init__` and \
                           `__new__`, and functions decorated with `@override`, `@overload`, \
                           `@abstractmethod` or `@fixture`, are exempt because their signature \
                           is imposed from outside.",
            why_is_this_bad: "When two positional parameters have the same type, a call that \
                              swaps them, such as `transfer(target_id, source_id, amount)`, \
                              still type-checks and reads plausibly in review. The bug shows up \
                              only at runtime, often as wrong data rather than an error.\n\n\
                              Make the parameters keyword-only with a `*` separator, for \
                              example `def transfer(*, source_id: str, target_id: str, amount: \
                              int)`, so every call names its arguments. Distinct types (such as \
                              `NewType` wrappers) also let the type checker catch the swap.",
            references: &[Reference {
                title: "PEP 3102: Keyword-Only Arguments",
                url: "https://peps.python.org/pep-3102/",
            }],
            examples: &[Example {
                language: SupportLang::Python,
                flagged: indoc::indoc! {r"
                    def transfer(source_id: str, target_id: str, amount: int) -> None:
                        ledger.debit(source_id, amount)
                        ledger.credit(target_id, amount)
                "},
                flagged_span: "transfer",
                fixed: indoc::indoc! {r"
                    def transfer(*, source_id: str, target_id: str, amount: int) -> None:
                        ledger.debit(source_id, amount)
                        ledger.credit(target_id, amount)
                "},
            }],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

/// Returns true if `func_node` is a Python Data Model dunder method with a fixed runtime positional signature
/// (all `__*__` methods except constructors `__init__` and `__new__`).
fn is_exempt_dunder(func_name: &str) -> bool {
    func_name.starts_with("__")
        && func_name.ends_with("__")
        && func_name.len() > 4
        && !matches!(func_name, "__init__" | "__new__")
}

/// Returns true if `func_node` is decorated with `@override`, `@overload`, `@abstractmethod`, or `@fixture`.
fn has_exempt_decorator(func_node: &AstNode<'_>) -> bool {
    has_decorator(func_node, |terminal| {
        matches!(
            terminal,
            "override" | "overload" | "abstractmethod" | "fixture"
        )
    })
}

/// Groups typed positional parameters by type annotation and returns groups with `>= 2` parameters.
fn collect_duplicate_type_groups(
    params: &[&PythonParameterInfo<'_>],
) -> Vec<(String, Vec<String>)> {
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for param in params {
        if let Some(ref type_annotation) = param.type_text {
            if let Some((_, existing)) = groups
                .iter_mut()
                .find(|(seen_type, _)| seen_type == type_annotation)
            {
                existing.push(param.name.clone());
            } else {
                groups.push((type_annotation.clone(), vec![param.name.clone()]));
            }
        }
    }

    groups
        .into_iter()
        .filter(|(_, params)| params.len() >= 2)
        .collect()
}

/// Evaluates a single Python function signature and returns a consolidated diagnostic if violated.
fn check_function_signature(
    rule: &CodeRule<CountOption>,
    signature: &PythonFunctionSignature<'_>,
    path: &Path,
    min_args: usize,
) -> Option<Diagnostic> {
    let func_name = signature.name.as_str();

    if is_exempt_dunder(func_name) || has_exempt_decorator(&signature.node) {
        return None;
    }

    let positional_params: Vec<_> = signature
        .parameters
        .iter()
        .filter(|param| param.is_positional())
        .collect();

    if positional_params.len() < min_args {
        return None;
    }

    let duplicate_groups = collect_duplicate_type_groups(&positional_params);
    if duplicate_groups.is_empty() {
        return None;
    }

    let duplicates = duplicate_groups
        .iter()
        .map(|(type_annotation, params)| format!("`{}: {type_annotation}`", params.join(", ")))
        .collect::<Vec<_>>()
        .join(", ");

    Some(rule.diagnostic_at_node(
        path,
        &signature.name_node,
        &[("function", func_name), ("duplicates", &duplicates)],
    ))
}

fn check_file(
    rule: &CodeRule<CountOption>,
    path: &Path,
    file: &ParsedFile,
    min_args: usize,
) -> Vec<Diagnostic> {
    extract_function_signatures(file)
        .iter()
        .filter_map(|signature| check_function_signature(rule, signature, path, min_args))
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                fewer_than_min_args => r#"
                    def add(a: int, b: int) -> int:
                        return a + b
                "#,
                keyword_only_separator => r#"
                    def safe_transfer(source_id: str, *, target_id: str, amount: int, fee: int) -> None:
                        pass
                "#,
                after_varargs_is_keyword_only => r#"
                    def vararg_fn(first: str, *args: int, second: str, third: str) -> None:
                        pass
                "#,
                method_self_excluded_below_min => r#"
                    class AccountService:
                        def compute(self, a: int, b: int) -> int:
                            return a + b
                "#,
                classmethod_cls_excluded_below_min => r#"
                    class AccountService:
                        @classmethod
                        def from_pair(cls, a: int, b: int) -> "AccountService":
                            return cls()
                "#,
                var_keyword_excluded => r#"
                    def dispatch(channel: str, retries: int, delay: float, **metadata: str) -> None:
                        pass
                "#,
                untyped_parameters_not_grouped => r#"
                    def process(raw_a, raw_b, user_id: str, count: int) -> None:
                        pass
                "#,
                dunder_method_exempt => r#"
                    class AccountService:
                        def __exit__(self, exc_type: object, exc_val: object, exc_tb: object) -> None:
                            pass
                "#,
                decorator_overload_exempt => r#"
                    from typing import overload

                    @overload
                    def overloaded_fn(a: str, b: str, c: int) -> int: ...
                "#,
                decorator_fixture_exempt => r#"
                    import pytest

                    @pytest.fixture
                    def sample_orders(first: str, second: str, third: int) -> None:
                        pass
                "#,
                decorator_override_exempt => r#"
                    from typing import override

                    class AccountService:
                        @override
                        def sync(self, primary: str, secondary: str, retries: int) -> None:
                            pass
                "#,
                decorator_abstractmethod_exempt => r#"
                    from abc import abstractmethod

                    class AccountService:
                        @abstractmethod
                        def reconcile(self, source: str, target: str, limit: int) -> None:
                            pass
                "#,
                distinct_positional_types => r#"
                    def process(user_id: str, count: int, ratio: float) -> None:
                        pass
                "#,
            ],
            fail: [
                single_duplicate_type_group => r#"
                    def transfer(source_id: str, target_id: str, amount: int) -> None:
                        pass
                "# => "transfer",
                multiple_duplicate_type_groups => r#"
                    def transfer(source_id: str, target_id: str, amount: int, fee: int) -> None:
                        pass
                "# => "transfer",
                non_adjacent_duplicate_types_with_default => r#"
                    def create_item(owner_id: str, weight: float, item_id: str = "default") -> None:
                        pass
                "# => "create_item",
                new_dunder_flagged => r#"
                    class AccountService:
                        def __new__(cls, host: str, port: int, api_key: str):
                            return super().__new__(cls)
                "# => "__new__",
                init_dunder_flagged => r#"
                    class AccountService:
                        def __init__(self, host: str, port: int, api_key: str) -> None:
                            pass
                "# => "__init__",
            ],
        },
    }
);
