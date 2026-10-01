//! Validation checks for the `jj edit` command.

// TODO: Consider if we should replace this rule with a no edit on bookmarked commit?

use crate::command_lint::rule::{InterceptedCommand, ProgramCliSchema};
use crate::command_lint::vcs::JjClient;
use crate::core::{Config, Detector};
use crate::diagnostic::{
    Diagnostic, RuleName, SourceLocation, SourceSpan, ViolationTemplate, violation_template,
};
use crate::rule_documentation::{Reference, RuleDoc};
use crate::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Topic};

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
    summary: "Command `jj edit {revision}` targets a non-empty described commit.",
    rationale: "Directly mutating an already-described commit rewrites reviewed history in place and risks tangling unrelated work into an existing change.",
    suggestion: "Create a child change on top with `jj new {revision}` and squash intentional fixes selectively via `jj squash`.",
};

/// Blocks running `jj edit <revision>` if the target revision has a non-empty description.
pub struct NoJJEditOnDescribedCommits;

impl NoJJEditOnDescribedCommits {
    /// The rule's declared facets (ADR 007).
    pub(crate) const CLASSIFICATION: Classification = Classification {
        topics: &[Topic::JJ],
        precision: Precision::Exact,
        consensus: Consensus::Opinionated,
        impacted_quality: ImpactedQuality::Reliability,
    };

    /// The rule's user-facing doc.
    pub(crate) const DOC: RuleDoc = RuleDoc {
        summary: "Blocks `jj edit` on a commit that already has a description.",
        what_it_does: "Checks shell commands before they run and flags `jj edit <revision>` \
                       (or a bare `jj edit`, which targets `@`) when the target revision \
                       has a non-empty description. Global options such as `-R` are \
                       understood. Commits without a description can still be edited.",
        why_is_this_bad: "In Jujutsu, a described commit is usually finished work, often \
                          already reviewed. Editing it makes every later file change part \
                          of that commit, silently: unrelated work gets tangled into it and \
                          reviewed history is rewritten in place.\n\n\
                          Create a child change with `jj new <revision>`, then move only \
                          the intended fixes into the commit with `jj squash`.",
        configuration: &[],
        references: &[Reference {
            title: "Jujutsu: Working copy",
            url: "https://jj-vcs.github.io/jj/latest/working-copy/",
        }],
    };
}

impl Detector for NoJJEditOnDescribedCommits {
    fn name(&self) -> RuleName {
        RuleName("no-edits-on-described-commits")
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl crate::command_lint::rule::CommandDetector for NoJJEditOnDescribedCommits {
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

        let (start, end) = cmd.span;
        vec![self.render_diagnostic(
            &[("revision", &revision)],
            SourceLocation::virtual_span(
                crate::diagnostic::VCS_CONTEXT_NAME,
                &cmd.raw_string,
                SourceSpan::new(start, end),
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
        insta::assert_snapshot!(output, @"[no-edits-on-described-commits] Line 1, Col 1: Command `jj edit d123` targets a non-empty described commit.");

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
