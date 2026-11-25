//! The command rule contract ([`CommandRule`]) and execution context ([`CommandContext`], [`CommandEnv`]).

architecture_component!(CommandLintContract);

use std::path::{Path, PathBuf};

use crate::command_lint::command::{InterceptedCommand, ParsedCommand};
use crate::command_lint::vcs::{CachedJjSession, CommitSummary, JjClient, JjRepository};
use crate::diagnostic::{Diagnostic, SourceSpan};
use crate::rule_declaration::{CommandExample, Declaration};

/// Execution metadata describing the environment in which an intercepted command line runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandContext {
    working_dir: PathBuf,
}

impl CommandContext {
    /// Creates a command execution context rooted at `working_dir`.
    #[must_use]
    pub fn new(working_dir: impl Into<PathBuf>) -> Self {
        Self {
            working_dir: working_dir.into(),
        }
    }

    /// Returns the base working directory in which the command line executes.
    #[must_use]
    fn working_dir(&self) -> &Path {
        &self.working_dir
    }
}

/// External execution environment available to command lint rules beyond the parsed shell AST.
pub struct CommandEnv<'a> {
    jj_session: CachedJjSession<'a>,
    context: &'a CommandContext,
}

impl<'a> CommandEnv<'a> {
    /// Creates a command environment backed by `jj_client` and borrowing `context`.
    #[must_use]
    pub(crate) fn new(jj_client: &'a dyn JjClient, context: &'a CommandContext) -> Self {
        Self {
            jj_session: CachedJjSession::new(jj_client),
            context,
        }
    }

    /// Evaluates `revset` against the effective repository directory for `command` (accounting for
    /// in-scope `cd` builtins and any explicit `jj -R` / `--repository` `repository` override).
    ///
    /// # Errors
    ///
    /// Returns an error if the target directory is statically unresolvable or if the underlying
    /// [`JjClient`] fails to evaluate `revset`.
    pub(crate) fn query_jj_revisions(
        &self,
        command: &InterceptedCommand,
        repository: &JjRepository,
        revset: &str,
    ) -> Result<Vec<CommitSummary>, String> {
        let override_dir = match repository {
            JjRepository::Inherited => None,
            JjRepository::Static(path) => Some(path.as_path()),
            JjRepository::Unresolvable => {
                return Err("cannot resolve dynamic repository override".to_owned());
            }
        };
        let repo_dir = command
            .resolve_working_dir(self.context.working_dir(), override_dir)
            .ok_or_else(|| "cannot resolve command working directory".to_owned())?;
        self.jj_session.query_revisions(&repo_dir, revset)
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
    /// Constructs a synthetic [`CommandRule`] for unit tests outside `command_lint`.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn new_for_test(
        declaration: Declaration<(), &'static [CommandExample]>,
        check: fn(&Self, &ParsedCommand, &CommandEnv<'_>) -> Vec<Diagnostic>,
    ) -> Self {
        Self { declaration, check }
    }

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
    pub(super) fn diagnostic_at(
        &self,
        parsed: &ParsedCommand,
        span: SourceSpan,
        params: &[(&str, &str)],
    ) -> Diagnostic {
        self.declaration
            .render_diagnostic(params, parsed.to_source_location(span))
    }
}
