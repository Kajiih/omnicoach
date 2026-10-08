//! Flags type-annotated class and instance attributes declared after methods in a Python class body.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::collect_fields_after_methods;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Attribute `{name}` of `{class}` is declared after a method definition.",
    rationale: "Declaring class or instance attributes after methods scatters the data layout of `{class}` across its body and obscures the synthesized `__init__` parameter order in `@dataclass`, `attrs`, and `NamedTuple` classes.",
    suggestion: "Move the `{name}` attribute declaration to the top of `{class}`, before any method definitions.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("field-after-method"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::DECLARATION_ORDER],
            precision: Precision::Exact,
            consensus: Consensus::Unopinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags type-annotated class and instance attributes declared after methods in a Python class body.",
            what_it_does: indoc::indoc! {r"
                Checks Python `class` definitions for type-annotated attribute declarations
                (`name: Type` or `name: Type = value`) placed after a method definition (`def` or
                `async def`) in the same class body.

                Unannotated class-body assignments after methods (such as method aliases
                `__repr__ = __str__` or `property(get_value)` descriptors, which reference an
                earlier `def`), subscript or attribute assignment targets (`cls.value: int = 1`),
                annotations inside method bodies, and test files are not flagged."},
            why_is_this_bad: indoc::indoc! {r"
                Class-level type annotations define the data layout of a class and determine the
                synthesized `__init__` parameter order in `@dataclass`, `attrs`, `NamedTuple`, and
                Pydantic `BaseModel` classes. Scattering field annotations below or between
                methods hides part of the class's state schema at the bottom of the class body.

                Declare all type-annotated class and instance attributes at the top of the class
                body, before any method definitions."},
            references: &[
                Reference {
                    title: "PEP 526: Syntax for Variable Annotations — Class and instance variable annotations",
                    url: "https://peps.python.org/pep-0526/#class-and-instance-variable-annotations",
                },
                Reference {
                    title: "flake8-class-attributes-order: CCE001",
                    url: "https://github.com/best-doctor/flake8-class-attributes-order",
                },
            ],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r"
                    class Connection:
                        host: str

                        def __init__(self, host: str, retries: int) -> None:
                            self.host = host
                            self.retries = retries

                        retries: int = 3
                "},
                flagged_span: "retries: int = 3",
                fixed: indoc::indoc! {r"
                    class Connection:
                        host: str
                        retries: int = 3

                        def __init__(self, host: str, retries: int) -> None:
                            self.host = host
                            self.retries = retries
                "},
            }],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    collect_fields_after_methods(file)
        .into_iter()
        .map(|field| {
            rule.diagnostic_at_node(
                path,
                &field.node,
                &[("name", &field.name), ("class", &field.class_name)],
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
                fields_before_methods => r#"
                    from dataclasses import dataclass
                    from typing import ClassVar

                    @dataclass(frozen=True, slots=True)
                    class Endpoint:
                        DEFAULT_PORT: ClassVar[int] = 80
                        host: str
                        port: int = 1

                        def is_secure(self) -> bool:
                            return self.port == 443
                "#,
                unannotated_method_alias_and_property_after_def_exempt => r#"
                    class Printer:
                        label: str

                        def __init__(self, label: str) -> None:
                            self.label = label

                        def __str__(self) -> str:
                            return self.label

                        __repr__ = __str__
                "#,
                non_name_target_and_nested_class_exempt => r#"
                    class Outer:
                        def execute(self) -> None:
                            local_count: int = 1
                            self._cached: int = local_count

                        cls.fallback: int = 0

                        class Inner:
                            port: int = 1

                            def inner_port(self) -> int:
                                return self.port
                "#,
            ],
            fail: [
                annotated_field_with_default_after_init => r#"
                    class Connection:
                        host: str

                        def __init__(self, host: str) -> None:
                            self.host = host
                            self.retries = 1

                        retries: int = 1
                "# => "retries: int = 1",
                bare_annotation_after_method => r#"
                    class Endpoint:
                        def port_number(self) -> int:
                            return self.port

                        port: int
                "# => "port: int",
                classvar_after_async_method => r#"
                    from typing import ClassVar

                    class Worker:
                        async def run(self) -> int:
                            return self.MAX_RETRIES

                        MAX_RETRIES: ClassVar[int] = 5
                "# => "MAX_RETRIES: ClassVar[int] = 5",
            ],
        },
    }
);
