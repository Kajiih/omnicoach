//! Validation checks for the `jj edit` command.

use crate::command_lint::vcs::JjClient;
use crate::command_lint::InterceptedCommand;
use crate::core::{Config, Rule};
use crate::diagnostic::{
    Diagnostic, LocationContext, SourceLocation, SourceSpan, ViolationMessage,
};

use crate::command_lint::ProgramCliSchema;

/// CLI schema definition for Jujutsu commands.
pub const JJ_CLI_SCHEMA: ProgramCliSchema = ProgramCliSchema {
    program_name: "jj",
    options_with_values: &[
        "-R",
        "--repository",
        "--config",
        "--config-file",
        "--at-operation",
        "--at-op",
        "--color",
    ],
};

/// Helper to extract revision from a `jj edit` command.
///
/// It ignores global options (e.g. `jj -R . edit`) and options passed to the `edit` subcommand
/// (e.g. `jj edit --ignore-working-copy revision`).
#[must_use]
pub fn extract_jj_edit_revision(cmd: &InterceptedCommand) -> Option<String> {
    let args = cmd.parse_args(&JJ_CLI_SCHEMA);
    if !args.has_subcommand_sequence(&["edit"]) {
        return None;
    }
    if args.positionals.len() > 1 {
        Some(args.positionals[1].clone())
    } else {
        Some("@".to_string())
    }
}

use crate::diagnostic::{RuleCode, RuleName};
use crate::rules::Tag;

/// VCS001: Blocks running `jj edit <revision>` if the target revision has a non-empty description.
pub struct NoJJEditOnDescribedCommits;

/// Violation attributes for `NoJJEditOnDescribedCommits` rule.
pub struct NoJJEditViolation {
    /// The target revision argument of the command.
    pub revision: String,
}

impl NoJJEditOnDescribedCommits {
    /// Formats the human-readable diagnostic message.
    #[must_use]
    pub fn format_message(violation: &NoJJEditViolation) -> ViolationMessage {
        let revision = &violation.revision;
        ViolationMessage {
            summary: format!("Running `jj edit {revision}` on a described commit is discouraged."),
            rationale: "Editing described commits breaks atomicity and review stability.".to_string(),
            suggestion: format!("Create a new change with `jj new {revision}` instead of editing this commit directly."),
        }
    }
}

impl Rule for NoJJEditOnDescribedCommits {
    fn code(&self) -> RuleCode {
        RuleCode("VCS001")
    }
    fn name(&self) -> RuleName {
        RuleName("no-edits-on-described-commits")
    }
    fn tags(&self) -> &'static [Tag] {
        &[Tag::Workflow, Tag::Vcs, Tag::JJ]
    }
}

impl crate::command_lint::CommandRule for NoJJEditOnDescribedCommits {
    fn check_command(
        &self,
        cmd: &InterceptedCommand,
        jj_client: &dyn JjClient,
        _config: &Config,
    ) -> Vec<Diagnostic> {
        if cmd.program_base_name() != "jj" {
            return Vec::new();
        }

        let Some(revision) = extract_jj_edit_revision(cmd) else {
            return Vec::new();
        };

        // Guard: Query target revision description
        // TODO(roadmap): Propagate or report VCS client query errors rather than silently returning empty diagnostics.
        let Ok(desc) = jj_client.get_commit_description(&revision) else {
            return Vec::new();
        };
        if desc.is_empty() {
            return Vec::new();
        }

        let violation = NoJJEditViolation { revision };
        let message = Self::format_message(&violation);

        vec![Diagnostic::new(
            self.code(),
            self.name(),
            message,
            SourceLocation {
                context: LocationContext::Virtual {
                    name: crate::diagnostic::VCS_CONTEXT_NAME.to_string(),
                    content: cmd.raw_string.clone(),
                },
                span: SourceSpan {
                    start: cmd.span.0,
                    end: cmd.span.1,
                },
            },
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MockJjClient {
        descriptions: HashMap<String, String>,
    }

    impl JjClient for MockJjClient {
        fn get_commit_description(&self, revision: &str) -> Result<String, String> {
            Ok(self.descriptions.get(revision).cloned().unwrap_or_default())
        }
    }

    #[test]
    fn test_jj_edit_described_commit_blocked() {
        let mut descriptions = HashMap::new();
        descriptions.insert("d123".to_string(), "Implement feature X".to_string());
        descriptions.insert("a456".to_string(), String::new());

        let jj_client = MockJjClient { descriptions };
        let config = Config::default();

        // Block described commit edit
        let output = crate::test_utils::assert_command_rule_snapshot(
            &NoJJEditOnDescribedCommits,
            "jj edit d123",
            &jj_client,
            &config,
        );
        insta::assert_snapshot!(output, @r###"
        [VCS001] Line 1, Col 1: Running `jj edit d123` on a described commit is discouraged.
        "###);

        // Allow empty/anonymous commit edit
        let output_allowed = crate::test_utils::assert_command_rule_snapshot(
            &NoJJEditOnDescribedCommits,
            "jj edit a456",
            &jj_client,
            &config,
        );
        assert!(output_allowed.is_empty());

        // Allow unrelated commands
        let output_log = crate::test_utils::assert_command_rule_snapshot(
            &NoJJEditOnDescribedCommits,
            "jj log -r d123",
            &jj_client,
            &config,
        );
        assert!(output_log.is_empty());
    }

    #[test]
    fn test_extract_jj_edit_revision_helper() {
        let cmd = InterceptedCommand::parse_all("jj edit").remove(0);
        assert_eq!(extract_jj_edit_revision(&cmd), Some("@".to_string()));

        let cmd = InterceptedCommand::parse_all("jj edit rev").remove(0);
        assert_eq!(extract_jj_edit_revision(&cmd), Some("rev".to_string()));

        let cmd = InterceptedCommand::parse_all("jj -R . edit rev").remove(0);
        assert_eq!(extract_jj_edit_revision(&cmd), Some("rev".to_string()));

        let cmd = InterceptedCommand::parse_all("jj edit --ignore-working-copy rev").remove(0);
        assert_eq!(extract_jj_edit_revision(&cmd), Some("rev".to_string()));

        let cmd = InterceptedCommand::parse_all("jj log").remove(0);
        assert_eq!(extract_jj_edit_revision(&cmd), None);
    }
}
