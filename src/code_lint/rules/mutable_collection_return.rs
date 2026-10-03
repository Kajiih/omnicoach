//! Flags Python function return annotations that use abstract mutable collection types without an explanation or local caller mutation (`mutable-collection-return`).

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    AnnotationTraversalDepth, collect_locally_mutated_return_functions,
    collect_mutable_collection_types, extract_function_signatures,
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
    summary: "Return annotation of `{function}` uses mutable collection type `{expression}` (`{token}`) without an explanation or local caller mutation.",
    rationale: "Returning `MutableSequence`, `MutableMapping`, or `MutableSet` exposes mutability across the boundary and prevents returning immutable collections (`tuple`, `MappingProxyType`) or read-only parameter views directly.",
    suggestion: "Replace `{token}` in the return annotation of `{function}` with `Sequence`, `Mapping`, or `AbstractSet` from `collections.abc`, or add a comment on `{function}` explaining why callers need a mutable collection.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("mutable-collection-return"),
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
            summary: "Flags Python function return annotations using `MutableSequence`, `MutableMapping`, or `MutableSet` without an explanation or local caller mutation.",
            what_it_does: "Flags functions and methods in Python source files (test files are \
                           not checked) whose return annotation uses `MutableSequence`, \
                           `MutableMapping`, or `MutableSet` (at the top level or inside \
                           transparent wrappers and covariant container positions) when no \
                           caller in the same file mutates the returned collection in place. \
                           Functions whose header carries a substantive explanation comment \
                           (under the default `require-explanation` mode) are not flagged. \
                           Dunder methods other than `__init__` and `__new__`, methods on \
                           `Protocol` or `ABC` classes, and functions decorated with `@override`, \
                           `@overload`, `@abstractmethod`, or `@fixture` are exempt.",
            why_is_this_bad: "Returning `MutableSequence`, `MutableMapping`, or `MutableSet` \
                              invites callers to mutate the returned collection in place and \
                              forces the implementation to allocate or return a mutable \
                              container, preventing zero-copy returns of `tuple`, `frozenset`, \
                              or read-only `Sequence` / `Mapping` inputs.\n\n\
                              Prefer `Sequence`, `Mapping`, or `AbstractSet` (`from \
                              collections.abc import Set as AbstractSet`) for read-only return \
                              values, or document in a comment on the function header why \
                              external callers require a mutable collection.",
            references: &[Reference {
                title: "Python collections.abc — Collections Abstract Base Classes",
                url: "https://docs.python.org/3/library/collections.abc.html",
            }],
            examples: &[Example {
                language: SupportLang::Python,
                flagged: indoc::indoc! {r"
                    from collections.abc import MutableSequence

                    def active_tags(self) -> MutableSequence[str]:
                        return self._tags
                "},
                flagged_span: "MutableSequence[str]",
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

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    let locally_mutated = collect_locally_mutated_return_functions(file);
    let mut diagnostics = Vec::new();

    for signature in extract_function_signatures(file) {
        if signature.is_exempt_from_signature_rules()
            || locally_mutated.contains(signature.name.as_str())
        {
            continue;
        }
        let Some(ref return_type_node) = signature.return_type_node else {
            continue;
        };
        let matched = collect_mutable_collection_types(
            return_type_node,
            AnnotationTraversalDepth::CovariantPositions,
        );
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
                readonly_abstract_return_types => r#"
                    from collections.abc import Mapping, Sequence, Set as AbstractSet

                    def get_items() -> Sequence[str]:
                        return ["a"]

                    def get_counts() -> Mapping[str, int]:
                        return {"a": 1}

                    def get_tags() -> AbstractSet[str]:
                        return {"v1"}
                "#,
                locally_mutated_via_chained_call => r#"
                    from collections.abc import MutableSequence

                    def make_buffer() -> MutableSequence[str]:
                        return []

                    def build() -> None:
                        make_buffer().append("ready")
                "#,
                locally_mutated_via_assigned_variable => r#"
                    from collections.abc import MutableMapping, MutableSequence

                    class Builder:
                        def create_items(self) -> MutableSequence[int]:
                            return []

                        def create_map(self) -> MutableMapping[str, int]:
                            return {}

                        def run(self) -> None:
                            items = self.create_items()
                            items.extend([1, 2])
                            mapping = self.create_map()
                            mapping["count"] = 2
                "#,
                explained_by_header_comment => r#"
                    from collections.abc import MutableSet

                    # External plugins add and discard tags directly on the returned set.
                    def plugin_tags() -> MutableSet[str]:
                        return set()
                "#,
                override_overload_abstract_protocol_exempt => r#"
                    import abc
                    from collections.abc import MutableSequence
                    from typing import Protocol, overload, override

                    class Sink(Protocol):
                        def buffer(self) -> MutableSequence[str]: ...

                    class Base(abc.ABC):
                        @abc.abstractmethod
                        def items(self) -> MutableSequence[str]:
                            pass

                    class Impl(Base):
                        @override
                        def items(self) -> MutableSequence[str]:
                            return []

                    @overload
                    def fetch(x: int) -> MutableSequence[int]: ...
                "#,
            ],
            fail: [
                unexplained_mutable_sequence_return => r#"
                    from collections.abc import MutableSequence

                    def get_users() -> MutableSequence[str]:
                        return ["alice"]
                "# => "MutableSequence[str]",
                readonly_local_caller_still_flags_mutable_return => r#"
                    from collections.abc import MutableMapping

                    def get_counts() -> MutableMapping[str, int]:
                        return {"a": 1}

                    def total() -> int:
                        counts = get_counts()
                        return counts.get("a", 0)
                "# => "MutableMapping[str, int]",
                optional_mutable_set_return => r#"
                    from collections.abc import MutableSet
                    from typing import Optional

                    def maybe_tags() -> Optional[MutableSet[str]]:
                        return None
                "# => "Optional[MutableSet[str]]",
                covariant_nested_awaitable_mutable_return => r#"
                    from collections.abc import Awaitable, MutableSet

                    def async_tags() -> Awaitable[MutableSet[str]]:
                        raise NotImplementedError
                "# => "Awaitable[MutableSet[str]]",
            ],
        },
    }
);
