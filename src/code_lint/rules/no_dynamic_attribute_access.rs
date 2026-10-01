//! Bans dynamic runtime attribute reflection (`getattr`, `hasattr`, `setattr`, `delattr`).

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::rule::{CodeDetector, RuleTarget};
use crate::core::{Detector, FilterListDefaults};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_documentation::{ConfigShape, Reference, RuleDoc};
use crate::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Topic};
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
    summary: "Dynamic attribute reflection call `{callee}()`.",
    rationale: "Runtime attribute reflection erases static attribute types to `Any`, hides symbol references from refactoring tools, and `hasattr` can silently swallow unexpected property exceptions.",
    suggestion: "Access attributes directly on a typed object, use a `Mapping` lookup (`dict.get`), or define a structural `Protocol`.",
};

/// Rule that bans dynamic attribute reflection in Python files.
pub struct NoDynamicAttributeAccess;

impl NoDynamicAttributeAccess {
    /// The rule's declared facets.
    pub(crate) const CLASSIFICATION: Classification = Classification {
        topics: &[Topic::TYPE_CHECKER_BYPASS],
        precision: Precision::Exact,
        consensus: Consensus::Opinionated,
        impacted_quality: ImpactedQuality::Reliability,
    };

    /// The rule's user-facing doc.
    pub(crate) const DOC: RuleDoc = RuleDoc {
        summary: "Flags `getattr`, `hasattr`, `setattr` and `delattr` calls in Python.",
        what_it_does: "Flags calls to the built-in functions `getattr`, `hasattr`, \
                       `setattr` and `delattr`, written bare or as `builtins.getattr` and \
                       so on, in all Python files, tests included. Methods of the same name \
                       on another object, such as `registry.getattr(\"key\")`, are not \
                       flagged.",
        why_is_this_bad: "These functions take the attribute name as a runtime string. The \
                          type checker cannot verify that the attribute exists and usually \
                          types the result as `Any`; renaming tools and \"find \
                          references\" miss the access; a typo fails only at runtime. \
                          `hasattr` also returns `False` when a property raises \
                          `AttributeError` internally, which hides the real bug.\n\n\
                          Access attributes directly on a typed object. For data keyed by \
                          runtime strings, use a `dict` or `Mapping`; to accept several \
                          types that share attributes, declare a `Protocol`.",
        configuration: &[ConfigShape::DenyList],
        references: &[Reference {
            title: "Python docs: built-in getattr",
            url: "https://docs.python.org/3/library/functions.html#getattr",
        }],
    };
}

impl Detector for NoDynamicAttributeAccess {
    fn name(&self) -> RuleName {
        RuleName("no-dynamic-attribute-access")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeDetector for NoDynamicAttributeAccess {
    fn target(&self) -> RuleTarget {
        RuleTarget::All
    }

    fn check_file(
        &self,
        path: &Path,
        file: &ParsedFile,
        config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        self.check_banned_calls(path, file, config, &DEFAULT_BANNED_FUNCTIONS)
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    NoDynamicAttributeAccess,
    {
        Python => {
            pass: [
                direct_attribute_access => r#"
                    value = service.timeout
                "#,
                dict_lookup => r#"
                    email = data.get("email")
                "#,
                custom_receiver_getattr => r#"
                    registry.getattr("key")
                "#,
                custom_receiver_setattr => r#"
                    registry.setattr("key", 1)
                "#,
                custom_receiver_hasattr => r#"
                    registry.hasattr("key")
                "#,
                custom_receiver_delattr => r#"
                    registry.delattr("key")
                "#,
            ],
            fail: [
                unqualified_getattr => r#"
                    val = getattr(service, "timeout", 10)
                "# => r#"getattr(service, "timeout", 10)"#,
                unqualified_hasattr => r#"
                    if hasattr(record, field_name):
                        pass
                "# => r#"hasattr(record, field_name)"#,
                unqualified_setattr => r#"
                    setattr(record, field_name, 42)
                "# => r#"setattr(record, field_name, 42)"#,
                unqualified_delattr => r#"
                    delattr(record, "deprecated_key")
                "# => r#"delattr(record, "deprecated_key")"#,
                builtins_qualified_getattr => r#"
                    builtins.getattr(service, "name")
                "# => r#"builtins.getattr(service, "name")"#,
                builtins_qualified_hasattr => r#"
                    builtins.hasattr(service, "name")
                "# => r#"builtins.hasattr(service, "name")"#,
                builtins_qualified_setattr => r#"
                    builtins.setattr(service, "name", "new_name")
                "# => r#"builtins.setattr(service, "name", "new_name")"#,
                builtins_qualified_delattr => r#"
                    builtins.delattr(service, "name")
                "# => r#"builtins.delattr(service, "name")"#,
            ],
        },
    }
);
