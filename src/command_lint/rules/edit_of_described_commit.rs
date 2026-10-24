//! Validation checks for the `jj edit` command.

use crate::command_lint::command::{InterceptedCommand, ParsedCommand, ProgramCliSchema};
use crate::command_lint::contract::{CommandEnv, CommandRule};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, CommandExample, Consensus, Declaration, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};

/// CLI schema definition for Jujutsu commands.
const JJ_CLI_SCHEMA: ProgramCliSchema = ProgramCliSchema {
    program_name: "jj",
    options_with_values: &[
        "-r",
        "--revision",
        "-R",
        "--repository",
        "--config",
        "--config-toml",
        "--config-file",
        "--at-operation",
        "--at-op",
        "--color",
    ],
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "`jj edit {revision}` targets a described commit.",
    rationale: "Editing a described commit rewrites reviewed history in place and tangles unrelated work into an existing change.",
    suggestion: "Create a child change with `jj new {revision}`, then move the intentional fixes with `jj squash`.",
};

/// The rule's declaration: blocks running `jj edit <revision>` if the target revision has a
/// non-empty description and has no unresolved conflicts.
pub const RULE: CommandRule = CommandRule {
    declaration: Declaration {
        name: RuleName("edit-of-described-commit"),
        template: &TEMPLATE,
        languages: &[],
        options: RuleOptions::none(),
        classification: Classification {
            topics: &[Topic::JJ],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags `jj edit` on a commit that already has a description.",
            what_it_does: indoc::indoc! {r"
                Checks shell commands before they run and flags `jj edit <revision>` when the
                target revision already has a description and has no unresolved conflicts."},
            why_is_this_bad: indoc::indoc! {r"
                In Jujutsu, a described commit is usually finished work, often already reviewed.
                Editing it makes every later file change part of that commit, silently: unrelated
                work gets tangled into it and reviewed history is rewritten in place.

                Create a child change with `jj new <revision>`, then move only the intended fixes
                into the commit with `jj squash`."},
            known_problems: Some(indoc::indoc! {r"
                When `jj` cannot read the target's description, such as for a mistyped revision,
                the command is not flagged."}),
            references: &[Reference {
                title: "Jujutsu: Working copy",
                url: "https://jj-vcs.github.io/jj/latest/working-copy/",
            }],
            examples: &[CommandExample {
                flagged: "jj edit kpqxywon",
                flagged_span: "jj edit kpqxywon",
                fixed: "jj new kpqxywon",
            }],
        },
    },
    check: check_command,
};

fn check_command(
    rule: &CommandRule,
    parsed: &ParsedCommand,
    env: &CommandEnv<'_>,
) -> Vec<Diagnostic> {
    parsed
        .commands()
        .iter()
        .filter_map(|cmd| {
            let revision = extract_jj_edit_revision(cmd)?;
            // TODO(roadmap): Propagate or report VCS client query errors rather than silently returning empty diagnostics.
            let commit_info = env.jj().get_commit_info(&revision).ok()?;
            (!commit_info.conflict && !commit_info.description.trim().is_empty())
                .then(|| rule.diagnostic_at(parsed, cmd.span, &[("revision", &revision)]))
        })
        .collect()
}

/// Extracts the target revision from a `jj edit` command, defaulting to `"@"` when omitted.
///
/// Understands global options (such as `-R .` or `--ignore-working-copy`) and the `-r` /
/// `--revision` option accepted by `jj edit`.
#[must_use]
fn extract_jj_edit_revision(cmd: &InterceptedCommand) -> Option<String> {
    if cmd.program_base_name() != JJ_CLI_SCHEMA.program_name {
        return None;
    }
    let args = cmd.parse_args(&JJ_CLI_SCHEMA);
    if !args.has_subcommand_sequence(&["edit"]) {
        return None;
    }
    args.get_option("-r")
        .or_else(|| args.get_option("--revision"))
        .or_else(|| args.positionals.get(1).map(String::as_str))
        .or(Some("@"))
        .map(ToString::to_string)
}

#[cfg(test)]
crate::test_utils::command_rule_test!(
    RULE,
    jj: [
        "@" => crate::command_lint::vcs::CommitInfo {
            description: "Described working copy".to_string(),
            conflict: false,
        },
        "kpqxywon" => crate::command_lint::vcs::CommitInfo {
            description: "feat: add user authentication".to_string(),
            conflict: false,
        },
        "a456" => crate::command_lint::vcs::CommitInfo {
            description: String::new(),
            conflict: false,
        },
        "ws_only" => crate::command_lint::vcs::CommitInfo {
            description: "   \n  ".to_string(),
            conflict: false,
        },
        "c789" => crate::command_lint::vcs::CommitInfo {
            description: "Conflicted commit".to_string(),
            conflict: true,
        },
    ],
    {
        pass: [
            empty_commit_allowed => "jj edit a456",
            whitespace_only_description_allowed => "jj edit ws_only",
            conflicted_described_commit_allowed => "jj edit c789",
            unknown_revision_error_ignored => "jj edit unknown_rev",
            unrelated_jj_subcommand_allowed => "jj log -r kpqxywon",
            non_jj_command_allowed => "git commit -m msg",
            redirect_on_empty_commit_not_treated_as_revision => "jj edit a456 > out.txt",
        ],
        fail: [
            described_commit => "jj edit kpqxywon",
            path_qualified_binary => "/usr/bin/jj edit kpqxywon",
            bare_edit_when_working_copy_described => "jj edit",
            short_revision_flag => "jj edit -r kpqxywon",
            short_glued_revision_flag => "jj edit -rkpqxywon",
            long_revision_flag => "jj edit --revision kpqxywon",
            long_equals_revision_flag => "jj edit --revision=kpqxywon",
            with_global_repository_flag => "jj -R . edit kpqxywon",
            with_ignore_working_copy_flag => "jj edit --ignore-working-copy kpqxywon",
            bare_edit_with_output_redirect => "jj edit > out.txt",
            with_leading_env_assignment => "JJ_USER=alice jj edit kpqxywon",
            inside_pipeline => "jj log | jj edit kpqxywon" => "jj edit kpqxywon",
            inside_command_substitution => r#"echo "$(jj edit kpqxywon)""# => "jj edit kpqxywon",
        ],
    }
);
