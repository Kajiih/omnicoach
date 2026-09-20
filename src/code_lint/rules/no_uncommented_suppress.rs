//! Rule: `no-uncommented-suppress`
//!
//! Enforces that calls to `contextlib.suppress(...)` or `suppress(...)` used as context managers
//! in Python `with` statements are accompanied by an adjacent explanatory comment
//! documenting why ignoring the exception is benign.

use crate::code_lint::ast_python::{find_enclosing_with_item, find_enclosing_with_statement};
use crate::code_lint::comments::CommentIndex;
use crate::code_lint::{CodeRule, SourceDoc};
use crate::core::{Config, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

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

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeRule for NoUncommentedSuppress {
    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<SourceDoc>,
        _config: &Config,
    ) -> Vec<Diagnostic> {
        let root = grep.root();
        let calls: Vec<_> = root
            .find_all("suppress($$$ARGS)")
            .chain(root.find_all("contextlib.suppress($$$ARGS)"))
            .collect();

        if calls.is_empty() {
            return Vec::new();
        }

        let comment_index = CommentIndex::from_ast(grep);
        let mut diagnostics = Vec::new();

        for call in calls {
            // Verify that this call is actually used as a context manager expression in a with statement
            let Some(with_item) = find_enclosing_with_item(&call) else {
                continue;
            };

            let Some(with_stmt) = find_enclosing_with_statement(&call) else {
                continue;
            };

            // Check whether the suppress call, its with-item, or the enclosing with-statement
            // is documented by an adjacent explanatory comment.
            // Note: with_stmt spans the entire with body; inspect preceding comments directly above
            // with_stmt, or inline comments on the with header lines before the body begins.
            let header_end_line = with_stmt.field("body").map_or_else(
                || with_stmt.end_pos().line() + 1,
                |body| body.start_pos().line(),
            );
            let with_start_line = with_stmt.start_pos().line() + 1;

            let is_documented = comment_index.has_explanation_for_node(&call)
                || comment_index.has_explanation_for_node(&with_item)
                || comment_index.has_adjacent_explanation(with_start_line)
                || (with_start_line..=header_end_line)
                    .any(|target_line| comment_index.has_inline_explanation(target_line));

            if !is_documented {
                diagnostics.push(self.diagnostic_at_node(path, &call, &[]));
            }
        }

        diagnostics
    }
}

// TODO: A lot of the test suite can be made more ricr with best practices (parameterized, etc)
#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn test_uncommented_suppress_flagged() {
        let source_violating = indoc! {r#"
            with suppress(FileNotFoundError):
                os.remove("tmp.txt")
        "#};

        let output = crate::test_utils::assert_code_rule_snapshot(
            &NoUncommentedSuppress,
            source_violating,
            "test.py",
        );
        insta::assert_snapshot!(output, @"[no-uncommented-suppress] Line 1, Col 6: Exception suppression must include an explanatory comment.");
    }

    #[test]
    fn test_contextlib_qualified_uncommented_suppress_flagged() {
        let source_violating = indoc! {r#"
            with contextlib.suppress(KeyError):
                data = cache["missing"]
        "#};

        let output = crate::test_utils::assert_code_rule_snapshot(
            &NoUncommentedSuppress,
            source_violating,
            "test.py",
        );
        insta::assert_snapshot!(output, @"[no-uncommented-suppress] Line 1, Col 6: Exception suppression must include an explanatory comment.");
    }

    #[test]
    fn test_inline_comment_explanation_allowed() {
        let source_ok = indoc! {r#"
            with suppress(FileNotFoundError):  # Safe to ignore if temp file was already deleted
                os.remove("tmp.txt")
        "#};

        let output = crate::test_utils::assert_code_rule_snapshot(
            &NoUncommentedSuppress,
            source_ok,
            "test.py",
        );
        assert!(output.is_empty());
    }

    #[test]
    fn test_preceding_comment_block_allowed() {
        let source_ok = indoc! {r#"
            # The background worker cleans up stale lock files,
            # so ignoring FileNotFoundError is safe here.
            with suppress(FileNotFoundError):
                os.remove("lock.txt")
        "#};

        let output = crate::test_utils::assert_code_rule_snapshot(
            &NoUncommentedSuppress,
            source_ok,
            "test.py",
        );
        assert!(output.is_empty());
    }

    #[test]
    fn test_multiline_parenthesized_with_allowed() {
        let source_ok = indoc! {r#"
            with (
                open("log.txt") as log,
                suppress(KeyError),  # Config key is optional in legacy environments
            ):
                process(log)
        "#};

        let output = crate::test_utils::assert_code_rule_snapshot(
            &NoUncommentedSuppress,
            source_ok,
            "test.py",
        );
        assert!(output.is_empty());
    }

    #[test]
    fn test_multiline_parenthesized_with_preceding_comment_allowed() {
        let source_ok = indoc! {r"
            # Optional cleanup of lock file if created
            with (
                suppress(FileNotFoundError),
            ):
                pass
        "};

        let output = crate::test_utils::assert_code_rule_snapshot(
            &NoUncommentedSuppress,
            source_ok,
            "test.py",
        );
        assert!(output.is_empty());
    }

    #[test]
    fn test_nested_with_statement_no_outer_false_positive() {
        let source = indoc! {r#"
            with file_lock:
                # File may already have been removed by another thread
                with suppress(FileNotFoundError):
                    os.remove("cache.bin")
        "#};

        let output =
            crate::test_utils::assert_code_rule_snapshot(&NoUncommentedSuppress, source, "test.py");
        assert!(output.is_empty());
    }

    #[test]
    fn test_directive_only_comment_flagged() {
        let source_violating = indoc! {r"
            with suppress(FileNotFoundError):  # noqa: SIM105
                pass
        "};

        let output = crate::test_utils::assert_code_rule_snapshot(
            &NoUncommentedSuppress,
            source_violating,
            "test.py",
        );
        insta::assert_snapshot!(output, @"[no-uncommented-suppress] Line 1, Col 6: Exception suppression must include an explanatory comment.");
    }

    #[test]
    fn test_too_short_comment_flagged() {
        let source_violating = indoc! {r"
            with suppress(FileNotFoundError):  # ignore
                pass
        "};

        let output = crate::test_utils::assert_code_rule_snapshot(
            &NoUncommentedSuppress,
            source_violating,
            "test.py",
        );
        insta::assert_snapshot!(output, @"[no-uncommented-suppress] Line 1, Col 6: Exception suppression must include an explanatory comment.");
    }

    #[test]
    fn test_suppress_call_outside_with_ignored() {
        let source = indoc! {r"
            # Suppress object passed as an argument or assigned
            mgr = suppress(FileNotFoundError)
        "};

        let output =
            crate::test_utils::assert_code_rule_snapshot(&NoUncommentedSuppress, source, "test.py");
        assert!(output.is_empty());
    }

    #[test]
    fn test_multiline_suppress_with_trailing_inline_comment() {
        let source = indoc! {r"
            with suppress(
                FileNotFoundError,
            ):  # Safe if lock file was already deleted
                pass
        "};

        let output =
            crate::test_utils::assert_code_rule_snapshot(&NoUncommentedSuppress, source, "test.py");
        assert!(output.is_empty());
    }

    #[test]
    fn test_multiline_suppress_with_argument_comment() {
        let source = indoc! {r"
            with suppress(
                FileNotFoundError,  # Lock file may already be removed by daemon
            ):
                pass
        "};

        let output =
            crate::test_utils::assert_code_rule_snapshot(&NoUncommentedSuppress, source, "test.py");
        assert!(output.is_empty());
    }

    #[test]
    fn test_noqa_with_plain_words_explanation() {
        let source = indoc! {r"
            with suppress(FileNotFoundError):  # noqa: SIM105 safe because transient cache
                pass
        "};

        let output =
            crate::test_utils::assert_code_rule_snapshot(&NoUncommentedSuppress, source, "test.py");
        assert!(output.is_empty());
    }

    #[test]
    fn test_body_inline_comment_does_not_suppress_violation() {
        let source = indoc! {r#"
            with suppress(FileNotFoundError):
                os.remove("tmp.txt")  # inline comment inside body
        "#};

        let output =
            crate::test_utils::assert_code_rule_snapshot(&NoUncommentedSuppress, source, "test.py");
        insta::assert_snapshot!(output, @"[no-uncommented-suppress] Line 1, Col 6: Exception suppression must include an explanatory comment.");
    }

    #[test]
    fn test_multiline_parenthesized_with_header_inline_comment() {
        let source = indoc! {r"
            with (
                open('log.txt'),
                suppress(FileNotFoundError),
            ):  # Safe if lock file was already deleted
                pass
        "};

        let output =
            crate::test_utils::assert_code_rule_snapshot(&NoUncommentedSuppress, source, "test.py");
        assert!(output.is_empty());
    }
}
