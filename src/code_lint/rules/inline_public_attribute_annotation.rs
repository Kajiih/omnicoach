//! Flags public Python instance attributes annotated inline inside methods.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::collect_inline_public_attribute_annotations;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Public attribute `self.{name}` of `{class}` is annotated inline in `{function}` with `{expression}`.",
    rationale: "Inline attribute annotations inside methods are omitted from `{class}.__annotations__` at runtime and hide the class's public data contract inside method bodies.",
    suggestion: "Move `{name}: {expression}` to the body of `{class}` and keep only the unannotated `self.{name}` assignment in `{function}`.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("inline-public-attribute-annotation"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::STATIC_TYPING],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags public Python instance attributes annotated inline inside methods instead of in the class body.",
            what_it_does: indoc::indoc! {r"
                Flags annotated assignments to public instance attributes (`self.attr: Type = value`
                or `self.attr: Type`) inside instance methods in Python source files (test files are
                not checked). Only direct instance methods whose first parameter is `self` and that
                are not decorated with `@staticmethod` or `@classmethod` are inspected. Private
                attributes starting with `_`, unannotated assignments (`self.attr = value`),
                unparameterized `Final` annotations (`self.attr: Final = value`), and
                field-synthesizing classes (`@dataclass`, `attrs`
                `@define`/`@frozen`/`@mutable`/`@s`, and Pydantic `BaseModel`, where a class-body
                annotation alters `__init__` or field validation) are not flagged."},
            why_is_this_bad: indoc::indoc! {r"
                A public attribute is part of a class's external interface. When its type annotation
                is written inline on `self.attr: Type` inside `__init__` or another method, Python's
                compiler discards the annotation at compile time rather than storing it in
                `cls.__annotations__`, so `typing.get_type_hints()` and `inspect.get_annotations()`
                cannot see it, and readers must scan method bodies to discover the class's public
                attributes.

                Declare `attr: Type` in the class body and assign `self.attr = value` without an
                inline type annotation inside methods. Internal state that does not belong to the
                class's public contract can be prefixed with `_` and annotated either in the class
                body or inline."},
            references: &[
                Reference {
                    title: "PEP 526: Syntax for Variable Annotations — Class and instance variable annotations",
                    url: "https://peps.python.org/pep-0526/#class-and-instance-variable-annotations",
                },
                Reference {
                    title: "Google Python Style Guide: Type Annotations",
                    url: "https://google.github.io/styleguide/pyguide.html#319-type-annotations",
                },
            ],
            examples: &[Example {
                language: SupportLang::Python,
                flagged: indoc::indoc! {r"
                    class Connection:
                        def __init__(self, host: str) -> None:
                            self.host = host
                            self.retries: int = 3
                "},
                flagged_span: "self.retries: int = 3",
                fixed: indoc::indoc! {r"
                    class Connection:
                        retries: int

                        def __init__(self, host: str) -> None:
                            self.host = host
                            self.retries = 3
                "},
            }],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    collect_inline_public_attribute_annotations(file)
        .into_iter()
        .map(|attribute| {
            rule.diagnostic_at_node(
                path,
                &attribute.assignment_node,
                &[
                    ("name", &attribute.name),
                    ("class", &attribute.class_name),
                    ("function", &attribute.method_name),
                    ("expression", &attribute.annotation_text),
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
                class_body_public_and_private_annotations => r#"
                    from typing import ClassVar

                    class Connection:
                        MAX_POOL: ClassVar[int] = 10
                        host: str
                        retries: int = 3
                        _socket: object | None = None

                        def __init__(self, host: str, retries: int) -> None:
                            self.host = host
                            self.retries = retries
                            self._socket = None
                "#,
                inline_private_attribute_annotations_exempt => r#"
                    class Cache:
                        def __init__(self) -> None:
                            self._entries: dict[str, int] = {}
                            self.__secret: str = "token"

                        def reset(self) -> None:
                            self._entries = {}
                "#,
                unannotated_public_attribute_assignments_exempt => r#"
                    class Counter:
                        count: int
                        items: list[str]

                        def __init__(self) -> None:
                            self.count = 0
                            self.items = ["a"]

                        def increment(self) -> None:
                            self.count += 1
                            self.items[0] = "b"
                "#,
                bare_final_inline_annotation_exempt => r#"
                    import typing
                    import typing_extensions
                    from typing import Annotated, Final

                    class Token:
                        def __init__(self, raw: str) -> None:
                            self.raw: Final = raw
                            self.version: typing.Final = 1
                            self.scheme: typing_extensions.Final = "bearer"
                            self.tag: Annotated[Final, "meta"] = "v1"
                "#,
                staticmethod_and_classmethod_exempt => r#"
                    from typing import Any

                    class Factory:
                        @staticmethod
                        def configure(self: Any) -> None:
                            self.timeout: int = 10

                        @classmethod
                        def from_env(cls) -> None:
                            cls.default_timeout: int = 20
                "#,
                non_self_first_parameter_or_other_object_attribute_exempt => r#"
                    from typing import Any, Self

                    class Box:
                        value: int
                        inner: Any

                        def __new__(cls) -> Self:
                            cls.cached: Any = None
                            return super().__new__(cls)

                        def bind(cls, self: Any) -> None:
                            self.cached: int = 1

                        def copy_into(self, other: "Box") -> None:
                            other.value: int = self.value
                            self.inner.value: int = 1
                "#,
                nested_function_and_lambda_inside_method_not_entered => r#"
                    from typing import Any

                    class Runner:
                        def run(self) -> None:
                            def helper(self: Any) -> None:
                                self.leaked: int = 1
                "#,
                local_variable_annotations_in_method_exempt => r#"
                    class Calculator:
                        def compute(self, x: int) -> int:
                            total: int = x + 1
                            return total
                "#,
                module_level_function_with_self_param_exempt => r#"
                    from typing import Any

                    def standalone(self: Any) -> None:
                        self.value: int = 1
                "#,
                pydantic_and_attrs_and_slots_private_class_attributes_exempt => r#"
                    class SlottedService:
                        __slots__ = ("_client", "_attempts")
                        _client: object | None
                        _attempts: int

                        def __init__(self, flag: bool) -> None:
                            self._client = None
                            if flag:
                                self._attempts: int = 1
                            else:
                                self._attempts: int = 0
                "#,
                dataclass_attrs_and_pydantic_base_model_exempt => r#"
                    from dataclasses import dataclass
                    from attrs import define
                    from pydantic import BaseModel

                    @dataclass
                    class OrderSummary:
                        items: list[int]

                        def __post_init__(self) -> None:
                            self.total: int = sum(self.items)

                    @define
                    class AttrsSummary:
                        items: list[int]

                        def __attrs_post_init__(self) -> None:
                            self.total: int = sum(self.items)

                    class PydanticSummary(BaseModel):
                        items: list[int]

                        def model_post_init(self, __context: object) -> None:
                            self.total: int = sum(self.items)
                "#,
            ],
            fail: [
                inline_public_attribute_in_init => r#"
                    class Connection:
                        def __init__(self, host: str) -> None:
                            self.host = host
                            self.retries: int = 3
                "# => "self.retries: int = 3",
                valueless_inline_public_attribute_in_init => r#"
                    class Server:
                        def __init__(self) -> None:
                            self.host: str
                "# => "self.host: str",
                inline_public_attribute_in_regular_method => r#"
                    class Worker:
                        def reset(self) -> None:
                            self.count: int = 0
                "# => "self.count: int = 0",
                inline_public_attribute_in_async_method => r#"
                    class AsyncClient:
                        async def connect(self) -> None:
                            self.connected: bool = True
                "# => "self.connected: bool = True",
                inline_public_attribute_in_property_or_decorated_method => r#"
                    class Monitor:
                        @property
                        def status(self) -> str:
                            self.cached_status: str = "ok"
                            return self.cached_status
                "# => "self.cached_status: str = \"ok\"",
                inline_public_attribute_inside_control_flow_block => r#"
                    class Engine:
                        def __init__(self, fast: bool) -> None:
                            if fast:
                                self.mode: str = "fast"
                "# => "self.mode: str = \"fast\"",
                parameterized_final_inline_annotation_flagged => r#"
                    from typing import Final

                    class Record:
                        def __init__(self, record_id: int) -> None:
                            self.record_id: Final[int] = record_id
                "# => "self.record_id: Final[int] = record_id",
                redundant_inline_annotation_when_also_annotated_in_class_body => r#"
                    class Worker:
                        count: int

                        def __init__(self) -> None:
                            self.count: int = 0
                "# => "self.count: int = 0",
                inline_public_attribute_in_nested_class_method => r#"
                    class Outer:
                        def build(self) -> None:
                            class Inner:
                                def __init__(self) -> None:
                                    self.value: int = 1
                "# => "self.value: int = 1",
            ]
        }
    }
);
