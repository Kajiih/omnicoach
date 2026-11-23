//! Validation checks for Jujutsu pushes whose mutable ancestor stack contains divergent changes.

use crate::command_lint::command::ParsedCommand;
use crate::command_lint::contract::{CommandEnv, CommandRule};
use crate::command_lint::vcs::parse_jj_invocation;
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, CommandExample, Consensus, Declaration, ImpactedQuality, Reference, RuleDoc,
    RuleOptions, Topic,
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "`{command}` pushes mutable history containing divergent change `{revision}`.",
    rationale: "Pushing while a mutable ancestor has divergent copies either fails Jujutsu remote safety checks or publishes one copy while leaving sibling copies stranded locally.",
    suggestion: "Verify the divergent copies with `jj log -r 'divergent()'` and reconcile or abandon the stale copy (`jj abandon <commit_id>`) before pushing.",
};

/// The rule's declaration: flags `jj git push` invocations whose pushed mutable ancestor stack
/// contains a divergent change.
pub const RULE: CommandRule = CommandRule {
    declaration: Declaration {
        name: RuleName("divergent-jj-push"),
        template: &TEMPLATE,
        languages: &[],
        options: RuleOptions::none(),
        classification: Classification {
            topics: &[Topic::JJ],
            consensus: Consensus::Unopinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags `jj git push` when the pushed mutable commit stack contains a divergent change.",
            what_it_does: indoc::indoc! {r"
                Checks `jj git push` invocations (excluding `--dry-run`, `--deleted`-only, and
                tag-only pushes) by inspecting the mutable ancestor range of the pushed targets
                (`-c`, `-r`, `--named`, `-b` / `--bookmark` / `--branch`, `--all`, `--tracked`, or
                bare `::@ & mutable()`) and flags the command when any commit in range reports
                `self.divergent()`."},
            why_is_this_bad: indoc::indoc! {r"
                Divergent changes arise when two visible commits share the same change ID—commonly
                after concurrent workspace snapshots, stale working-copy updates, or rewrites across
                parallel operations. Pushing before reconciling divergence either fails Jujutsu's
                bookmark safety checks or publishes one copy while leaving the sibling copy behind
                locally.

                Inspect the copies with `jj log -r 'divergent()'`, keep the intended commit or fold
                changes together, and abandon the duplicate commit ID before pushing."},
            when_to_allow: None,
            known_problems: Some(indoc::indoc! {r"
                Bare `jj git push` without explicit `-r`, `-c`, `--named`, `-b`, `--all`, or
                `--tracked` targets checks `::@ & mutable()` so the check works offline even before
                a remote bookmark exists; if an unrelated divergent commit sits below `@` outside
                `revsets.git-push`, pass an explicit `-r`, `-c`, or `-b` target.

                `--tracked` approximates tracked bookmarks with all local `bookmarks()` because
                Jujutsu revsets have no local-side tracked-bookmark predicate, and queries inspect
                at most the 256 most recent commits matching the pushed revset."}),
            references: &[Reference {
                title: "Jujutsu: Pushing Bookmarks Safety Checks",
                url: "https://docs.jj-vcs.dev/latest/bookmarks/#pushing-bookmarks-safety-checks",
            }],
            examples: &[CommandExample {
                flagged: "jj git push",
                flagged_span: "jj git push",
                fixed: "jj abandon stale_commit && jj git push -c clean_rev",
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
    let source = parsed.source();
    parsed
        .commands()
        .iter()
        .filter_map(|command| {
            let invocation = parse_jj_invocation(command)?;
            let pushed_revset = invocation.pushed_mutable_revset()?;
            let commits = env
                .query_jj_revisions(command, invocation.repository(), &pushed_revset)
                .ok()?;
            let divergent_commit = commits.iter().find(|commit| commit.divergent)?;
            let command_slice = &source[command.span.start..command.span.end];
            Some(rule.diagnostic_at(
                parsed,
                command.span,
                &[
                    ("command", command_slice),
                    ("revision", &divergent_commit.change_id),
                ],
            ))
        })
        .collect()
}

#[cfg(test)]
crate::test_utils::command_rule_test!(
    RULE,
    jj: [
        "::@ & mutable()" => crate::command_lint::vcs::CommitSummary {
            change_id: "llmqmzku".to_string(),
            divergent: false,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "::@ & mutable()" => crate::command_lint::vcs::CommitSummary {
            change_id: "kpqxywon".to_string(),
            divergent: true,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "::(clean_rev) & mutable()" => crate::command_lint::vcs::CommitSummary {
            change_id: "llmqmzku".to_string(),
            divergent: false,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "::((clean_rev) & (bookmarks() | tags())) & mutable()" => crate::command_lint::vcs::CommitSummary {
            change_id: "llmqmzku".to_string(),
            divergent: false,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "::(bookmarks(clean-*)) & mutable()" => crate::command_lint::vcs::CommitSummary {
            change_id: "llmqmzku".to_string(),
            divergent: false,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "::(unbookmarked_div) & mutable()" => crate::command_lint::vcs::CommitSummary {
            change_id: "kpqxywon".to_string(),
            divergent: true,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "::((unbookmarked_div) & (bookmarks() | tags())) & mutable()" => crate::command_lint::vcs::CommitSummary {
            change_id: "llmqmzku".to_string(),
            divergent: false,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "::((bookmarked_div) & (bookmarks() | tags())) & mutable()" => crate::command_lint::vcs::CommitSummary {
            change_id: "kpqxywon".to_string(),
            divergent: true,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "::(div_rev) & mutable()" => crate::command_lint::vcs::CommitSummary {
            change_id: "kpqxywon".to_string(),
            divergent: true,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "::(bookmarks(div-*)) & mutable()" => crate::command_lint::vcs::CommitSummary {
            change_id: "kpqxywon".to_string(),
            divergent: true,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "::(bookmarks(clean-* | div-*)) & mutable()" => crate::command_lint::vcs::CommitSummary {
            change_id: "kpqxywon".to_string(),
            divergent: true,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "::(bookmarks()) & mutable()" => crate::command_lint::vcs::CommitSummary {
            change_id: "kpqxywon".to_string(),
            divergent: true,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
    ],
    {
        pass: [
            clean_change_push => "jj git push -c clean_rev",
            clean_revision_push => "jj git push -r clean_rev",
            push_revision_unbookmarked_exempt => "jj git push -r unbookmarked_div",
            clean_named_push => "jj git push --named feat=clean_rev",
            clean_bookmark_glob_push => "jj git push -b clean-*",
            dry_run_long_flag_exempt => "jj git push --dry-run",
            short_tag_only_push_exempt => "jj git push -t v0.5.0",
            long_tag_only_push_exempt => "jj git push --tag v0.5.0",
            deleted_only_push_exempt => "jj git push --deleted",
            git_fetch_not_checked => "jj git fetch",
            help_flag_exempt => "jj git push --help",
            unknown_revision_error_ignored => "jj git push -c unknown_rev",
            non_jj_git_push_exempt => "git push origin main",
        ],
        fail: [
            bare_push_with_non_first_divergent_commit => "jj git push",
            push_change_with_divergent_commit => "jj git push -c unbookmarked_div",
            push_revision_bookmarked_flagged => "jj git push -r bookmarked_div",
            push_named_bookmark_divergent => "jj git push --named feat=div_rev",
            push_bookmark_glob_divergent => "jj git push -b div-*",
            push_branch_alias => "jj git push --branch div-*",
            push_bookmark_union_expr => "jj git push -b 'clean-* | div-*'",
            push_all_bookmarks_divergent => "jj git push --all",
            push_deleted_and_tracked => "jj git push --deleted --tracked",
            push_with_repo_flag_between_tokens => "jj git -R ../ws2 push",
            inside_subshell_cd => "(cd ../ws2 && jj git push)" => "jj git push",
        ],
    }
);
