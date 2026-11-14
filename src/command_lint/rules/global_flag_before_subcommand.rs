//! Validation checks for global CLI flags placed before a subcommand.

use crate::command_lint::command::{ParsedCommand, extract_leading_global_flags};
use crate::command_lint::contract::{CommandEnv, CommandRule};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, CommandExample, Consensus, Declaration, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "`{command}` places `{flag}` before subcommand `{subcommand}`.",
    rationale: "Leading flags shift the subcommand out of the first argument slot and prevent prefix-based command rules from matching the subcommand.",
    suggestion: "Move global flags after the subcommand: `{replacement}`.",
};

/// The rule's declaration: flags `clap`-based subcommand CLIs (`jj`, `cargo`, `uv`) when global
/// flags appear before the subcommand instead of after it.
pub const RULE: CommandRule = CommandRule {
    declaration: Declaration {
        name: RuleName("global-flag-before-subcommand"),
        template: &TEMPLATE,
        languages: &[],
        options: RuleOptions::none(),
        classification: Classification {
            topics: &[Topic::COMMAND_LINE],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags global CLI flags placed before the subcommand.",
            what_it_does: indoc::indoc! {r"
                Checks shell invocations of `jj`, `cargo`, and `uv` and flags commands where one or
                more global flags appear before the subcommand token, while exempting standalone
                top-level flags such as `--help`, `--version`, `cargo --list`, and
                `cargo --explain`."},
            why_is_this_bad: indoc::indoc! {r"
                Prefix-based command allowlists and shell history filters match the binary and its
                subcommand in the first two tokens (`jj log`, `cargo test`, `uv run`). Placing a
                global flag such as `--no-pager`, `-R .`, or `--locked` before the subcommand
                shifts the subcommand out of the first argument slot, breaking prefix matching
                even though `clap`-based CLIs accept global flags after the subcommand.

                Place the subcommand immediately after the binary (or after a leading `cargo`
                `+toolchain` selector) and pass global flags after the subcommand."},
            known_problems: Some(indoc::indoc! {r"
                Only `clap`-based subcommand CLIs registered in the rule's schema table (`jj`,
                `cargo`, and `uv`) are checked; non-`clap` CLIs such as `git` reject top-level
                flags when placed after the subcommand."}),
            references: &[Reference {
                title: "clap: Arg::global",
                url: "https://docs.rs/clap/latest/clap/struct.Arg.html#method.global",
            }],
            examples: &[CommandExample {
                flagged: "jj --no-pager log -n 5",
                flagged_span: "jj --no-pager log -n 5",
                fixed: "jj log --no-pager -n 5",
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
            let leading = extract_leading_global_flags(cmd)?;
            let command_slice = &source[cmd.span.start..cmd.span.end];
            let replacement = leading.reorder_subcommand_first(cmd, source);
            Some(rule.diagnostic_at(
                parsed,
                cmd.span,
                &[
                    ("command", command_slice),
                    ("flag", &leading.first_flag),
                    ("subcommand", &leading.subcommand),
                    ("replacement", &replacement),
                ],
            ))
        })
        .collect()
}

#[cfg(test)]
crate::test_utils::command_rule_test!(
    RULE,
    jj: [],
    {
        pass: [
            jj_subcommand_first => "jj log --no-pager -n 5",
            jj_repo_after_subcommand => "jj status -R /tmp/ws --no-pager",
            cargo_subcommand_first => "cargo test --locked --lib",
            cargo_plus_toolchain_before_subcommand => "cargo +nightly test --locked",
            uv_subcommand_first => "uv run --directory /tmp/app pytest",
            git_top_level_flag_exempt => "git -C /tmp/repo status",
            git_no_pager_exempt => "git --no-pager log -n 5",
            standalone_help_flag => "jj --help",
            standalone_version_flag => "cargo --version",
            standalone_cargo_list => "cargo --list",
            standalone_cargo_explain => "cargo --explain E0308",
            flags_without_subcommand => "jj -R /tmp/ws --no-pager",
            double_dash_terminator => "jj --no-pager -- log",
        ],
        fail: [
            jj_leading_boolean_flag => "jj --no-pager log -n 5",
            jj_leading_short_option_with_value => "jj -R /tmp/ws status",
            jj_leading_glued_short_option => "jj -R. diff",
            jj_leading_long_equals_option => "jj --repository=/tmp/ws bookmark list",
            jj_multiple_leading_flags => "jj --no-pager --ignore-working-copy -R . log -n 5",
            jj_interspersed_redirect => "jj --no-pager 2>/dev/null log -n 5",
            path_qualified_jj_binary => "/usr/bin/jj --no-pager status",
            cargo_leading_locked_flag => "cargo --locked test --lib",
            cargo_plus_toolchain_then_flag => "cargo +nightly --locked test",
            cargo_leading_short_cluster => "cargo -vC /tmp/crate check",
            uv_leading_boolean_flag => "uv --offline sync",
            uv_leading_option_with_value => "uv --directory /tmp/app run pytest",
            uv_leading_short_directory_option => "uv -C /tmp/app run pytest",
            inside_pipeline => "jj --no-pager log -n 5 | head" => "jj --no-pager log -n 5",
            inside_command_substitution => r#"echo "$(cargo --quiet check)""# => "cargo --quiet check",
        ],
    }
);
