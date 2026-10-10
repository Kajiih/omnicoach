//! Flags Python dataclasses defined without `slots=True`.

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
    summary: "Dataclass `{class}` is defined without `slots=True`.",
    rationale: "Without slots each instance carries a `__dict__`, which costs memory per instance and accepts a misspelled attribute assignment silently.",
    suggestion: "Add `slots=True` to the `@dataclass` decorator, or state `slots=False` when dynamic attributes or multiple inheritance are needed.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("unslotted-dataclass"),
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
            summary: "Requires Python dataclasses to declare `slots=True`.",
            what_it_does: indoc::indoc! {r"
                Flags a class decorated with `@dataclass` or `@dataclasses.dataclass` that does not
                pass `slots`. An explicit `slots=False` counts as a deliberate choice and is not
                reported."},
            why_is_this_bad: indoc::indoc! {r"
                Without slots, each instance carries a `__dict__` that costs memory and silently
                accepts misspelled attribute assignments, so a typo in `instance.nmae = ...` creates
                a new attribute instead of failing.

                Write `@dataclass(slots=True)` (`slots` needs Python 3.10 or later). When dynamic
                attributes or multiple inheritance with other slotted bases are really needed, say
                so with `slots=False`."},
            known_problems: None,
            references: &[
                Reference {
                    title: "Python docs: dataclasses",
                    url: "https://docs.python.org/3/library/dataclasses.html",
                },
                Reference::NAME_RESOLUTION,
            ],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r"
                    from dataclasses import dataclass

                    @dataclass(frozen=True)
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
        .filter(|class| class.is_dataclass_missing_arg("slots"))
        .map(|class| rule.diagnostic_at_node(path, &class.name_node, &[("class", &class.name)]))
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                unqualified_slots_allowed => r#"
                    from dataclasses import dataclass

                    @dataclass(frozen=True, slots=True)
                    class ValidModel:
                        id: str
                "#,
                qualified_slots_allowed => r#"
                    import dataclasses

                    @dataclasses.dataclass(slots=True)
                    class ValidQualifiedModel:
                        id: str
                "#,
                explicit_no_slots_opt_out_allowed => r#"
                    from dataclasses import dataclass

                    @dataclass(slots=False)
                    class ExplicitNoSlots:
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
                non_stdlib_dataclass_decorator_exempt => r#"
                    from marvin import dataclass

                    @dataclass
                    class CustomModel:
                        id: str
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
                frozen_without_slots => r#"
                    from dataclasses import dataclass

                    @dataclass(frozen=True)
                    class MissingSlots:
                        id: str
                "# => "MissingSlots",
                stacked_decorators_with_dataclass => r#"
                    from dataclasses import dataclass

                    @other_decorator
                    @dataclass
                    class StackedModel:
                        id: str
                "# => "StackedModel",
                aliased_dataclass_missing_slots => r#"
                    from dataclasses import dataclass as dc

                    @dc(frozen=True)
                    class AliasedModel:
                        id: str
                "# => "AliasedModel",
            ],
        },
    }
);
