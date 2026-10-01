//! Rule: `no-typing-cast`
//!
//! Flags calls to `typing.cast(...)`, `typing_extensions.cast(...)`, or `cast(...)` in Python production code.
//! `cast()` bypasses static type verification without runtime validation, masking underlying type errors and bugs.
//!
//! By default, `cast` is completely banned (`mode = "ban"`).
//! When configured with `mode = "require-explanation"`, `cast()` is permitted if accompanied by
//! an adjacent explanatory comment.
//! In all modes, legitimate uses can be justified via `# omni:ignore[no-typing-cast] -- <explanation>`.

// TODO: Remove this case with legitimate uses from the documentation because it's the same thing for every rule. Generalize to the whole project.
// TODO: Also remove the documentation of modes as it's the same for all rules
// TODO: Also remove from every violation template, they should not suggest to ignore.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::rule::{CodeDetector, RuleTarget};
use crate::core::{Config, Detector, FilterListDefaults};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_documentation::{ConfigShape, Reference, RuleDoc};
use crate::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Topic};
use ast_grep_language::SupportLang;
use std::path::Path;

/// Static defaults for banned typing cast functions.
const DEFAULT_BANNED_CALLS: FilterListDefaults = FilterListDefaults {
    base: &["cast", "typing.cast", "typing_extensions.cast"],
    extend: &[],
    exempt: &[],
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Type cast call `{callee}()`.",
    rationale: "`typing.cast()` forces the type checker to accept a target type without runtime validation, silently masking type mismatches and upstream bugs.",
    suggestion: "Narrow the type at runtime with `isinstance()` or a `TypeGuard` function, or model the contract with a `Protocol`.",
};

/// Rule struct.
pub struct NoTypingCast;

impl NoTypingCast {
    /// The rule's declared facets.
    pub(crate) const CLASSIFICATION: Classification = Classification {
        topics: &[Topic::TYPE_CHECKER_BYPASS],
        precision: Precision::Exact,
        consensus: Consensus::Opinionated,
        impacted_quality: ImpactedQuality::Reliability,
    };

    /// The rule's user-facing doc.
    pub(crate) const DOC: RuleDoc = RuleDoc {
        summary: "Flags `typing.cast` calls in Python production code.",
        what_it_does: "Flags calls to `typing.cast`, `typing_extensions.cast` and a bare \
                       `cast` in Python source files; test files are not checked. Calls are \
                       matched by how they are written, not by where the name was imported \
                       from: a bare `cast(...)` is flagged even if `cast` comes from another \
                       library, while a method call such as `pl.col(\"a\").cast(pl.Int64)` \
                       is not.",
        why_is_this_bad: "`cast` tells the type checker to trust a type without checking \
                          it, at analysis time or at runtime. If the value is not what the \
                          cast claims, the error surfaces later and far from its cause, and \
                          the type checker can no longer help find it. A cast also stays \
                          silently wrong when the surrounding code changes.\n\n\
                          Narrow the type with a check the type checker understands: \
                          `isinstance()`, a `TypeGuard` or `TypeIs` function, or a \
                          `Protocol` that describes the contract.",
        configuration: &[ConfigShape::DenyList],
        references: &[Reference {
            title: "Python docs: typing.cast",
            url: "https://docs.python.org/3/library/typing.html#typing.cast",
        }],
    };
}

impl Detector for NoTypingCast {
    fn name(&self) -> RuleName {
        RuleName("no-typing-cast")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeDetector for NoTypingCast {
    fn target(&self) -> RuleTarget {
        RuleTarget::SourceOnly
    }

    fn check_file(&self, path: &Path, file: &ParsedFile, config: &Config) -> Vec<Diagnostic> {
        self.check_banned_calls(path, file, config, &DEFAULT_BANNED_CALLS)
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    NoTypingCast,
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
            ],
        },
    }
);
