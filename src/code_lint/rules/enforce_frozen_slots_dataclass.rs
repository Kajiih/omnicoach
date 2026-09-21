//! Enforces that Python `@dataclass` classes specify `frozen=True` and `slots=True`.

use crate::code_lint::ast_python::{PythonClassInfo, extract_classes};
use crate::code_lint::{CodeRule, RuleTarget};
use crate::core::{Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Dataclass `{class}` should be defined with `{missing}` to ensure immutability and memory efficiency.",
    rationale: "Default Python dataclasses are mutable and retain dynamic `__dict__` overhead. Defining `frozen=True` enforces immutability and thread-safety, while `slots=True` eliminates per-instance dictionary memory overhead and speeds up attribute access.",
    suggestion: "Add `{missing}` to the `@dataclass(...)` decorator. If mutability or dynamic attributes are strictly required, explicitly specify `frozen=False` or `slots=False` to document the design choice.",
};

/// Rule that enforces `@dataclass(frozen=True, slots=True)` in Python files.
pub struct EnforceFrozenSlotsDataclass;

impl Rule for EnforceFrozenSlotsDataclass {
    fn name(&self) -> RuleName {
        RuleName("enforce-frozen-slots-dataclass")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Typing, Tag::Style, Tag::Opinionated]
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
    let dataclass_dec = cls.dataclass_decorator()?;
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

impl CodeRule for EnforceFrozenSlotsDataclass {
    fn target(&self) -> RuleTarget {
        RuleTarget::All
    }

    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<crate::code_lint::SourceDoc>,
        _config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        extract_classes(&grep.root())
            .into_iter()
            .filter_map(|cls| check_dataclass_info(self, &cls, path))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_code_rule_snapshot;

    #[test]
    fn test_enforce_frozen_slots_dataclass_detection() {
        let rule = EnforceFrozenSlotsDataclass;

        let source = indoc::indoc! {r"
            from dataclasses import dataclass
            import dataclasses

            # Flagged: bare @dataclass missing both
            @dataclass
            class BareModel:
                id: str

            # Flagged: only frozen=True provided, missing slots=True
            @dataclass(frozen=True)
            class MissingSlots:
                id: str

            # Flagged: only slots=True provided, missing frozen=True
            @dataclasses.dataclass(slots=True)
            class MissingFrozen:
                id: str

            # OK: both frozen=True and slots=True specified
            @dataclass(frozen=True, slots=True)
            class ValidModel:
                id: str

            # OK: user explicitly specified frozen=False (intentional opt-out)
            @dataclass(frozen=False, slots=True)
            class ExplicitMutable:
                id: str

            # OK: user explicitly specified slots=False (intentional opt-out)
            @dataclass(frozen=True, slots=False)
            class ExplicitNoSlots:
                id: str

            # OK: standard non-dataclass class
            class RegularClass:
                def __init__(self, x: int) -> None:
                    self.x = x
        "};

        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, source, "src/models.py"), @"
        [enforce-frozen-slots-dataclass] Line 6, Col 7: Dataclass `BareModel` should be defined with `frozen=True and slots=True` to ensure immutability and memory efficiency.
        [enforce-frozen-slots-dataclass] Line 11, Col 7: Dataclass `MissingSlots` should be defined with `slots=True` to ensure immutability and memory efficiency.
        [enforce-frozen-slots-dataclass] Line 16, Col 7: Dataclass `MissingFrozen` should be defined with `frozen=True` to ensure immutability and memory efficiency.
        ");
    }
}
