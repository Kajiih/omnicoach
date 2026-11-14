//! Validation checks for unbounded `jj log` revision walks.

use crate::command_lint::command::{
    InterceptedCommand, JJ_CLI_SCHEMA, ParsedCommand, intersect_revset_with_mutable,
    is_bounded_jj_revset,
};
use crate::command_lint::contract::{CommandEnv, CommandRule};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, CommandExample, Consensus, Declaration, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "`{command}` runs `jj log` without bounding the revision walk.",
    rationale: "Open revset ranges, history-wide predicates, and file paths without `-r` bypass `revsets.log` and walk immutable history across the repository.",
    suggestion: "Narrow the query with `mutable()` or `--limit`, such as `{replacement}`.",
};

/// The rule's declaration: flags `jj log` invocations that bypass `revsets.log` without bounding
/// the revision walk via `--limit` / `-n` or a bounded revset expression.
pub const RULE: CommandRule = CommandRule {
    declaration: Declaration {
        name: RuleName("unbounded-jj-log"),
        template: &TEMPLATE,
        languages: &[],
        options: RuleOptions::none(),
        classification: Classification {
            topics: &[Topic::JJ],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags `jj log` invocations that do not bound the revision walk.",
            what_it_does: indoc::indoc! {r"
                Checks `jj log` commands and flags invocations that pass an open revset range
                (such as `::x`, `..x`, `x::`, `x..`, or `root()..x`), a prefix complement (`~x`),
                a history-wide set or predicate (such as `all()`, `mine()`, `author(...)`,
                `description(...)`, `files(...)`, `conflicts()`, or 1-argument `ancestors(x)`), or
                positional file paths without `-r`, unless bounded by `& mutable()`, a two-sided
                non-root range (`main..@`), a depth/count-capped call, or `-n` / `--limit`."},
            why_is_this_bad: indoc::indoc! {r"
                Bare `jj log` uses Jujutsu's default `revsets.log` configuration, which stays
                scoped to recent mutable work and trunk tips. Passing an explicit `-r` expression
                or a positional fileset without a revision bound walks immutable history back to
                `root()`, dumping hundreds or thousands of commits into standard output.

                Intersect open revsets with `& mutable()`, use a two-sided range between non-root
                commits (`main..@`), or cap output with `-n` / `--limit`."},
            known_problems: Some(indoc::indoc! {r"
                Boundedness is checked syntactically without querying the repository graph: open
                descendant ranges on mutable commits (such as `jj log -r @::`) are flagged unless
                intersected with `& mutable()` or paired with `--limit`, and user-defined revset
                aliases that expand to an unbounded revset are treated as single symbols and not
                flagged."}),
            references: &[Reference {
                title: "Jujutsu: Revset language",
                url: "https://jj-vcs.github.io/jj/latest/revsets/",
            }],
            examples: &[CommandExample {
                flagged: "jj log -r ::@",
                flagged_span: "jj log -r ::@",
                fixed: "jj log -r '::@ & mutable()'",
            }],
        },
    },
    check: check_command,
};

fn check_command(
    rule: &CommandRule,
    parsed: &ParsedCommand,
    _env: &CommandEnv<'_>,
) -> Vec<Diagnostic> {
    let source = parsed.source();
    parsed
        .commands()
        .iter()
        .filter_map(|cmd| {
            let replacement = suggest_bounded_jj_log(cmd)?;
            let command_slice = &source[cmd.span.start..cmd.span.end];
            Some(rule.diagnostic_at(
                parsed,
                cmd.span,
                &[("command", command_slice), ("replacement", &replacement)],
            ))
        })
        .collect()
}

fn suggest_bounded_jj_log(cmd: &InterceptedCommand) -> Option<String> {
    if cmd.program_base_name() != JJ_CLI_SCHEMA.program_name {
        return None;
    }
    let args = cmd.parse_args(&JJ_CLI_SCHEMA);
    if !args.has_subcommand_sequence(&["log"]) {
        return None;
    }
    if args.has_flag("-h")
        || args.has_flag("--help")
        || args.has_flag("-V")
        || args.has_flag("--version")
        || args.has_flag("-n")
        || args.has_flag("--limit")
    {
        return None;
    }

    let mut revisions = ["-r", "--revision", "--revisions"]
        .into_iter()
        .flat_map(|flag| args.option_values(flag))
        .map(String::as_str)
        .peekable();

    if revisions.peek().is_none() {
        if args.positionals.len() <= 1 {
            return None;
        }
        let trailing_paths = args.positionals[1..].join(" ");
        return Some(format!("jj log -r 'mutable()' {trailing_paths}"));
    }

    let first_unbounded = revisions.find(|revset| !is_bounded_jj_revset(revset))?;
    let bounded_revset = intersect_revset_with_mutable(first_unbounded);
    Some(format!("jj log -r '{bounded_revset}'"))
}

#[cfg(test)]
crate::test_utils::command_rule_test!(
    RULE,
    jj: [],
    {
        pass: [
            bare_jj_log_uses_default_revset => "jj log",
            bare_jj_log_with_no_pager => "jj log --no-pager",
            short_limit_flag_bounds_open_revset => "jj log -r ::@ -n 10",
            short_glued_limit_flag_bounds_all => "jj log -r 'all()' -n5",
            long_limit_flag_bounds_open_revset => "jj log -r ::@ --limit 20",
            long_equals_limit_flag_bounds_files => "jj log --limit=15 src/lib.rs",
            single_working_copy_revision => "jj log -r @",
            parent_step_revision => "jj log -r @---",
            bookmark_revision => "jj log -r main",
            hyphenated_bookmark_revision => "jj log -r feature-branch-",
            two_sided_range_revision => "jj log -r main..@",
            two_sided_dag_range_revision => "jj log -r main::@",
            mutable_function_revision => "jj log -r 'mutable()'",
            open_range_intersected_with_mutable => "jj log -r '::@ & mutable()'",
            predicate_intersected_with_mutable => "jj log -r 'author(alice) & mutable()'",
            unquoted_glob_hyphen_wildcard_in_mutable => "jj log -r 'description(glob:fix-*) & mutable()'",
            difference_from_mutable_is_bounded => "jj log -r 'mutable() ~ ::main'",
            trunk_cutoff_range_is_bounded => "jj log -r 'trunk()..@'",
            bounded_two_arg_ancestors => "jj log -r 'ancestors(@, 5)'",
            bounded_reachable_in_mutable => "jj log -r 'reachable(@, mutable())'",
            bounded_latest_call => "jj log -r 'latest(all(), 5)'",
            fileset_with_bounded_revset => "jj log -r 'mutable()' src/lib.rs",
            short_help_flag_with_open_revset_exempt => "jj log -r ::@ -h",
            long_help_flag_with_open_revset_exempt => "jj log -r ::@ --help",
            short_version_flag_with_open_revset_exempt => "jj log -r ::@ -V",
            long_version_flag_with_open_revset_exempt => "jj log -r ::@ --version",
            unrelated_jj_subcommand_exempt => "jj diff -r ::@",
            non_jj_command_exempt => "git log",
        ],
        fail: [
            prefix_dag_ancestors => "jj log -r ::@",
            prefix_range_ancestors => "jj log -r ..@",
            postfix_dag_descendants => "jj log -r main::",
            postfix_range_descendants => "jj log -r main..",
            full_dag_operator => "jj log -r ::",
            full_range_operator => "jj log -r ..",
            root_anchored_range => "jj log -r 'root()..@'",
            root_anchored_dag_range => "jj log -r 'root()::@'",
            union_with_root_anchored_dag_range => "jj log -r '(root() | main)::@'",
            none_anchored_range => "jj log -r 'none()..@'",
            prefix_complement => "jj log -r ~@",
            all_function => "jj log -r 'all()'",
            immutable_function => "jj log -r 'immutable()'",
            visible_heads_function => "jj log -r 'visible_heads()'",
            mine_function => "jj log -r 'mine()'",
            author_predicate => "jj log -r 'author(alice)'",
            description_predicate => "jj log -r 'description(fix)'",
            files_predicate => "jj log -r 'files(src/lib.rs)'",
            conflicts_predicate => "jj log -r 'conflicts()'",
            one_arg_ancestors => "jj log -r 'ancestors(@)'",
            one_arg_descendants => "jj log -r 'descendants(main)'",
            union_with_unbounded_branch => "jj log -r '@ | all()'",
            difference_from_unbounded_left => "jj log -r 'all() ~ @'",
            short_equals_revision_flag => "jj log -r=::@",
            long_revision_flag => "jj log --revision ::@",
            long_revisions_equals_flag => "jj log --revisions=::@",
            multiple_revision_flags_one_unbounded => "jj log -r @ -r ::main",
            positional_fileset_without_revset => "jj log src/lib.rs",
            path_qualified_jj_binary => "/usr/bin/jj log -r ::@",
            inside_pipeline => "jj log -r ::@ | head -n 5" => "jj log -r ::@",
            inside_command_substitution => r#"echo "$(jj log -r 'all()')""# => "jj log -r 'all()'",
        ],
    }
);
