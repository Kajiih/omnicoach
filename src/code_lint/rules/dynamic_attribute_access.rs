//! Bans dynamic runtime attribute reflection (`getattr`, `hasattr`, `setattr`, `delattr`).

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::collections::HashSet;
use std::path::Path;

const BANNED: ListOption = ListOption {
    kind: ListKind::Deny,
    doc: "Reflection functions flagged when called.",
    default: FilterListDefaults {
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
        remove: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "An attribute is accessed dynamically with `{callee}()`.",
    rationale: "Runtime reflection erases the attribute's static type to `Any`, hides the reference from refactoring tools, and `hasattr` can swallow exceptions raised by properties.",
    suggestion: "Access the attribute directly on a typed object, use a `Mapping` lookup (`dict.get`), or define a structural `Protocol`.",
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("dynamic-attribute-access"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python],
        options: RuleOptions::code_rule(BANNED),
        classification: Classification {
            topics: &[Topic::TYPE_CHECKER_BYPASS],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
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
            references: &[Reference {
                title: "Python docs: built-in getattr",
                url: "https://docs.python.org/3/library/functions.html#getattr",
            }],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

fn check_file(
    rule: &CodeRule<ListOption>,
    path: &Path,
    file: &ParsedFile,
    banned_functions: &HashSet<String>,
) -> Vec<Diagnostic> {
    rule.check_banned_calls(path, file, banned_functions)
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
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
