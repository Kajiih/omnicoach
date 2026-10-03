//! Flags Python function return annotations that use abstract mutable collection types no caller in the file mutates.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    collect_locally_mutated_return_functions, collect_mutable_collection_types,
    extract_function_signatures, read_only_collection_replacements,
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
    summary: "Return annotation `{expression}` of `{function}` contains mutable collection type `{token}`, but no caller in this file mutates the result.",
    rationale: "Returning `MutableSequence`, `MutableMapping`, or `MutableSet` exposes mutability across the boundary and prevents returning immutable collections (`tuple`, `MappingProxyType`) or read-only parameter views directly.",
    suggestion: "Replace `{token}` in the return annotation of `{function}` with `{replacement}` if callers outside this file are not meant to mutate the result either.",
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
            summary: "Flags Python function return annotations using `MutableSequence`, `MutableMapping`, or `MutableSet` when no caller in the file mutates the result.",
            what_it_does: "Flags functions and methods in Python source files (test files are \
                           not checked) whose return annotation uses `MutableSequence`, \
                           `MutableMapping`, or `MutableSet` (at the top level or inside \
                           transparent `|`, `Optional`, `Union`, or `Annotated` wrappers) when \
                           no caller in the same file mutates the returned collection in place, \
                           either directly (`make().append(x)`) or through a variable bound in \
                           the same function (`buf = make()`, `(buf := make())`). Callers are \
                           matched by function name only. Dunder methods other than `__init__`, \
                           `__new__`, and `__call__`, methods on `Protocol` or `ABC` classes, and \
                           functions decorated with `@override`, `@overload`, `@abstractmethod`, \
                           `@fixture`, or `@<function>.register` are exempt.",
            why_is_this_bad: "Returning `MutableSequence`, `MutableMapping`, or `MutableSet` \
                              invites callers to mutate the returned collection in place and \
                              forces the implementation to allocate or return a mutable \
                              container, preventing zero-copy returns of `tuple`, `frozenset`, \
                              or read-only `Sequence` / `Mapping` inputs.\n\n\
                              A mutable return type is a contract that callers may mutate the \
                              result, so it is a deliberate choice; otherwise use `Sequence`, \
                              `Mapping`, or `Set` (imported as `AbstractSet`). Only callers in \
                              the same file are checked, so the suggestion cannot rule out \
                              callers elsewhere that rely on mutation.",
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
        let matched = collect_mutable_collection_types(return_type_node);
        if matched.is_empty() {
            continue;
        }
        let token = matched.join(", ");
        let replacement = read_only_collection_replacements(&matched);
        let expression = return_type_node.text();
        diagnostics.push(rule.diagnostic_at_node(
            path,
            return_type_node,
            &[
                ("function", &signature.name),
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
                protocol_class_exempt => r#"
                    from collections.abc import MutableSequence
                    from typing import Protocol

                    class Sink(Protocol):
                        def buffer(self) -> MutableSequence[str]: ...
                "#,
                abstractmethod_exempt => r#"
                    import abc
                    from collections.abc import MutableSequence

                    class Base:
                        @abc.abstractmethod
                        def items(self) -> MutableSequence[str]:
                            pass
                "#,
                override_exempt => r#"
                    from collections.abc import MutableSequence
                    from typing import override

                    class Impl(Base):
                        @override
                        def items(self) -> MutableSequence[str]:
                            return []
                "#,
                overload_exempt => r#"
                    from collections.abc import MutableSequence
                    from typing import overload

                    @overload
                    def fetch(x: int) -> MutableSequence[int]: ...
                "#,
                locally_mutated_via_walrus_binding => r#"
                    from collections.abc import MutableSequence

                    def make_buffer() -> MutableSequence[str]:
                        return []

                    def build() -> None:
                        if (buffer := make_buffer()) is not None:
                            buffer.append("ready")
                "#,
                nested_mutable_type_not_checked => r#"
                    from collections.abc import Awaitable, MutableSet

                    def async_tags() -> Awaitable[MutableSet[str]]:
                        raise NotImplementedError
                "#,
                known_gap_same_named_callee_exempts_function => r#"
                    from collections.abc import MutableSequence

                    def get() -> MutableSequence[str]:
                        return []

                    def configure(config) -> None:
                        config.get("plugins").append("core")
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
                binding_in_another_function_does_not_exempt_callee => r#"
                    from collections.abc import MutableMapping, MutableSequence

                    def make_a() -> MutableSequence[int]:
                        return []

                    def make_b() -> MutableMapping[str, int]:
                        return {}

                    def fill() -> None:
                        buffer = make_a()
                        buffer.append(1)

                    def read() -> int:
                        buffer = make_b()
                        return len(buffer)
                "# => "MutableMapping[str, int]",
                optional_mutable_set_return => r#"
                    from collections.abc import MutableSet
                    from typing import Optional

                    def maybe_tags() -> Optional[MutableSet[str]]:
                        return None
                "# => "Optional[MutableSet[str]]",
            ],
        },
    }
);
