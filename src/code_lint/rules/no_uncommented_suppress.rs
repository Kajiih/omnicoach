//! Rule: `no-uncommented-suppress`
//!
//! Enforces that calls to `contextlib.suppress(...)` or `suppress(...)` used as context managers
//! in Python `with` statements are accompanied by an adjacent explanatory comment
//! documenting why ignoring the exception is benign.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::is_with_context_manager;
use crate::code_lint::rule::CodeDetector;
use crate::code_lint::semantic::calls;
use crate::core::{
    Detector, EnforcementMode, FilterListDefaults, LanguageDefaults, ListKind, ListOption,
    OptionSpec, ResolvedOptions, RuleOptions,
};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::Rule;
use crate::rule_documentation::{Reference, RuleDoc};
use crate::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Topic};
use ast_grep_language::SupportLang;
use std::path::Path;

const BANNED_CALLS: ListOption = ListOption {
    kind: ListKind::Deny,
    doc: "Exception suppression calls flagged when called.",
    default: FilterListDefaults {
        base: &["suppress", "contextlib.suppress"],
        extend: &[],
        exempt: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Exception suppression `suppress(...)` has no explanatory comment.",
    rationale: "Silently swallowing exceptions without documenting why the failure is benign hides unexpected bugs and leaves maintainers unable to distinguish intentional ignoring from accidental masking.",
    suggestion: "Add an inline or directly preceding `# comment` explaining why the suppressed exception is safe to ignore.",
};

/// Rule struct.
struct NoUncommentedSuppress;

/// The rule's declaration.
pub const RULE: Rule<dyn CodeDetector> = Rule {
    detector: &NoUncommentedSuppress,
    classification: Classification {
        topics: &[Topic::ERROR_HANDLING],
        precision: Precision::Exact,
        consensus: Consensus::Opinionated,
        impacted_quality: ImpactedQuality::Maintainability,
    },
    doc: RuleDoc {
        summary: "Requires a comment explaining each `contextlib.suppress` block.",
        what_it_does: "Flags `suppress(...)` and `contextlib.suppress(...)` used as a context \
                       manager in a Python `with` statement, unless a comment explains it. \
                       The comment can trail the `suppress(...)` line, trail any line of a \
                       multi-line `with (...)` header, or sit in the block of comment lines \
                       directly above the statement. It must be substantive: at least three \
                       words and ten characters, and a bare tool directive such as \
                       `# noqa: SIM105` or `# type: ignore` does not count. Comments inside \
                       the `with` body do not count. A `suppress(...)` call outside a `with` \
                       statement is not flagged. With `enforcement_mode = \"ban\"`, every \
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
    },
    options: RuleOptions {
        enforcement_mode: Some(LanguageDefaults::new(
            EnforcementMode::RequireExplanation,
            &[],
        )),
        options: &[OptionSpec::List(&BANNED_CALLS)],
    },
};

impl Detector for NoUncommentedSuppress {
    fn name(&self) -> RuleName {
        RuleName("no-uncommented-suppress")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeDetector for NoUncommentedSuppress {
    fn check_file(
        &self,
        path: &Path,
        file: &ParsedFile,
        options: &ResolvedOptions<'_>,
    ) -> Vec<Diagnostic> {
        calls::find_banned_calls(file, &options.list(&BANNED_CALLS))
            .into_iter()
            .filter(|matched| is_with_context_manager(&matched.node))
            .map(|matched| self.diagnostic_at_node(path, &matched.node, &[]))
            .collect()
    }
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
