//! Enforces that Python `@dataclass` classes specify `frozen=True` and `slots=True`.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{PythonClassInfo, extract_classes};
use crate::code_lint::rule::{CodeDetector, RuleTarget};
use crate::core::Detector;
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_documentation::{Reference, RuleDoc};
use crate::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Topic};
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Dataclass `{class}` is defined without `{missing}`.",
    rationale: "Default Python dataclasses are mutable and backed by a dynamic `__dict__`, allowing accidental state mutation and incurring unnecessary per-instance memory overhead.",
    suggestion: "Add `{missing}` to the `@dataclass` decorator (or explicitly pass `frozen=False` / `slots=False` when mutability or dynamic attributes are required).",
};

/// Rule that enforces `@dataclass(frozen=True, slots=True)` in Python files.
pub struct EnforceFrozenSlotsDataclass;

impl EnforceFrozenSlotsDataclass {
    /// The rule's declared facets (ADR 007).
    pub(crate) const CLASSIFICATION: Classification = Classification {
        topics: &[Topic::RECORD_TYPES],
        precision: Precision::Exact,
        consensus: Consensus::Opinionated,
        impacted_quality: ImpactedQuality::Reliability,
    };

    /// The rule's user-facing doc.
    pub(crate) const DOC: RuleDoc = RuleDoc {
        summary: "Requires Python dataclasses to declare `frozen=True` and `slots=True`.",
        what_it_does: "Flags a class decorated with `@dataclass` or \
                       `@dataclasses.dataclass` that does not pass both `frozen` and \
                       `slots`, in all Python files, tests included. An argument passed \
                       explicitly, even as `frozen=False` or `slots=False`, counts as a \
                       deliberate choice and is not reported as missing. The decorator is \
                       matched by name, not by import: a bare `@dataclass` is checked \
                       whatever module it comes from, while other decorators, such as \
                       `@attrs.define`, are not.",
        why_is_this_bad: "A default dataclass is mutable: any code holding an instance can \
                          change its fields, so a value passed to a function or stored in a \
                          cache can change behind the owner's back, and the instance cannot \
                          be hashed by value. Without slots, each instance carries a \
                          `__dict__` that costs memory and silently accepts misspelled \
                          attribute assignments.\n\n\
                          Write `@dataclass(frozen=True, slots=True)` (`slots` needs Python \
                          3.10 or later) and derive modified copies with \
                          `dataclasses.replace`. When mutability or a `__dict__` is really \
                          needed, say so with `frozen=False` or `slots=False`.",
        configuration: &[],
        references: &[Reference {
            title: "Python docs: dataclasses",
            url: "https://docs.python.org/3/library/dataclasses.html",
        }],
    };
}

impl Detector for EnforceFrozenSlotsDataclass {
    fn name(&self) -> RuleName {
        RuleName("enforce-frozen-slots-dataclass")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

/// Evaluates whether a dataclass definition is missing `frozen=True` or `slots=True`.
///
/// If the user explicitly passed `frozen=...` or `slots=...` (including `frozen=False` or
/// `slots=False`), this represents an intentional configuration or opt-out and is NOT flagged.
fn check_dataclass_info(
    rule: &EnforceFrozenSlotsDataclass,
    cls: &PythonClassInfo<'_>,
    path: &Path,
) -> Option<Diagnostic> {
    let dataclass_dec = cls
        .decorators
        .iter()
        .find(|dec| matches!(dec.path.as_str(), "dataclass" | "dataclasses.dataclass"))?;
    let missing: Vec<&str> = [("frozen", "frozen=True"), ("slots", "slots=True")]
        .into_iter()
        .filter(|(key, _)| !dataclass_dec.has_arg(key))
        .map(|(_, label)| label)
        .collect();

    if missing.is_empty() {
        return None;
    }

    let missing_description = missing.join(" and ");
    Some(rule.diagnostic_at_node(
        path,
        &cls.name_node,
        &[("class", &cls.name), ("missing", &missing_description)],
    ))
}

impl CodeDetector for EnforceFrozenSlotsDataclass {
    fn target(&self) -> RuleTarget {
        RuleTarget::All
    }

    fn check_file(
        &self,
        path: &Path,
        file: &ParsedFile,
        _config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        extract_classes(file)
            .into_iter()
            .filter_map(|cls| check_dataclass_info(self, &cls, path))
            .collect()
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    EnforceFrozenSlotsDataclass,
    {
        Python => {
            pass: [
                unqualified_frozen_slots_allowed => r#"
                    from dataclasses import dataclass

                    @dataclass(frozen=True, slots=True)
                    class ValidModel:
                        id: str
                "#,
                qualified_frozen_slots_allowed => r#"
                    import dataclasses

                    @dataclasses.dataclass(frozen=True, slots=True)
                    class ValidQualifiedModel:
                        id: str
                "#,
                explicit_mutable_opt_out_allowed => r#"
                    from dataclasses import dataclass

                    @dataclass(frozen=False, slots=True)
                    class ExplicitMutable:
                        id: str
                "#,
                explicit_no_slots_opt_out_allowed => r#"
                    from dataclasses import dataclass

                    @dataclass(frozen=True, slots=False)
                    class ExplicitNoSlots:
                        id: str
                "#,
                explicit_both_false_opt_out_allowed => r#"
                    from dataclasses import dataclass

                    @dataclass(frozen=False, slots=False)
                    class ExplicitBothFalse:
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
                missing_slots_argument => r#"
                    from dataclasses import dataclass

                    @dataclass(frozen=True)
                    class MissingSlots:
                        id: str
                "# => "MissingSlots",
                missing_frozen_argument => r#"
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
