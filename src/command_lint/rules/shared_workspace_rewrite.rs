//! Validation checks for Jujutsu commands that rewrite commits shared with another workspace.

use crate::command_lint::command::ParsedCommand;
use crate::command_lint::contract::{CommandEnv, CommandRule};
use crate::command_lint::vcs::parse_jj_invocation;
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, CommandExample, Consensus, Declaration, ImpactedQuality, Reference, RuleDoc,
    RuleOptions, Topic,
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "`{command}` rewrites mutable revision `{revision}` that another Jujutsu workspace builds on.",
    rationale: "Rewriting a commit checked out or inherited by another active workspace leaves that workspace's working copy stale and creates divergent commits when both workspaces snapshot.",
    suggestion: "Create an isolated child change with `jj new` before editing, or coordinate the rewrite from a single workspace and then run `jj workspace update-stale` in the other workspace.",
};

/// The rule's declaration: flags Jujutsu history-rewriting commands whose target mutable commits
/// are checked out by or ancestors of another active workspace's working-copy commit.
pub const RULE: CommandRule = CommandRule {
    declaration: Declaration {
        name: RuleName("shared-workspace-rewrite"),
        template: &TEMPLATE,
        languages: &[],
        options: RuleOptions::none(),
        classification: Classification {
            topics: &[Topic::JJ],
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags Jujutsu history-rewriting commands that mutate a commit another workspace builds on.",
            what_it_does: indoc::indoc! {r"
                Checks Jujutsu history-rewriting commands (`edit`, `commit`, `describe`,
                `metaedit`, `abandon`, `squash`, `split`, `rebase`, `restore`, `absorb`, `fix`,
                `diffedit`, non-`--list` `resolve`, and `new -A` / `-B`) and flags invocations
                whose target revision set contains a mutable commit checked out by or below
                another active workspace's working-copy commit (`working_copies() ~ @`)."},
            why_is_this_bad: indoc::indoc! {r"
                When multiple workspaces share a Jujutsu repository (such as parallel coding agents
                or side-by-side feature worktrees), rewriting a commit that another workspace has
                checked out or builds on immediately makes that other workspace stale. If the other
                workspace is concurrently editing files and snapshots its working copy, Jujutsu
                creates divergent commits with the same change ID and forces manual reconciliation.

                Keep concurrent workspaces on independent child changes (`jj new <base>`), and fold
                or rebase shared ancestor stacks only after coordinating across workspaces and then
                running `jj workspace update-stale` in the sibling workspace."},
            when_to_allow: Some(indoc::indoc! {r"
                When intentionally rebasing or squashing a shared commit stack while no process is
                active in the sibling workspace, followed immediately by
                `jj workspace update-stale` in that sibling workspace."}),
            known_problems: Some(indoc::indoc! {r"
                `jj absorb` and `jj fix` evaluate their candidate mutable revision range statically
                before content-level hunk or formatter matching runs, so they are flagged whenever
                a mutable commit in scope is shared with another workspace even if no lines in that
                commit end up changing.

                Queries inspect at most the 256 most recent commits matching the target revset.
                When two workspaces already share the exact same working-copy commit (`@`),
                rewriting `@` itself is flagged, while rewriting a strict ancestor below that
                shared `@` is not detected by `working_copies() ~ present(@)`."}),
            references: &[Reference {
                title: "Jujutsu: Working copy & Workspaces",
                url: "https://docs.jj-vcs.dev/latest/working-copy/",
            }],
            examples: &[CommandExample {
                flagged: "jj describe -r shared_rev -m 'feat: rewrite shared base'",
                flagged_span: "jj describe -r shared_rev -m 'feat: rewrite shared base'",
                fixed: "jj new shared_rev",
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
            let target_revset = invocation.rewritten_revset()?;
            let commits = env
                .query_jj_revisions(command, invocation.repository(), &target_revset)
                .ok()?;
            let shared_commit = commits.iter().find(|commit| commit.in_other_workspace)?;
            let command_slice = &source[command.span.start..command.span.end];
            Some(rule.diagnostic_at(
                parsed,
                command.span,
                &[
                    ("command", command_slice),
                    ("revision", &shared_commit.change_id),
                ],
            ))
        })
        .collect()
}

#[cfg(test)]
crate::test_utils::command_rule_test!(
    RULE,
    jj: [
        "@" => crate::command_lint::vcs::CommitSummary {
            change_id: "llmqmzku".to_string(),
            in_other_workspace: false,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "@-" => crate::command_lint::vcs::CommitSummary {
            change_id: "private0".to_string(),
            in_other_workspace: false,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "shared_rev" => crate::command_lint::vcs::CommitSummary {
            change_id: "xszkspru".to_string(),
            description: "feat: shared base".to_string(),
            in_other_workspace: true,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "other_ws_head" => crate::command_lint::vcs::CommitSummary {
            change_id: "rrynoqwt".to_string(),
            in_other_workspace: true,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "mixed_stack" => crate::command_lint::vcs::CommitSummary {
            change_id: "llmqmzku".to_string(),
            in_other_workspace: false,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "mixed_stack" => crate::command_lint::vcs::CommitSummary {
            change_id: "xszkspru".to_string(),
            in_other_workspace: true,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "reachable(@, mutable())" => crate::command_lint::vcs::CommitSummary {
            change_id: "xszkspru".to_string(),
            in_other_workspace: true,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "::@ & mutable()" => crate::command_lint::vcs::CommitSummary {
            change_id: "xszkspru".to_string(),
            in_other_workspace: true,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "((main)..(@)) & mutable()" => crate::command_lint::vcs::CommitSummary {
            change_id: "llmqmzku".to_string(),
            in_other_workspace: false,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "((main)..(shared_rev)) & mutable()" => crate::command_lint::vcs::CommitSummary {
            change_id: "xszkspru".to_string(),
            in_other_workspace: true,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "(shared_parent)+ & mutable()" => crate::command_lint::vcs::CommitSummary {
            change_id: "xszkspru".to_string(),
            in_other_workspace: true,
            ..crate::command_lint::vcs::CommitSummary::default()
        },
    ],
    {
        pass: [
            describe_private_working_copy => "jj describe -m 'feat: private change'",
            commit_private_working_copy => "jj commit -m 'feat: private commit'",
            squash_into_private_parent => "jj squash -u",
            squash_onto_shared_commit_allowed => "jj squash -f @ -o shared_rev -m 'feat: new'",
            edit_private_parent => "jj edit @-",
            abandon_private_working_copy => "jj abandon",
            split_private_working_copy => "jj split -m 'part 1' src/lib.rs",
            restore_private_working_copy => "jj restore src/lib.rs",
            restore_from_shared_commit_allowed => "jj restore --from shared_rev src/lib.rs",
            rebase_private_branch_onto_main => "jj rebase -o main",
            rebase_private_source_onto_shared_commit_allowed => "jj rebase -s @ -o shared_rev",
            fix_scoped_to_private_source => "jj fix -s @",
            resolve_list_on_shared_commit_allowed => "jj resolve --list -r shared_rev",
            new_on_shared_commit_allowed => "jj new shared_rev",
            log_on_shared_commit_allowed => "jj log -r shared_rev",
            diff_on_shared_commit_allowed => "jj diff -r shared_rev",
            unknown_revision_error_ignored => "jj abandon unknown_rev",
            help_flag_exempt => "jj rebase -s shared_rev -o main --help",
            non_jj_command_exempt => "git rebase main",
        ],
        fail: [
            edit_other_workspace_head => "jj edit other_ws_head",
            describe_shared_ancestor => "jj describe -r shared_rev -m 'feat: update'",
            desc_alias_positional_shared_ancestor => "jj desc shared_rev -m 'feat: update'",
            metaedit_shared_ancestor => "jj metaedit -r shared_rev --update-author",
            abandon_shared_ancestor => "jj abandon shared_rev",
            abandon_mixed_stack_with_non_first_shared => "jj abandon mixed_stack",
            squash_into_shared_ancestor => "jj squash -u --into shared_rev",
            squash_from_shared_commit => "jj squash -u --from shared_rev --into @",
            split_shared_ancestor => "jj split -r shared_rev -m 'part 1' src/lib.rs",
            restore_into_shared_ancestor => "jj restore --into shared_rev src/lib.rs",
            restore_changes_in_shared_ancestor => "jj restore -c shared_rev",
            resolve_ours_on_shared_ancestor => "jj resolve --tool :ours -r shared_rev",
            diffedit_on_shared_ancestor => "jj diffedit -r shared_rev",
            new_insert_before_shared_ancestor => "jj new -B shared_rev -m 'feat: prep'",
            new_insert_after_shared_parent => "jj new -A shared_parent -m 'fixup!'",
            rebase_source_shared_ancestor => "jj rebase -s shared_rev -o main",
            rebase_revision_shared_ancestor => "jj rebase -r shared_rev -o main",
            rebase_branch_shared_ancestor => "jj rebase -b shared_rev -o main",
            absorb_across_shared_mutable_stack => "jj absorb",
            bare_fix_across_shared_mutable_stack => "jj fix",
            inside_subshell_cd => "(cd ../ws2 && jj abandon shared_rev)" => "jj abandon shared_rev",
        ],
    }
);
