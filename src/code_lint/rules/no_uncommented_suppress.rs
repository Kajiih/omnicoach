//! Rule: `no-uncommented-suppress`
//!
//! Enforces that calls to `contextlib.suppress(...)` or `suppress(...)` used as context managers
//! in Python `with` statements are accompanied by an adjacent explanatory comment
//! documenting why ignoring the exception is benign.

architecture_component!(CodeLintRules);

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::is_with_context_manager;
use crate::code_lint::rule::CodeRule;
use crate::core::{Config, EnforcementMode, FilterListDefaults, LanguageDefaults, Rule, Tag};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
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
    summary: "Exception suppression `suppress(...)` has no explanatory comment.",
    rationale: "Silently swallowing exceptions without documenting why the failure is benign hides unexpected bugs and leaves maintainers unable to distinguish intentional ignoring from accidental masking.",
    suggestion: "Add an inline or directly preceding `# comment` explaining why the suppressed exception is safe to ignore.",
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
    fn check_file(&self, path: &Path, file: &ParsedFile, config: &Config) -> Vec<Diagnostic> {
        self.find_configured_banned_calls(file, config, &DEFAULT_BANNED_CALLS)
            .into_iter()
            .filter(|matched| is_with_context_manager(&matched.node))
            .map(|matched| self.diagnostic_at_node(path, &matched.node, &[]))
            .collect()
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    NoUncommentedSuppress,
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
