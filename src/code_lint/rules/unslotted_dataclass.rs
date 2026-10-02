//! Flags Python dataclasses defined without `slots=True`.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::extract_classes;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, ImpactedQuality, Precision, Reference, RuleDoc,
    RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
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
        languages: &[SupportLang::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::RECORD_TYPES],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Requires Python dataclasses to declare `slots=True`.",
            what_it_does: "Flags a class decorated with `@dataclass` or \
                           `@dataclasses.dataclass` that does not pass `slots`, in all Python \
                           files, tests included. An explicit `slots=False` counts as a \
                           deliberate choice and is not reported. The decorator is matched by \
                           name, not by import: a bare `@dataclass` is checked whatever module \
                           it comes from, while other decorators, such as `@attrs.define`, \
                           are not.",
            why_is_this_bad: "Without slots, each instance carries a `__dict__` that costs memory \
                              and silently accepts misspelled attribute assignments, so a typo \
                              in `instance.nmae = ...` creates a new attribute instead of \
                              failing.\n\n\
                              Write `@dataclass(slots=True)` (`slots` needs Python 3.10 or \
                              later). When dynamic attributes or multiple inheritance with \
                              other slotted bases are really needed, say so with \
                              `slots=False`.",
            references: &[Reference {
                title: "Python docs: dataclasses",
                url: "https://docs.python.org/3/library/dataclasses.html",
            }],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    extract_classes(file)
        .into_iter()
        .filter(|class| {
            class
                .dataclass_decorator()
                .is_some_and(|decorator| !decorator.has_arg("slots"))
        })
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
            ],
        },
    }
);
