//! Validation checks for the `jj edit` command.

use crate::command_lint::InterceptedCommand;
use crate::command_lint::vcs::JjClient;
use crate::core::{Config, Rule, RuleName};
use crate::diagnostic::{
    Diagnostic, SourceLocation, SourceSpan, ViolationTemplate, violation_template,
};
use crate::rules::Tag;

use crate::command_lint::ProgramCliSchema;

/// CLI schema definition for Jujutsu commands.
const JJ_CLI_SCHEMA: ProgramCliSchema = ProgramCliSchema {
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
fn extract_jj_edit_revision(cmd: &InterceptedCommand) -> Option<String> {
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

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Running `jj edit {revision}` on a described commit is discouraged.",
    rationale: "Editing described commits breaks atomicity and review stability.",
    suggestion: "Create a new change with `jj new {revision}` instead of editing this commit directly.",
};

/// Blocks running `jj edit <revision>` if the target revision has a non-empty description.
pub struct NoJJEditOnDescribedCommits;

impl Rule for NoJJEditOnDescribedCommits {
    fn name(&self) -> RuleName {
        RuleName("no-edits-on-described-commits")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Workflow, Tag::Vcs, Tag::JJ]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
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

        vec![self.render_diagnostic(
            &[("revision", &revision)],
            SourceLocation::virtual_span(
                crate::diagnostic::VCS_CONTEXT_NAME,
                &cmd.raw_string,
                SourceSpan::new(cmd.span.0, cmd.span.1),
            ),
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
        insta::assert_snapshot!(output, @"[no-edits-on-described-commits] Line 1, Col 1: Running `jj edit d123` on a described commit is discouraged.");

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

    #[rstest::rstest]
    #[case("jj edit", Some("@"))]
    #[case("jj edit rev", Some("rev"))]
    #[case("jj -R . edit rev", Some("rev"))]
    #[case("jj edit --ignore-working-copy rev", Some("rev"))]
    #[case("jj log", None)]
    fn test_extract_jj_edit_revision_helper(
        #[case] command_line: &str,
        #[case] expected_revision: Option<&str>,
    ) {
        let cmd = InterceptedCommand::parse_all(command_line).remove(0);
        assert_eq!(
            extract_jj_edit_revision(&cmd),
            expected_revision.map(ToString::to_string)
        );
    }
}
