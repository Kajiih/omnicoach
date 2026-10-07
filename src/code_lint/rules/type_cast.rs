//! Flags `typing.cast` calls in Python source files.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::collections::HashSet;
use std::path::Path;

const BANNED: ListOption = ListOption {
    kind: ListKind::Deny,
    doc: "Cast functions flagged when called.",
    default: FilterListDefaults {
        base: &["cast", "typing.cast", "typing_extensions.cast"],
        extend: &[],
        remove: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "A value is cast with `{callee}()`.",
    rationale: "A cast makes the type checker accept the target type without any runtime check, so a wrong assumption upstream is carried forward silently.",
    suggestion: "Narrow the type at runtime with `isinstance()` or a `TypeGuard`, or model the contract with a `Protocol`.",
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("type-cast"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(BANNED),
        classification: Classification {
            topics: &[Topic::TYPE_CHECKER_BYPASS],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags `typing.cast` calls in Python source files.",
            what_it_does: indoc::indoc! {r#"
                Flags calls to `typing.cast`, `typing_extensions.cast` and a bare `cast` in Python
                source files; test files are not checked. Calls are matched by how they are written,
                not by where the name was imported from: a bare `cast(...)` is flagged even if
                `cast` comes from another library, while a method call such as
                `pl.col("a").cast(pl.Int64)` is not."#},
            why_is_this_bad: indoc::indoc! {r"
                `cast` tells the type checker to trust a type without checking it, at analysis time
                or at runtime. If the value is not what the cast claims, the error surfaces later
                and far from its cause, and the type checker can no longer help find it. A cast also
                stays silently wrong when the surrounding code changes.

                Narrow the type with a check the type checker understands: `isinstance()`, a
                `TypeGuard` or `TypeIs` function, or a `Protocol` that describes the contract."},
            references: &[Reference {
                title: "Python docs: typing.cast",
                url: "https://docs.python.org/3/library/typing.html#typing.cast",
            }],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r#"
                    def load_port(config: dict[str, object]) -> int:
                        return cast(int, config["port"])
                "#},
                flagged_span: r#"cast(int, config["port"])"#,
                fixed: indoc::indoc! {r#"
                    def load_port(config: dict[str, object]) -> int:
                        port = config["port"]
                        if not isinstance(port, int):
                            raise TypeError(f"port must be an int, got {port!r}")
                        return port
                "#},
            }],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

fn check_file(
    rule: &CodeRule<ListOption>,
    path: &Path,
    file: &ParsedFile,
    banned: &HashSet<String>,
) -> Vec<Diagnostic> {
    rule.check_banned_calls(path, file, banned)
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                polars_column_cast => r#"
                    df = df.select(pl.col("a").cast(pl.Int64))
                "#,
                custom_method_cast => r#"
                    result = obj.cast("param")
                "#,
                isinstance_narrowing => r#"
                    if isinstance(val, int):
                        x = val
                "#,
                unrelated_imported_cast => r#"
                    from sqlalchemy import cast
                    x = cast(column, Integer)
                "#,
                locally_defined_cast => r#"
                    def cast(value, kind):
                        return kind(value)

                    x = cast(y, int)
                "#,
            ],
            fail: [
                bare_cast => r#"
                    x = cast(int, y)
                "# => r#"cast(int, y)"#,
                typing_qualified_cast => r#"
                    import typing
                    x = typing.cast(list[str], data)
                "# => r#"typing.cast(list[str], data)"#,
                typing_extensions_cast => r#"
                    import typing_extensions
                    x = typing_extensions.cast(int, data)
                "# => r#"typing_extensions.cast(int, data)"#,
                typing_module_alias_cast => r#"
                    import typing as t
                    x = t.cast(int, data)
                "# => r#"t.cast(int, data)"#,
                typing_from_import_alias_cast => r#"
                    from typing import cast as typed
                    x = typed(int, data)
                "# => r#"typed(int, data)"#,
            ],
        },
    }
);
