//! Rule: `no-uncommented-suppress`
//!
//! Enforces that calls to `contextlib.suppress(...)` or `suppress(...)` used as context managers
//! in Python `with` statements are accompanied by an adjacent explanatory comment
//! documenting why ignoring the exception is benign.

use crate::code_lint::ast_python::is_with_context_manager;
use crate::code_lint::{CodeRule, SourceDoc};
use crate::core::{Config, EnforcementMode, FilterListDefaults, LanguageDefaults, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

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
        self.find_configured_banned_calls(grep, config, &DEFAULT_BANNED_CALLS)
            .into_iter()
            .filter(|matched| is_with_context_manager(&matched.node))
            .map(|matched| self.diagnostic_at_node(path, &matched.node, &[]))
            .collect()
    }
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
