//! Flags Python module-level constants whose type annotation or value is a mutable collection.

use crate::code_lint::ast::python::{
    AnnotationTraversalDepth, CollectionKind, CollectionShape, PythonCollectionType,
    PythonModuleAssignment, call_callee, collect_collection_types, collect_module_assignments,
    collection_display, collection_type,
};
use crate::code_lint::ast::{AstNode, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::code_lint::policy::replacements_by_shape;
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

/// The builtin mapping, which a `Mapping` annotation already makes read-only to type checkers.
const DICT: &str = "dict";

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Module constant `{name}` has mutable collection type `{token}`.",
    rationale: "A module constant typed or initialized as a mutable collection allows in-place mutation across importers, and `Final` only prevents rebinding the name.",
    suggestion: "Replace the `{token}` type or value of `{name}` with `{replacement}` and annotate `{name}` with `Final`, or rename `{name}` to `_`-prefixed lowercase without `Final` if it is mutable module state.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("mutable-module-constant"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::GLOBAL_STATE, Topic::STATIC_TYPING],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags Python module-level constants whose type annotation or value is a mutable collection.",
            what_it_does: indoc::indoc! {r"
                Flags module-level assignments (including inside top-level `if`, `elif`, `else`,
                `try`, `except`, `finally`, and `with` blocks) in Python source files (test files
                are not checked) whose target is a single identifier in `UPPER_SNAKE_CASE` (with an
                optional leading `_`) or annotated with `Final`. A constant is flagged when its type
                annotation uses a concrete mutable collection (`list`, `dict`, `set`, `List`,
                `Dict`, `Set`, `typing.List`, `typing.Dict`, `typing.Set`, or the `collections`
                containers `defaultdict`, `deque`, `Counter`, and `OrderedDict` with their `typing`
                aliases) or an abstract mutable collection (`MutableSequence`, `MutableMapping`,
                `MutableSet`), including inside `Final`, `Optional`, `Union`, `|`, `Annotated`, and
                covariant container positions (`tuple[list[T], ...]`, `Mapping[K, list[V]]`). When
                the annotation does not itself use a mutable collection, a constant is flagged if
                its value is a `list` or `set` literal (`[...]`, `{a, b}`), comprehension
                (`[x for ...]`, `{x for ...}`), or mutable constructor call (`list(...)`,
                `set(...)`, `deque(...)`), or if its value is a `dict` literal (`{k: v}`, `{}`),
                comprehension (`{k: v for ...}`), or `dict(...)` call without a `Mapping` annotation
                (such as `Mapping[K, V]` or `Final[Mapping[K, V]]`). Calls to `defaultdict`,
                `Counter`, and `OrderedDict` are always flagged. Unqualified `Set` is resolved from
                the file's imports (`from typing import Set` is flagged;
                `from collections.abc import Set` is exempt). Dunder names (such as `__all__`),
                lowercase module variables without `Final`, attribute or unpacking targets
                (`config.ALLOWED = ...`, `A, B = ...`), class attributes, function-local variables,
                and type aliases (`TypeAlias`, `type X = ...`) are not flagged. String annotations
                are not resolved."},
            why_is_this_bad: indoc::indoc! {r#"
                In Python, `UPPER_SNAKE_CASE` naming and `typing.Final` signal that a module
                attribute is a constant, yet `Final` only prevents rebinding the variable name. When
                a constant is initialized as `ALLOWED = ["a", "b"]` or
                `ALLOWED: Final = ["a", "b"]`, type checkers infer `list[str]` and allow callers to
                run `ALLOWED.append(...)` or `ALLOWED.clear()` without error, corrupting shared
                state across every module that imports it.

                Use `tuple` literals (`(...)`) instead of `list`, `frozenset` instead of `set`, and
                `frozendict` (Python 3.15+), `types.MappingProxyType`, or a
                `collections.abc.Mapping` annotation instead of `dict`, combined with `Final`. If
                the collection is intentionally mutated at runtime, name it in `_`-prefixed
                `lower_snake_case` without `Final` to show that it is module state rather than a
                constant."#},
            references: &[
                Reference {
                    title: "PEP 591: Adding a final qualifier to typing",
                    url: "https://peps.python.org/pep-0591/",
                },
                Reference {
                    title: "PEP 814: Add frozendict built-in type",
                    url: "https://peps.python.org/pep-0814/",
                },
                Reference {
                    title: "Google Python Style Guide: Global variables",
                    url: "https://google.github.io/styleguide/pyguide.html#25-global-variables",
                },
            ],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r#"
                    ALLOWED_ROLES: list[str] = ["admin", "viewer"]
                "#},
                flagged_span: "list[str]",
                fixed: indoc::indoc! {r#"
                    from typing import Final

                    ALLOWED_ROLES: Final = ("admin", "viewer")
                "#},
            }],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for assignment in collect_module_assignments(file) {
        let is_dunder = assignment.name.starts_with("__") && assignment.name.ends_with("__");
        if is_dunder || !assignment.is_constant() {
            continue;
        }
        let Some((target_node, matched)) = mutable_collections(&assignment) else {
            continue;
        };
        let token = PythonCollectionType::joined_paths(&matched);
        let replacement = replacements_by_shape(&matched, |shape| match shape {
            CollectionShape::Mapping => "frozendict",
            CollectionShape::Set => "frozenset",
            CollectionShape::Sequence | CollectionShape::Iterable => "tuple",
        });
        diagnostics.push(rule.diagnostic_at_node(
            path,
            target_node,
            &[
                ("name", &assignment.name),
                ("token", &token),
                ("replacement", &replacement),
            ],
        ));
    }
    diagnostics
}

/// The node to report and the mutable collection types of `assignment`: those of its annotation,
/// or else the collection its value builds.
fn mutable_collections<'tree, 'assignment>(
    assignment: &'assignment PythonModuleAssignment<'tree>,
) -> Option<(&'assignment AstNode<'tree>, Vec<PythonCollectionType>)> {
    if let Some(annotation) = &assignment.annotation {
        let matched: Vec<_> =
            collect_collection_types(annotation, AnnotationTraversalDepth::CovariantPositions)
                .into_iter()
                .filter(|collection_type| {
                    matches!(
                        collection_type.kind,
                        CollectionKind::ConcreteMutable | CollectionKind::AbstractMutable
                    )
                })
                .collect();
        if !matched.is_empty() {
            return Some((annotation, matched));
        }
    }
    let value = assignment.value.as_ref()?;
    let built = collection_display(value).or_else(|| {
        call_callee(value)
            .and_then(|callee| collection_type(&callee))
            .filter(is_runtime_mutable_collection_constructor)
    })?;
    let is_read_only_mapping = assignment.annotation.as_ref().is_some_and(|annotation| {
        collect_collection_types(
            annotation,
            AnnotationTraversalDepth::TransparentWrappersOnly,
        )
        .iter()
        .any(|collection_type| collection_type.name == "Mapping")
    });
    if is_read_only_mapping && built.name == DICT {
        return None;
    }
    Some((value, vec![built]))
}

/// Returns true if `constructed` is called at runtime to build a mutable collection: `list`,
/// `dict`, `set`, `defaultdict`, `deque`, `Counter`, or `OrderedDict`.
fn is_runtime_mutable_collection_constructor(constructed: &PythonCollectionType) -> bool {
    matches!(
        constructed.name.as_str(),
        "list" | DICT | "set" | "defaultdict" | "deque" | "Counter" | "OrderedDict"
    )
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                immutable_collection_literals => r#"
                    from typing import Final

                    ALLOWED_ROLES: Final = ("admin", "viewer")
                    EMPTY_TUPLE = ()
                    MAX_RETRIES: Final = 3
                "#,
                immutable_collection_constructors => r#"
                    from types import MappingProxyType
                    from typing import Final

                    ALLOWED_TAGS: Final = frozenset({"alpha", "beta"})
                    PORTS_PROXY: Final = MappingProxyType({"http": 80, "https": 443})
                    PORTS_FROZEN: Final = frozendict({"http": 80, "https": 443})
                "#,
                mapping_annotated_dict_literal_exempt => r#"
                    from collections.abc import Mapping
                    from typing import Final

                    PORTS: Mapping[str, int] = {"http": 80, "https": 443}
                    FINAL_PORTS: Final[Mapping[str, int]] = {"http": 80}
                "#,
                mapping_annotated_dict_comprehension_exempt => r#"
                    from collections.abc import Mapping

                    INDEX_BY_KEY: Mapping[int, int] = {x: x for x in range(4)}
                "#,
                mapping_annotated_dict_call_exempt => r#"
                    from collections.abc import Mapping

                    BUILT_PORTS: Mapping[str, int] = dict(http=80)
                "#,
                dunder_all_exempt => r#"
                    from typing import Final

                    __all__ = ["Service", "Client"]
                    __all__: Final = ["Service", "Client"]
                    __all__: list[str] = ["Service", "Client"]
                "#,
                lowercase_module_state_not_flagged => r#"
                    _cache: dict[str, int] = {}
                    _handlers = []
                    active_sessions: set[str] = set()
                "#,
                class_scope_not_flagged => r#"
                    from typing import Final

                    class Config:
                        ALLOWED = ["a", "b"]
                        FINAL_ROLES: Final = ["a", "b"]
                        PORTS: dict[str, int] = {"http": 80}
                "#,
                function_scope_not_flagged => r#"
                    def load() -> None:
                        LOCAL_LIST = ["a", "b"]
                        LOCAL_DICT: dict[str, int] = {}
                "#,
                attribute_target_not_flagged => r#"
                    from typing import Final

                    config.ALLOWED: Final = ["a", "b"]
                "#,
                unpacking_target_not_flagged => r#"
                    FIRST_GROUP, SECOND_GROUP = ["a"], ["b"]
                "#,
                typing_type_alias_not_flagged => r#"
                    from typing import TypeAlias

                    JSON_ARRAY: TypeAlias = list[str]
                "#,
                pep695_type_alias_not_flagged => r#"
                    type MODERN_ARRAY = list[str]
                "#,
                collections_abc_set_import_exempts_set_annotation => r#"
                    from collections.abc import Set

                    ALLOWED_TAGS: Set[str] = frozenset({"alpha", "beta"})
                "#,
            ],
            fail: [
                unannotated_list_literal => r#"
                    ALLOWED = ["admin", "viewer"]
                "# => r#"["admin", "viewer"]"#,
                parenthesized_mutable_literal => r#"
                    ALLOWED = (["admin", "viewer"])
                "# => r#"["admin", "viewer"]"#,
                multiline_parenthesized_with_comment => r#"
                    ALLOWED = (
                        # Default roles
                        ["admin", "viewer"]
                    )
                "# => r#"["admin", "viewer"]"#,
                unannotated_dict_literal => r#"
                    PORTS = {"http": 80}
                "# => r#"{"http": 80}"#,
                unannotated_set_literal => r#"
                    TAGS = {"alpha", "beta"}
                "# => r#"{"alpha", "beta"}"#,
                list_comprehension_value => r#"
                    SQUARES = [x * x for x in range(4)]
                "# => "[x * x for x in range(4)]",
                set_comprehension_value => r#"
                    UNIQUE_MODS = {x % 3 for x in range(9)}
                "# => "{x % 3 for x in range(9)}",
                dict_comprehension_value => r#"
                    INDEX_BY_KEY = {x: x for x in range(4)}
                "# => "{x: x for x in range(4)}",
                mutable_constructor_call => r#"
                    EMPTY_BUFFER = list()
                "# => "list()",
                subscripted_constructor_call => r#"
                    EMPTY_ITEMS = list[str]()
                "# => "list[str]()",
                collections_deque_call => r#"
                    from collections import deque

                    WORK_QUEUE = deque()
                "# => "deque()",
                private_upper_snake_case_constant => r#"
                    _INTERNAL_ROLES = ["admin"]
                "# => r#"["admin"]"#,
                bare_final_with_mutable_list => r#"
                    from typing import Final

                    ALLOWED: Final = ["admin", "viewer"]
                "# => r#"["admin", "viewer"]"#,
                bare_final_with_mutable_dict => r#"
                    from typing import Final

                    PORTS: Final = {"http": 80}
                "# => r#"{"http": 80}"#,
                lowercase_final_with_mutable_list => r#"
                    from typing import Final

                    allowed_roles: Final = ["admin", "viewer"]
                "# => r#"["admin", "viewer"]"#,
                annotated_wrapped_final_on_lowercase => r#"
                    from typing import Annotated, Final

                    allowed_roles: Annotated[Final[list[str]], "doc"] = ["admin"]
                "# => r#"Annotated[Final[list[str]], "doc"]"#,
                bare_annotation_without_value => r#"
                    ALLOWED: list[str]
                "# => "list[str]",
                annotated_concrete_list => r#"
                    ALLOWED: list[str] = ("admin", "viewer")
                "# => "list[str]",
                annotated_final_concrete_dict => r#"
                    from typing import Final

                    PORTS: Final[dict[str, int]] = {"http": 80}
                "# => "Final[dict[str, int]]",
                union_mapping_and_concrete_dict_flagged => r#"
                    from collections.abc import Mapping

                    PORTS: Mapping[str, int] | dict[str, int] = {"http": 80}
                "# => "Mapping[str, int] | dict[str, int]",
                annotated_mutable_sequence => r#"
                    from collections.abc import MutableSequence

                    ALLOWED: MutableSequence[str] = ("admin", "viewer")
                "# => "MutableSequence[str]",
                nested_mutable_in_covariant_tuple_annotation => r#"
                    from collections.abc import MutableSequence

                    GROUPS: tuple[MutableSequence[str], ...] = ()
                "# => "tuple[MutableSequence[str], ...]",
                sequence_annotation_with_mutable_list_literal => r#"
                    from collections.abc import Sequence

                    ALLOWED: Sequence[str] = ["admin", "viewer"]
                "# => r#"["admin", "viewer"]"#,
                abstract_set_annotation_with_mutable_set_literal => r#"
                    from collections.abc import Set as AbstractSet

                    TAGS: AbstractSet[str] = {"alpha", "beta"}
                "# => r#"{"alpha", "beta"}"#,
                mapping_annotation_with_defaultdict_call => r#"
                    from collections import defaultdict
                    from collections.abc import Mapping

                    COUNTS: Mapping[str, int] = defaultdict(int)
                "# => "defaultdict(int)",
                mapping_annotation_with_mutable_value_type => r#"
                    from collections.abc import Mapping

                    GROUPS: Mapping[str, list[int]] = {"a": [1, 2]}
                "# => "Mapping[str, list[int]]",
                unqualified_set_annotation_without_abc_import => r#"
                    from typing import Set

                    TAGS: Set[str] = frozenset({"alpha"})
                "# => "Set[str]",
                inside_top_level_if => r#"
                    import sys

                    if sys.platform == "linux":
                        LINUX_HOSTS = ["localhost"]
                "# => r#"["localhost"]"#,
                inside_top_level_try_except => r#"
                    try:
                        import uvloop as _uvloop
                    except ImportError:
                        FALLBACK_HOSTS = ["localhost"]
                "# => r#"["localhost"]"#,
            ],
        },
    }
);
