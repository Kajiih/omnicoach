//! Rule: `no-uncommented-suppress`
//!
//! Enforces that calls to `contextlib.suppress(...)` or `suppress(...)` used as context managers
//! in Python `with` statements are accompanied by an adjacent explanatory comment
//! documenting why ignoring the exception is benign.

use crate::code_lint::ast_python::{find_enclosing_with_item, find_enclosing_with_statement};
use crate::code_lint::calls::find_banned_calls;
use crate::code_lint::comments::CommentIndex;
use crate::code_lint::{AstNode, CodeRule, SourceDoc};
use crate::core::{
    Config, DenyListConfig, DynamicRuleConfig, EnforcementMode, FilterListDefaults,
    LanguageDefaults, Rule, RuleName,
};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Configuration for the `NoUncommentedSuppress` rule.
pub type NoUncommentedSuppressConfig = DynamicRuleConfig<DenyListConfig>;

/// Static defaults for banned suppress functions.
const DEFAULT_BANNED_CALLS: FilterListDefaults = FilterListDefaults {
    base: &["suppress", "contextlib.suppress"],
    extend: &[],
    exempt: &[],
};

const DEFAULT_ENFORCEMENT: LanguageDefaults<EnforcementMode> = LanguageDefaults {
    base: EnforcementMode::RequireExplanation,
    overrides: &[],
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Exception suppression must include an explanatory comment.",
    rationale: "Silently suppressing exceptions without documenting why the failure is benign obscures unexpected bugs and leaves future maintainers confused.",
    suggestion: "Add a comment directly above or inline with the `suppress(...)` statement explaining why ignoring this exception is safe.",
};

/// Rule struct.
pub struct NoUncommentedSuppress;

impl Rule for NoUncommentedSuppress {
    fn name(&self) -> RuleName {
        RuleName("no-uncommented-suppress")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Exceptions]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }

    fn default_enforcement_mode(&self) -> LanguageDefaults<EnforcementMode> {
        DEFAULT_ENFORCEMENT
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeRule for NoUncommentedSuppress {
    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<SourceDoc>,
        config: &Config,
    ) -> Vec<Diagnostic> {
        let mode = self.enforcement_mode(*grep.lang(), config);
        let rule_config: NoUncommentedSuppressConfig = config.get_rule_config(self.name().0);
        let effective_banned =
            rule_config.effective_banned_for_lang(*grep.lang(), &DEFAULT_BANNED_CALLS);
        let calls = find_banned_calls(grep, &effective_banned);

        let mut comment_index = None;
        let mut diagnostics = Vec::new();

        for call_match in calls {
            let call = &call_match.node;
            let Some(with_stmt) = find_enclosing_with_statement(call) else {
                continue;
            };
            if find_enclosing_with_item(call).is_none() {
                continue;
            }

            let is_documented = if mode == EnforcementMode::Ban {
                false
            } else {
                let index = comment_index.get_or_insert_with(|| CommentIndex::from_ast(grep));
                is_suppression_documented(index, &with_stmt, call)
            };

            if !is_documented {
                diagnostics.push(self.diagnostic_at_node(path, call, &[]));
            }
        }

        diagnostics
    }
}

/// Determines if a `suppress(...)` context manager invocation is documented by an explanatory comment.
///
/// Documentation can be provided in two forms:
/// 1. Standalone comment block directly preceding the `with` statement, or directly
///    preceding the `suppress(...)` call within a multiline header.
/// 2. Inline explanatory comment on any line of the `with` header (from `with` to `:`).
fn is_suppression_documented(
    comment_index: &CommentIndex<'_>,
    with_stmt: &AstNode<'_>,
    call: &AstNode<'_>,
) -> bool {
    let with_start_line = with_stmt.start_pos().line() + 1;
    let call_start_line = call.start_pos().line() + 1;
    let header_end_line = with_stmt.field("body").map_or_else(
        || with_stmt.end_pos().line() + 1,
        |body| body.start_pos().line(),
    );

    comment_index.has_adjacent_explanation(with_start_line)
        || comment_index.has_adjacent_explanation(call_start_line)
        || (with_start_line..=header_end_line)
            .any(|line| comment_index.has_inline_explanation(line))
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use rstest::rstest;

    #[rstest]
    #[case::single_line_inline(indoc! {r#"
        with suppress(FileNotFoundError):  # Safe to ignore if temp file was already deleted
            os.remove("tmp.txt")
    "#})]
    #[case::preceding_comment_block(indoc! {r#"
        # The background worker cleans up stale lock files,
        # so ignoring FileNotFoundError is safe here.
        with suppress(FileNotFoundError):
            os.remove("lock.txt")
    "#})]
    #[case::multiline_parenthesized_with_inline(indoc! {r#"
        with (
            open("log.txt") as log,
            suppress(KeyError),  # Config key is optional in legacy environments
        ):
            process(log)
    "#})]
    #[case::multiline_parenthesized_with_preceding(indoc! {r"
        # Optional cleanup of lock file if created
        with (
            suppress(FileNotFoundError),
        ):
            pass
    "})]
    #[case::multiline_header_trailing_comment(indoc! {r"
        with (
            open('log.txt'),
            suppress(FileNotFoundError),
        ):  # Safe if lock file was already deleted
            pass
    "})]
    #[case::suppress_call_outside_with_ignored(indoc! {r"
        # Suppress object passed as an argument or assigned
        mgr = suppress(FileNotFoundError)
    "})]
    fn test_valid_suppressions_allowed(#[case] source: &str) {
        let output =
            crate::test_utils::assert_code_rule_snapshot(&NoUncommentedSuppress, source, "test.py");
        assert!(output.is_empty());
    }

    #[rstest]
    #[case::bare_suppress(
        indoc! {r#"
            with suppress(FileNotFoundError):
                os.remove("tmp.txt")
        "#},
        "[no-uncommented-suppress] Line 1, Col 6: Exception suppression must include an explanatory comment."
    )]
    #[case::contextlib_qualified(
        indoc! {r#"
            with contextlib.suppress(KeyError):
                data = cache["missing"]
        "#},
        "[no-uncommented-suppress] Line 1, Col 6: Exception suppression must include an explanatory comment."
    )]
    #[case::body_inline_comment_does_not_mask(
        indoc! {r#"
            with suppress(FileNotFoundError):
                os.remove("tmp.txt")  # inline comment inside body
        "#},
        "[no-uncommented-suppress] Line 1, Col 6: Exception suppression must include an explanatory comment."
    )]
    fn test_uncommented_suppress_flagged(#[case] source: &str, #[case] expected: &str) {
        let output =
            crate::test_utils::assert_code_rule_snapshot(&NoUncommentedSuppress, source, "test.py");
        assert_eq!(output.trim(), expected);
    }
}
