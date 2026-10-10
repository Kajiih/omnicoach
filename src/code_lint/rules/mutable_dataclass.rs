//! Flags Python dataclasses defined without `frozen=True`.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::extract_classes;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Dataclass `{class}` is defined without `frozen=True`.",
    rationale: "A mutable dataclass lets any holder change it in place, so a value shared between two owners changes under one of them, and it cannot be hashed by value.",
    suggestion: "Add `frozen=True` to the `@dataclass` decorator, or state `frozen=False` when in-place mutation is needed.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("mutable-dataclass"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::RECORD_TYPES],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Requires Python dataclasses to declare `frozen=True`.",
            what_it_does: indoc::indoc! {r"
                Flags a class decorated with `@dataclass` or `@dataclasses.dataclass` that does not
                pass `frozen`. An explicit `frozen=False` counts as a deliberate choice and is not
                reported."},
            why_is_this_bad: indoc::indoc! {r"
                A default dataclass is mutable: any code holding an instance can change its fields,
                so a value passed to a function or stored in a cache can change behind the owner's
                back, and the instance cannot be hashed by value.

                Write `@dataclass(frozen=True)` and derive modified copies with
                `dataclasses.replace`. When in-place mutation is really needed, say so with
                `frozen=False`."},
            known_problems: Some(indoc::indoc! {r"
                The decorator is matched as written, not through imports: a `@dataclass` imported
                from another library is checked, and one imported under another name, such as
                `from dataclasses import dataclass as dc`, is not."}),
            references: &[Reference {
                title: "Python docs: dataclasses",
                url: "https://docs.python.org/3/library/dataclasses.html",
            }],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r"
                    from dataclasses import dataclass

                    @dataclass(slots=True)
                    class Invoice:
                        number: str
                        amount_cents: int
                "},
                flagged_span: "Invoice",
                fixed: indoc::indoc! {r"
                    from dataclasses import dataclass

                    @dataclass(frozen=True, slots=True)
                    class Invoice:
                        number: str
                        amount_cents: int
                "},
            }],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    extract_classes(file)
        .into_iter()
        .filter(|class| class.is_dataclass_missing_arg("frozen"))
        .map(|class| rule.diagnostic_at_node(path, &class.name_node, &[("class", &class.name)]))
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                unqualified_frozen_allowed => r#"
                    from dataclasses import dataclass

                    @dataclass(frozen=True, slots=True)
                    class ValidModel:
                        id: str
                "#,
                qualified_frozen_allowed => r#"
                    import dataclasses

                    @dataclasses.dataclass(frozen=True)
                    class ValidQualifiedModel:
                        id: str
                "#,
                explicit_mutable_opt_out_allowed => r#"
                    from dataclasses import dataclass

                    @dataclass(frozen=False)
                    class ExplicitMutable:
                        id: str
                "#,
                undecorated_class_exempt => r#"
                    class RegularClass:
                        pass
                "#,
                other_decorator_class_exempt => r#"
                    @other_decorator
                    class DecoratedClass:
                        pass
                "#,
            ],
            fail: [
                bare_unqualified_dataclass => r#"
                    from dataclasses import dataclass

                    @dataclass
                    class BareModel:
                        id: str
                "# => "BareModel",
                empty_parens_qualified_dataclass => r#"
                    import dataclasses

                    @dataclasses.dataclass()
                    class EmptyParensModel:
                        id: str
                "# => "EmptyParensModel",
                slots_without_frozen => r#"
                    from dataclasses import dataclass

                    @dataclass(slots=True)
                    class MissingFrozen:
                        id: str
                "# => "MissingFrozen",
                stacked_decorators_with_dataclass => r#"
                    from dataclasses import dataclass

                    @other_decorator
                    @dataclass
                    class StackedModel:
                        id: str
                "# => "StackedModel",
            ],
        },
    }
);
