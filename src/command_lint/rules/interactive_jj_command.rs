//! Validation checks for Jujutsu commands that launch an interactive editor or diff tool.

use crate::command_lint::command::{InterceptedCommand, ParsedCommand};
use crate::command_lint::contract::{CommandEnv, CommandRule};
use crate::command_lint::vcs::{JjInteractivity, JjInvocation, parse_jj_invocation};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, CommandExample, Consensus, Declaration, ImpactedQuality, Reference, RuleDoc,
    RuleOptions, Topic,
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "`{command}` launches an interactive Jujutsu editor or merge session.",
    rationale: "Interactive editors, diff selectors, and merge tools wait on terminal input and hang non-interactive shells and background sessions.",
    suggestion: "Pass explicit non-interactive flags or file paths instead, such as `{replacement}`.",
};

/// The rule's declaration: flags Jujutsu commands that open an interactive text editor, diff
/// editor, or 3-way merge tool instead of running non-interactively.
pub const RULE: CommandRule = CommandRule {
    declaration: Declaration {
        name: RuleName("interactive-jj-command"),
        template: &TEMPLATE,
        languages: &[],
        options: RuleOptions::none(),
        classification: Classification {
            topics: &[Topic::JJ],
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags Jujutsu commands that open an interactive text, diff, or merge editor.",
            what_it_does: indoc::indoc! {r"
                Checks `jj` commands and flags invocations that open `$EDITOR`, an interactive
                diff editor, or an external merge tool:

                - `--editor` on description-editing subcommands (`describe`, `commit`, `split`,
                  `squash`), or `-i` / `--interactive` / `--tool` on diff-editing subcommands
                  (`commit`, `split`, `squash`, `restore`, `absorb`).
                - `jj diffedit`, and `jj resolve` without `-l` / `--list` or `--tool :ours` /
                  `--tool :theirs`.
                - `jj describe` (`desc`) without `-m` / `--message`, `--stdin`, or `--no-edit`,
                  `jj commit` (`ci`) without `-m` / `--message`, and `jj split` without both
                  `-m` / `--message` and at least one `<FILESET>`.
                - `jj squash` without `-u` / `--use-destination-message`, `-m` / `--message`, or
                  `-k` / `--keep-emptied` when two or more combined commits already have non-empty
                  descriptions."},
            why_is_this_bad: indoc::indoc! {r"
                Headless shells, CI pipelines, and background coding agents have no interactive
                terminal attached. Launching `$EDITOR` or a TUI hunk/merge selector blocks until
                the command times out or fails with an editor error.

                Pass explicit message flags (`-m`, `--stdin`, `jj squash -u`), explicit file paths
                (`jj split -m '...' <paths>`, `jj squash -u <paths>`), or non-interactive conflict
                inspection (`jj resolve --list`) so the command completes deterministically."},
            when_to_allow: Some(indoc::indoc! {r"
                When running `omnicoach command` interactively in a local terminal with a
                configured text or diff editor."}),
            known_problems: Some(indoc::indoc! {r"
                When `jj squash <paths>` moves only a subset of modified files and leaves the
                source commit non-empty, Jujutsu keeps the source commit without prompting for a
                combined description; because static command analysis does not compute post-squash
                tree emptiness, `jj squash <paths>` between two described commits is still flagged
                unless `-u`, `-m`, or `-k` is provided.

                When `jj` cannot evaluate a `jj squash` revset (such as for a mistyped revision or
                outside a repository), the command is not flagged. User-defined Jujutsu `[aliases]`
                and less common interactive commands (`jj arrange`, `jj converge`, `jj config edit`,
                `jj sparse edit`, `jj file edit`) are not inspected."}),
            references: &[Reference {
                title: "Jujutsu: Configuration (Editor & Diff Editing)",
                url: "https://docs.jj-vcs.dev/latest/config/#editor",
            }],
            examples: &[CommandExample {
                flagged: "jj describe -r @-",
                flagged_span: "jj describe -r @-",
                fixed: "jj describe -r @- -m 'feat: update change'",
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
            let replacement = find_interactive_replacement(command, &invocation, env)?;
            let command_slice = &source[command.span.start..command.span.end];
            Some(rule.diagnostic_at(
                parsed,
                command.span,
                &[("command", command_slice), ("replacement", replacement)],
            ))
        })
        .collect()
}

fn find_interactive_replacement(
    command: &InterceptedCommand,
    invocation: &JjInvocation,
    env: &CommandEnv<'_>,
) -> Option<&'static str> {
    match invocation.interactivity()? {
        JjInteractivity::AlwaysInteractive { replacement } => Some(replacement),
        JjInteractivity::IfMultipleDescribed {
            revset,
            replacement,
        } => {
            let commits = env
                .query_jj_revisions(command, invocation.repository(), &revset)
                .ok()?;
            let described_count = commits
                .iter()
                .filter(|commit| !commit.description.trim().is_empty())
                .count();
            (described_count >= 2).then_some(replacement)
        }
    }
}

#[cfg(test)]
crate::test_utils::command_rule_test!(
    RULE,
    jj: [
        "@" => crate::command_lint::vcs::CommitSummary {
            change_id: "wpsyytqp".to_string(),
            description: "wip: local draft".to_string(),
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "@-" => crate::command_lint::vcs::CommitSummary {
            change_id: "yywunynk".to_string(),
            description: "feat: parent commit".to_string(),
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "@--" => crate::command_lint::vcs::CommitSummary {
            change_id: "zvquqxyv".to_string(),
            description: "feat: grandparent commit".to_string(),
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "main..@" => crate::command_lint::vcs::CommitSummary {
            change_id: "wpsyytqp".to_string(),
            description: "wip: first commit".to_string(),
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "main..@" => crate::command_lint::vcs::CommitSummary {
            change_id: "yywunynk".to_string(),
            description: "feat: second commit".to_string(),
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "empty_src" => crate::command_lint::vcs::CommitSummary {
            change_id: "aaaa1111".to_string(),
            description: String::new(),
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "empty_src-" => crate::command_lint::vcs::CommitSummary {
            change_id: "bbbb2222".to_string(),
            description: "feat: described parent".to_string(),
            ..crate::command_lint::vcs::CommitSummary::default()
        },
        "empty_dst" => crate::command_lint::vcs::CommitSummary {
            change_id: "cccc3333".to_string(),
            description: "   \n  ".to_string(),
            ..crate::command_lint::vcs::CommitSummary::default()
        },
    ],
    {
        pass: [
            describe_short_message_flag => "jj describe -m 'feat: add parser'",
            describe_long_message_flag => "jj describe --message='feat: add parser' @-",
            describe_stdin_flag => "jj describe --stdin",
            describe_no_edit_flag => "jj describe --no-edit --reset-author",
            desc_alias_short_message_flag => "jj desc -m 'feat: add parser'",
            commit_short_message_flag => "jj commit -m 'feat: add parser' src/lib.rs",
            ci_alias_long_message_flag => "jj ci --message 'feat: add parser'",
            split_message_with_fileset => "jj split -m 'part 1' src/lib.rs",
            squash_use_destination_message_short => "jj squash -u",
            squash_use_destination_message_long => "jj squash --use-destination-message --into @--",
            squash_explicit_message_flag => "jj squash -m 'combined message'",
            squash_keep_emptied_short_flag => "jj squash -k",
            squash_keep_emptied_long_flag => "jj squash --keep-emptied",
            squash_from_undescribed_source_into_described_parent => "jj squash -r empty_src",
            squash_from_described_source_into_undescribed_target => "jj squash --into empty_dst",
            squash_single_source_onto_new_commit => "jj squash -o @--",
            squash_unknown_revision_error_ignored => "jj squash -r unknown_rev",
            restore_non_interactive_paths => "jj restore src/lib.rs",
            absorb_non_interactive_default => "jj absorb",
            resolve_short_list_flag => "jj resolve -l",
            resolve_long_list_flag => "jj resolve --list",
            resolve_builtin_ours_tool => "jj resolve --tool :ours src/lib.rs",
            resolve_builtin_theirs_tool => "jj resolve --tool=:theirs src/lib.rs",
            diff_with_formatter_tool => "jj diff --tool git",
            show_with_formatter_tool => "jj show --tool stat @-",
            new_message_with_editor_text => "jj new -m --editor",
            file_search_pattern_with_short_i => "jj file search -p -i",
            piper_findreviewers_short_i => "jj piper findreviewers -i alice",
            status_subcommand_exempt => "jj status",
            help_flag_exempt => "jj describe --help",
            non_jj_command_exempt => "git commit",
        ],
        fail: [
            bare_describe => "jj describe",
            describe_revision_without_message => "jj describe -r @-",
            desc_alias_without_message => "jj desc @-",
            describe_with_editor_override => "jj describe -m 'feat: x' --editor",
            bare_commit => "jj commit",
            ci_alias_without_message => "jj ci src/lib.rs",
            commit_with_editor_override => "jj commit -m 'feat: x' --editor",
            commit_interactive_short => "jj commit -i -m 'feat: x'",
            commit_with_diff_tool => "jj commit --tool :builtin -m 'feat: x'",
            bare_split => "jj split",
            split_message_without_fileset => "jj split -m 'part 1'",
            split_fileset_without_message => "jj split src/lib.rs",
            split_with_editor_override => "jj split -m 'part 1' --editor src/lib.rs",
            split_interactive_with_fileset => "jj split -i -m 'part 1' src/lib.rs",
            split_with_diff_tool => "jj split --tool :builtin -m 'part 1' src/lib.rs",
            squash_between_two_described_commits => "jj squash",
            squash_into_described_grandparent => "jj squash --into @--",
            squash_from_only_described => "jj squash --from @-",
            squash_three_described_commits => "jj squash -f @ -f @- --into @--",
            squash_multi_source_onto_new_commit => "jj squash -f @ -f @- -o @--",
            squash_single_flag_revset_onto_new_commit => "jj squash -f 'main..@' -o @--",
            squash_with_editor_override => "jj squash -u --editor",
            squash_interactive_short => "jj squash -i -u",
            squash_diff_tool => "jj squash --tool meld -u",
            restore_interactive_long => "jj restore --interactive src/lib.rs",
            restore_diff_tool => "jj restore --tool :builtin src/lib.rs",
            absorb_interactive_short => "jj absorb -i",
            absorb_diff_tool => "jj absorb --tool meld",
            diffedit_subcommand => "jj diffedit -r @",
            bare_resolve => "jj resolve",
            resolve_file_without_tool => "jj resolve src/lib.rs",
            resolve_builtin_editor_tool => "jj resolve --tool :editor src/lib.rs",
            resolve_external_merge_tool => "jj resolve --tool meld src/lib.rs",
            inside_pipeline => "echo wip | jj describe" => "jj describe",
        ],
    }
);
