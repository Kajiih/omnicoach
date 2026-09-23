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

use crate::code_lint::{CodeRule, RuleTarget, SourceDoc};
use crate::core::{Config, FilterListDefaults, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
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

impl Rule for NoTypingCast {
    fn name(&self) -> RuleName {
        RuleName("no-typing-cast")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Typing]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeRule for NoTypingCast {
    fn target(&self) -> RuleTarget {
        RuleTarget::SourceOnly
    }

    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<SourceDoc>,
        config: &Config,
    ) -> Vec<Diagnostic> {
        self.check_banned_calls(path, grep, config, &DEFAULT_BANNED_CALLS)
    }
}

#[cfg(test)]
crate::rule_test!(
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
                unrelated_call => r#"
                    print("hello world")
                "#,
                isinstance_narrowing => r#"
                    if isinstance(val, int):
                        x = val
                "#,
            ],
            fail: [
                bare_cast => r#"
                    x = cast(int, y)
                "# => [r#"cast(int, y)"#],
                typing_qualified_cast => r#"
                    import typing
                    x = typing.cast(list[str], data)
                "# => [r#"typing.cast(list[str], data)"#],
                typing_extensions_cast => r#"
                    import typing_extensions
                    x = typing_extensions.cast(int, data)
                "# => [r#"typing_extensions.cast(int, data)"#],
                multiple_uncommented_casts => r#"
                    a = cast(int, x)
                    b = typing.cast(str, y)
                "# => [
                    r#"cast(int, x)"#,
                    r#"typing.cast(str, y)"#,
                ],
            ],
        },
    }
);
