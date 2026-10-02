//! Flags `contextlib.suppress` blocks, by default only those without an explanatory comment.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::is_with_context_manager;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::code_lint::semantic::calls;
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, EnforcementMode, Example, FilterListDefaults,
    ImpactedQuality, LanguageDefaults, ListKind, ListOption, Precision, Reference, RuleDoc,
    RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::collections::HashSet;
use std::path::Path;

const BANNED: ListOption = ListOption {
    kind: ListKind::Deny,
    doc: "Exception suppression calls flagged when called.",
    default: FilterListDefaults {
        base: &["suppress", "contextlib.suppress"],
        extend: &[],
        remove: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "An exception is silenced with `{callee}()`.",
    rationale: "Silencing an exception without a recorded reason hides unexpected failures and leaves the next reader unable to tell intentional ignoring from accidental masking.",
    suggestion: "Replace the block with an explicit `except` handler, or add a comment on the `{callee}(...)` line explaining why ignoring the exception is safe.",
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("suppressed-exception"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python],
        options: RuleOptions {
            enforcement_mode: Some(LanguageDefaults::new(
                EnforcementMode::RequireExplanation,
                &[],
            )),
            options: BANNED,
        },
        classification: Classification {
            topics: &[Topic::ERROR_HANDLING],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags `contextlib.suppress` blocks, by default only those without an explanatory comment.",
            what_it_does: "Flags `suppress(...)` and `contextlib.suppress(...)` used as a context \
                           manager in a Python `with` statement, unless a comment explains it. \
                           The comment can trail the `suppress(...)` line, trail any line of a \
                           multi-line `with (...)` header, or sit in the block of comment lines \
                           directly above the statement. It must be substantive: at least three \
                           words and ten characters, and a bare tool directive such as \
                           `# noqa: SIM105` or `# type: ignore` does not count. Comments inside \
                           the `with` body do not count. A `suppress(...)` call outside a `with` \
                           statement is not flagged. With `enforcement-mode = \"ban\"`, every \
                           such block is flagged, commented or not.",
            why_is_this_bad: "`suppress` silently discards an exception. The code does not say \
                              why that failure is harmless, so a reader cannot tell an \
                              intentional ignore from a bug being hidden, and a later change \
                              that makes the exception meaningful goes unnoticed.\n\n\
                              State why the exception is safe to ignore in a comment next to \
                              the `with` statement, for example \
                              `# The file may already have been removed by the cleanup job.`",
            references: &[Reference {
                title: "Python docs: contextlib.suppress",
                url: "https://docs.python.org/3/library/contextlib.html#contextlib.suppress",
            }],
            examples: &[Example {
                language: SupportLang::Python,
                flagged: indoc::indoc! {r"
                    with suppress(FileNotFoundError):
                        os.remove(lock_path)
                "},
                flagged_span: "suppress(FileNotFoundError)",
                fixed: indoc::indoc! {r"
                    # The cleanup job may already have removed the lock file.
                    with suppress(FileNotFoundError):
                        os.remove(lock_path)
                "},
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
    banned_calls: &HashSet<String>,
) -> Vec<Diagnostic> {
    calls::find_banned_calls(file, banned_calls)
        .into_iter()
        .filter(|matched| is_with_context_manager(&matched.node))
        .map(|matched| rule.diagnostic_at_node(path, &matched.node, &[("callee", &matched.callee)]))
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                single_line_inline => r#"
                    with suppress(FileNotFoundError):  # Safe to ignore if temp file was already deleted
                        os.remove("tmp.txt")
                "#,
                preceding_comment_block => r#"
                    # The background worker cleans up stale lock files,
                    # so ignoring FileNotFoundError is safe here.
                    with suppress(FileNotFoundError):
                        os.remove("lock.txt")
                "#,
                multiline_parenthesized_with_inline => r#"
                    with (
                        open("log.txt") as log,
                        suppress(KeyError),  # Config key is optional in legacy environments
                    ):
                        process(log)
                "#,
                multiline_parenthesized_with_preceding => r#"
                    # Optional cleanup of lock file if created
                    with (
                        suppress(FileNotFoundError),
                    ):
                        pass
                "#,
                multiline_header_trailing_comment => r#"
                    with (
                        open("log.txt"),
                        suppress(FileNotFoundError),
                    ):  # Safe if lock file was already deleted
                        pass
                "#,
                suppress_call_outside_with_ignored => r#"
                    # Suppress object passed as an argument or assigned
                    mgr = suppress(FileNotFoundError)
                "#,
            ],
            fail: [
                bare_suppress => r#"
                    with suppress(FileNotFoundError):
                        os.remove("tmp.txt")
                "# => "suppress(FileNotFoundError)",
                contextlib_qualified => r#"
                    with contextlib.suppress(KeyError):
                        data = cache["missing"]
                "# => "contextlib.suppress(KeyError)",
                body_inline_comment_does_not_mask => r#"
                    with suppress(FileNotFoundError):
                        os.remove("tmp.txt")  # inline comment inside body
                "# => "suppress(FileNotFoundError)",
                directive_only_comment_does_not_mask => r#"
                    with suppress(FileNotFoundError):  # noqa: SIM105
                        os.remove("tmp.txt")
                "# => "suppress(FileNotFoundError)",
            ],
        },
    }
);
