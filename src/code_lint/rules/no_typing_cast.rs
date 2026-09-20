//! Rule: `no-typing-cast`
//!
//! Flags calls to `typing.cast(...)`, `typing_extensions.cast(...)`, or `cast(...)` in Python production code.
//! `cast()` bypasses static type verification without runtime validation, masking underlying type errors and bugs.
//!
//! By default, `cast` is completely banned (`mode = "ban"`).
//! When configured with `mode = "require-explanation"`, `cast()` is permitted if accompanied by
//! an adjacent explanatory comment.
//! In all modes, legitimate uses can be justified via `# omni:ignore[no-typing-cast] -- <explanation>`.

use crate::code_lint::comments::CommentIndex;
use crate::code_lint::{CodeRule, RuleTarget, SourceDoc};
use crate::core::{
    Config, DynamicRuleConfig, EnforcementConfig, EnforcementMode, LanguageDefaults, Rule, RuleName,
};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

const DEFAULT_ENFORCEMENT: LanguageDefaults<EnforcementMode> = LanguageDefaults {
    base: EnforcementMode::Ban,
    overrides: &[],
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Unchecked type assertion `{call_name}()` is discouraged.",
    rationale: "`typing.cast()` performs an unchecked assertion that bypasses static type verification without runtime validation.",
    suggestion: "Use structural subtyping (Protocols), runtime type narrowing (`isinstance()`), or domain types instead. If unavoidable, document why with `# omni:ignore[no-typing-cast] -- <reason>`.",
};

/// Dynamic configuration for `NoTypingCast`.
pub type NoTypingCastConfig = DynamicRuleConfig<EnforcementConfig>;

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
        let rule_config: NoTypingCastConfig = config.get_rule_config(self.name().0);
        let mode = rule_config.effective_mode_for_lang(*grep.lang(), &DEFAULT_ENFORCEMENT);

        let root = grep.root();

        let bare_casts = root.find_all("cast($$$ARGS)").map(|node| (node, "cast"));

        let typing_casts = root
            .find_all("typing.cast($$$ARGS)")
            .map(|node| (node, "typing.cast"));

        let typing_ext_casts = root
            .find_all("typing_extensions.cast($$$ARGS)")
            .map(|node| (node, "typing_extensions.cast"));

        let calls = bare_casts.chain(typing_casts).chain(typing_ext_casts);

        let mut comment_index = None;
        let mut diagnostics = Vec::new();

        for (node, call_name) in calls {
            if mode == EnforcementMode::RequireExplanation {
                let index = comment_index.get_or_insert_with(|| CommentIndex::from_ast(grep));
                if index.has_explanation_for_node(&node) {
                    continue;
                }
            }

            diagnostics.push(self.diagnostic_at_node(path, &node, &[("call_name", call_name)]));
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_code_rule_snapshot_with_config;
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
    #[case::commented_cast_still_banned_by_default(
        indoc! {r"
            # Valid reason why cast is safe
            x = cast(int, y)
        "},
        "[no-typing-cast] Line 2, Col 5: Unchecked type assertion `cast()` is discouraged."
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
    fn test_require_explanation_mode_allows_documented_cast() {
        let source_documented = indoc! {r"
            # Valid reason why cast is safe
            x = cast(int, y)
        "};
        let source_uncommented = indoc! {r"
            x = cast(int, y)
        "};

        let config_toml = r#"
            [rules.no-typing-cast]
            mode = "require-explanation"
        "#;
        let config: Config = toml::from_str(config_toml).unwrap();

        let output_doc = assert_code_rule_snapshot_with_config(
            &NoTypingCast,
            source_documented,
            "test.py",
            &config,
        );
        assert!(output_doc.is_empty());

        let output_uncommented = assert_code_rule_snapshot_with_config(
            &NoTypingCast,
            source_uncommented,
            "test.py",
            &config,
        );
        assert_eq!(
            output_uncommented.trim(),
            "[no-typing-cast] Line 1, Col 5: Unchecked type assertion `cast()` is discouraged."
        );
    }

    #[test]
    fn test_skipped_on_test_file() {
        assert_eq!(NoTypingCast.target(), RuleTarget::SourceOnly);
    }
}
