//! The command rule contract ([`CommandRule`]) and execution environment ([`CommandEnv`]).

architecture_component!(CommandLintContract);

use crate::command_lint::command::ParsedCommand;
use crate::command_lint::vcs::JjClient;
use crate::diagnostic::{Diagnostic, SourceSpan};
use crate::rule_declaration::{CommandExample, Declaration};

/// External execution environment available to command lint rules beyond the parsed shell AST.
pub struct CommandEnv<'a> {
    jj_client: &'a dyn JjClient,
}

impl<'a> CommandEnv<'a> {
    /// Creates a command environment backed by `jj_client`.
    #[must_use]
    pub const fn new(jj_client: &'a dyn JjClient) -> Self {
        Self { jj_client }
    }

    /// Returns the Jujutsu repository query client.
    #[must_use]
    pub fn jj(&self) -> &dyn JjClient {
        self.jj_client
    }
}

/// A rule that analyzes intercepted shell commands: its declaration and the function that
/// finds its violations.
///
/// A command rule file exposes one `pub const` `CommandRule`, listed in `COMMAND_RULES`
/// (`src/command_lint/rules.rs`) and verified by `command_rule_test!`.
#[derive(Clone, Copy)]
pub struct CommandRule {
    /// Name, template, options, classification, and user-facing documentation.
    pub declaration: Declaration<(), &'static [CommandExample]>,
    /// Finds the rule's violations in a parsed shell command line.
    pub(super) check: fn(&Self, &ParsedCommand, &CommandEnv<'_>) -> Vec<Diagnostic>,
}

impl CommandRule {
    /// Finds the rule's violations in `parsed`.
    #[must_use]
    pub(crate) fn check_command(
        &self,
        parsed: &ParsedCommand,
        env: &CommandEnv<'_>,
    ) -> Vec<Diagnostic> {
        (self.check)(self, parsed, env)
    }

    /// Renders a [`Diagnostic`] at `span` in `parsed` using this rule's violation template.
    #[must_use]
    pub fn diagnostic_at(
        &self,
        parsed: &ParsedCommand,
        span: SourceSpan,
        params: &[(&str, &str)],
    ) -> Diagnostic {
        self.declaration
            .render_diagnostic(params, parsed.to_source_location(span))
    }
}

#[cfg(test)]
const PURE_SHELL_TEMPLATE: crate::diagnostic::ViolationTemplate = crate::diagnostic::violation_template! {
    summary: "redundant `{program} -c` wrapper",
    rationale: "Wrapping a command in `sh -c` or `bash -c` adds an extra shell layer.",
    suggestion: "Run `{inner}` directly without `{program} -c`.",
};

#[cfg(test)]
const PURE_SHELL_RULE: CommandRule = CommandRule {
    declaration: Declaration {
        name: crate::diagnostic::RuleName("redundant-shell-wrapper"),
        template: &PURE_SHELL_TEMPLATE,
        languages: &[],
        options: crate::rule_declaration::RuleOptions::none(),
        classification: crate::rule_declaration::Classification {
            topics: &[crate::rule_declaration::Topic::JJ],
            precision: crate::rule_declaration::Precision::Exact,
            consensus: crate::rule_declaration::Consensus::Opinionated,
            impacted_quality: crate::rule_declaration::ImpactedQuality::Maintainability,
        },
        doc: crate::rule_declaration::RuleDoc {
            summary: "Flags redundant `sh -c` or `bash -c` command wrappers.",
            what_it_does: "Checks shell commands for `sh -c` or `bash -c` wrappers.",
            why_is_this_bad: "An extra shell wrapper obscures the executed command.",
            known_problems: None,
            references: &[],
            examples: &[CommandExample {
                flagged: "bash -c 'cargo test'",
                flagged_span: "bash -c 'cargo test'",
                fixed: "cargo test",
            }],
        },
    },
    check: |rule, parsed, _env| {
        parsed
            .commands()
            .iter()
            .filter_map(|command| {
                let program = command.program_name();
                let is_shell = matches!(program, "sh" | "bash");
                let [first_arg, second_arg] = command.arguments.as_slice() else {
                    return None;
                };
                if !is_shell || first_arg.text != "-c" {
                    return None;
                }
                Some(rule.diagnostic_at(
                    parsed,
                    command.span,
                    &[("program", program), ("inner", &second_arg.text)],
                ))
            })
            .collect()
    },
};

#[cfg(test)]
crate::test_utils::command_rule_test!(PURE_SHELL_RULE, {
    pass: [
        direct_command => "cargo build",
        non_shell_with_dash_c => "python3 -c 'print(1)'",
        bash_script_with_arg => "bash script.sh arg1",
    ],
    fail: [
        bash_dash_c_wrapper => "bash -c 'cargo check'",
        sh_dash_c_in_chain => "echo ok && sh -c 'cargo clippy'" => "sh -c 'cargo clippy'",
    ],
});
