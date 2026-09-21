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
    summary: "Unchecked type assertion `{callee}()` is discouraged.",
    rationale: "`typing.cast()` performs an unchecked assertion that bypasses static type verification without runtime validation.",
    suggestion: "Use structural subtyping (Protocols), runtime type narrowing (`isinstance()`), or domain types instead. If unavoidable, document why with `# omni:ignore[no-typing-cast] -- <reason>`.",
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
mod tests {
    use super::*;
    use indoc::indoc;
    use rstest::rstest;

    #[rstest]
    #[case::bare_cast(
        indoc! {r"
            x = cast(int, y)
        "},
        "[no-typing-cast] Line 1, Col 5: Unchecked type assertion `cast()` is discouraged."
    )]
    #[case::typing_qualified_cast(
        indoc! {r"
            import typing
            x = typing.cast(list[str], data)
        "},
        "[no-typing-cast] Line 2, Col 5: Unchecked type assertion `typing.cast()` is discouraged."
    )]
    #[case::typing_extensions_cast(
        indoc! {r"
            import typing_extensions
            x = typing_extensions.cast(int, data)
        "},
        "[no-typing-cast] Line 2, Col 5: Unchecked type assertion `typing_extensions.cast()` is discouraged."
    )]
    fn test_typing_cast_flagged_by_default(#[case] source: &str, #[case] expected: &str) {
        let output = crate::test_utils::assert_code_rule_snapshot(&NoTypingCast, source, "test.py");
        assert_eq!(output.trim(), expected);
    }

    #[rstest]
    #[case::polars_column_cast(indoc! {r"
        df = df.select(pl.col('a').cast(pl.Int64))
    "})]
    #[case::custom_method_cast(indoc! {r"
        result = obj.cast('param')
    "})]
    #[case::unrelated_call(indoc! {r"
        print('hello world')
    "})]
    fn test_unrelated_calls_allowed(#[case] source: &str) {
        let output = crate::test_utils::assert_code_rule_snapshot(&NoTypingCast, source, "test.py");
        assert!(output.is_empty());
    }

    #[test]
    fn test_skipped_on_test_file() {
        assert_eq!(NoTypingCast.target(), RuleTarget::SourceOnly);
    }
}
