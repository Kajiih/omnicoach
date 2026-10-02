//! Flags Python function return annotations that use concrete mutable collection types without an explanation (`concrete-collection-return`).

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    PythonFunctionSignature, collect_concrete_collection_types,
    collect_locally_mutated_return_functions, extract_function_signatures,
    has_exempt_signature_decorator, is_exempt_dunder_method, is_in_protocol_or_abc_class,
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
    summary: "Return annotation of `{function}` uses concrete collection type `{expression}` (`{token}`).",
    rationale: "Returning an invariant concrete collection type such as `list`, `dict`, or `set` exposes mutability across the boundary and forces callers holding a `Sequence` or `Mapping` to copy before returning.",
    suggestion: "Replace `{token}` in the return annotation of `{function}` with `Sequence`, `Mapping`, or `Set` from `collections.abc` (or `tuple` / `frozenset`), or add a comment on `{function}` explaining why callers need a mutable collection.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("concrete-collection-return"),
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
            summary: "Flags Python function return annotations using concrete mutable collection types such as `list`, `dict`, or `set` without an explanation.",
            what_it_does: "Flags functions and methods in Python source files (test files are \
                           not checked) whose return annotation uses a concrete mutable \
                           collection constructor (`list`, `dict`, `set`, `typing.List`, \
                           `typing.Dict`, or `typing.Set`), either at the top level or inside \
                           transparent wrappers (`|`, `Optional`, `Union`, `Annotated`) and \
                           covariant container positions (`Sequence[list[T]]`, \
                           `Mapping[K, list[V]]`, `Awaitable[list[T]]`). Functions whose return \
                           value is mutated in place by a caller in the same file, or whose \
                           header carries a substantive explanation comment (under the default \
                           `require-explanation` mode), are not flagged. Dunder methods other \
                           than `__init__` and `__new__`, methods on `Protocol` or `ABC` \
                           classes, and functions decorated with `@override`, `@overload`, \
                           `@abstractmethod`, or `@fixture` are exempt.",
            why_is_this_bad: "Returning a concrete `list`, `dict`, or `set` exposes internal \
                              state to in-place caller mutation and locks the implementation \
                              into returning an invariant mutable container even when it could \
                              otherwise return a cached `tuple`, a `Sequence` view, or a \
                              parameter directly without copying.\n\n\
                              Annotate read-only return values with `Sequence`, `Mapping`, or \
                              `Set` from `collections.abc` (or `tuple` / `frozenset`). When a \
                              function intentionally returns a fresh mutable buffer for callers \
                              to mutate in place, document that contract in a comment on the \
                              function header.",
            references: &[Reference {
                title: "PEP 585: Type Hinting Generics In Standard Collections",
                url: "https://peps.python.org/pep-0585/",
            }],
            examples: &[Example {
                language: SupportLang::Python,
                flagged: indoc::indoc! {r"
                    def active_tags(self) -> list[str]:
                        return self._tags
                "},
                flagged_span: "list[str]",
                fixed: indoc::indoc! {r"
                    from collections.abc import Sequence

                    def active_tags(self) -> Sequence[str]:
                        return self._tags
                "},
            }],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

fn is_exempt_function(signature: &PythonFunctionSignature<'_>) -> bool {
    is_exempt_dunder_method(&signature.name)
        || has_exempt_signature_decorator(&signature.node)
        || is_in_protocol_or_abc_class(&signature.node)
}

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    let locally_mutated = collect_locally_mutated_return_functions(file);
    let mut diagnostics = Vec::new();

    for signature in extract_function_signatures(file) {
        if is_exempt_function(&signature) || locally_mutated.contains(&signature.name) {
            continue;
        }
        let Some(ref return_type_node) = signature.return_type_node else {
            continue;
        };
        let matched = collect_concrete_collection_types(return_type_node);
        if matched.is_empty() {
            continue;
        }
        let token = matched.join(", ");
        let expression = return_type_node.text();
        diagnostics.push(rule.diagnostic_at_node(
            path,
            return_type_node,
            &[
                ("function", &signature.name),
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
                abstract_return_types => r#"
                    from collections.abc import Iterable, Mapping, Sequence, Set

                    def get_users() -> Sequence[str]:
                        return ["alice"]

                    def get_counts() -> Mapping[str, int]:
                        return {"a": 1}

                    def get_tags() -> Set[str]:
                        return {"v1"}

                    def stream_ids() -> Iterable[int]:
                        return [1, 2]
                "#,
                immutable_concrete_return_types => r#"
                    def get_coords() -> tuple[int, ...]:
                        return (1, 2)

                    def get_flags() -> frozenset[str]:
                        return frozenset()
                "#,
                explained_by_preceding_header_comment => r#"
                    # Callers sort and append to the returned buffer in place.
                    def make_buffer() -> list[str]:
                        return []
                "#,
                explained_on_decorated_function_header => r#"
                    class Builder:
                        # Callers mutate the returned list directly to assemble batches.
                        @staticmethod
                        def make_batch() -> list[int]:
                            return []
                "#,
                locally_mutated_via_variable_binding => r#"
                    def make_buf() -> list[int]:
                        return []

                    def caller() -> None:
                        buf = make_buf()
                        buf.append(1)
                "#,
                locally_mutated_directly_on_call => r#"
                    def make_map() -> dict[str, int]:
                        return {}

                    def caller() -> None:
                        make_map()["k"] = 1
                "#,
                override_overload_abstract_protocol_dunder_exempt => r#"
                    import abc
                    from typing import Protocol, overload, override

                    class P(Protocol):
                        def items(self) -> list[str]: ...

                    class Base(abc.ABC):
                        @abc.abstractmethod
                        def keys(self) -> set[str]:
                            pass

                    class Impl(Base):
                        @override
                        def keys(self) -> set[str]:
                            return set()

                        def __dir__(self) -> list[str]:
                            return []

                    @overload
                    def fetch(x: int) -> list[int]: ...
                "#,
            ],
            fail: [
                unexplained_concrete_list_return => r#"
                    def get_users() -> list[str]:
                        return ["alice"]
                "# => "list[str]",
                union_and_optional_concrete_return => r#"
                    from typing import Optional

                    def get_config() -> Optional[dict[str, int]]:
                        return None
                "# => "Optional[dict[str, int]]",
                covariant_nested_awaitable_return => r#"
                    from collections.abc import Awaitable

                    def fetch_tags() -> Awaitable[set[str]]:
                        raise NotImplementedError
                "# => "Awaitable[set[str]]",
                body_comment_does_not_count_as_header_explanation => r#"
                    @staticmethod
                    def get_items() -> list[str]:
                        # Internal implementation comment inside the body block.
                        return ["a"]
                "# => "list[str]",
            ],
        },
    }
);
