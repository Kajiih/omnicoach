//! The command rule contract ([`CommandRule`]): a declaration and the function that checks one
//! intercepted command.

architecture_component!(CommandLintContract);

use crate::command_lint::command::InterceptedCommand;
use crate::command_lint::vcs::JjClient;
use crate::diagnostic::Diagnostic;
use crate::rule_declaration::Declaration;

/// A rule that analyzes intercepted shell commands: its declaration and the function that
/// finds its violations.
///
/// A command rule file exposes one `pub const` `CommandRule`, listed in `COMMAND_RULES`
/// (`src/command_lint/rules.rs`). It declares `languages: &[]`, `RuleOptions::none()`, and
/// `examples: &[]` (command rules have no `rule_test!` harness yet).
#[derive(Clone, Copy)]
pub struct CommandRule {
    /// Name, template, options, classification and doc; command rules analyze no language.
    pub declaration: Declaration,
    /// Finds the rule's violations in one intercepted command.
    pub check: fn(&Self, &InterceptedCommand, &dyn JjClient) -> Vec<Diagnostic>,
}

impl CommandRule {
    /// Finds the rule's violations in `cmd`.
    #[must_use]
    pub fn check_command(
        &self,
        cmd: &InterceptedCommand,
        jj_client: &dyn JjClient,
    ) -> Vec<Diagnostic> {
        (self.check)(self, cmd, jj_client)
    }
}
