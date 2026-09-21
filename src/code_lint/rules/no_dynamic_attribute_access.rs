//! Bans dynamic runtime attribute reflection (`getattr`, `hasattr`, `setattr`, `delattr`).

use crate::code_lint::{CodeRule, RuleTarget};
use crate::core::{FilterListDefaults, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Static defaults for banned dynamic reflection functions.
const DEFAULT_BANNED_FUNCTIONS: FilterListDefaults = FilterListDefaults {
    base: &[
        "getattr",
        "hasattr",
        "setattr",
        "delattr",
        "builtins.getattr",
        "builtins.hasattr",
        "builtins.setattr",
        "builtins.delattr",
    ],
    extend: &[],
    exempt: &[],
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Dynamic reflection call `{callee}()` bypasses static type checking.",
    rationale: "Runtime attribute reflection (`getattr`, `hasattr`, `setattr`, `delattr`) defeats static type analysis by erasing attribute types to `Any`, obscures symbol references during refactoring, and `hasattr` can silently mask unexpected property exceptions.",
    suggestion: "Use direct attribute access, a `Protocol` / `TypedDict` interface, `isinstance()` narrowing, or an explicit `dict` lookup instead of `{callee}()`.",
};

/// Rule that bans dynamic attribute reflection in Python files.
pub struct NoDynamicAttributeAccess;

impl Rule for NoDynamicAttributeAccess {
    fn name(&self) -> RuleName {
        RuleName("no-dynamic-attribute-access")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Typing, Tag::Safety, Tag::Opinionated]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeRule for NoDynamicAttributeAccess {
    fn target(&self) -> RuleTarget {
        RuleTarget::All
    }

    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<crate::code_lint::SourceDoc>,
        config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        self.check_banned_calls(path, grep, config, &DEFAULT_BANNED_FUNCTIONS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_code_rule_snapshot;

    #[test]
    fn test_dynamic_attribute_access_detection() {
        let rule = NoDynamicAttributeAccess;

        let source = indoc::indoc! {r#"
            value = getattr(service, "timeout", 10)
            if hasattr(record, field_name):
                setattr(record, field_name, 42)
            delattr(record, "deprecated_key")

            import builtins
            builtins.getattr(service, "name")

            # Custom methods with matching names must NOT be flagged:
            registry.getattr("key")
        "#};
        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, source, "service.py"), @"
        [no-dynamic-attribute-access] Line 1, Col 9: Dynamic reflection call `getattr()` bypasses static type checking.
        [no-dynamic-attribute-access] Line 2, Col 4: Dynamic reflection call `hasattr()` bypasses static type checking.
        [no-dynamic-attribute-access] Line 3, Col 5: Dynamic reflection call `setattr()` bypasses static type checking.
        [no-dynamic-attribute-access] Line 4, Col 1: Dynamic reflection call `delattr()` bypasses static type checking.
        [no-dynamic-attribute-access] Line 7, Col 1: Dynamic reflection call `builtins.getattr()` bypasses static type checking.
        ");
    }
}
