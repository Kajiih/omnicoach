//! Two-stage Jujutsu (`jj`) CLI subcommand and option parser.

use std::path::PathBuf;

use super::revset::{intersect_revset_with_mutable, is_bounded_jj_revset};
use crate::command_lint::command::{
    InterceptedCommand, LastOptionValue, ParsedArgs, ProgramCliSchema, SubcommandCliSchema,
};

const JJ_PROGRAM: &str = "jj";
const FLAG_REPO_SHORT: &str = "-R";
const FLAG_REPO_LONG: &str = "--repository";
const FLAG_REV_SHORT: &str = "-r";
const FLAG_REV_LONG: &str = "--revision";
const FLAG_REVS_LONG: &str = "--revisions";
const FLAG_LIMIT_SHORT: &str = "-n";
const FLAG_LIMIT_LONG: &str = "--limit";
const FLAG_HELP_SHORT: &str = "-h";
const FLAG_HELP_LONG: &str = "--help";
const FLAG_VERSION_SHORT: &str = "-V";
const FLAG_VERSION_LONG: &str = "--version";
const WORKING_COPY_REV: &str = "@";

macro_rules! jj_options_with_values {
    ($($extra:expr,)*) => {
        &[
            $($extra,)*
            FLAG_REPO_SHORT,
            FLAG_REPO_LONG,
            "--at-operation",
            "--at-op",
            "--color",
            "--config",
            "--config-toml",
            "--config-file",
        ]
    };
}

/// Global Jujutsu CLI options that consume a following value argument.
const JJ_GLOBAL_OPTIONS_WITH_VALUES: &[&str] = jj_options_with_values!();

/// Standalone top-level flags on `jj` that print help or version without running a subcommand.
const JJ_STANDALONE_FLAGS: &[&str] = &[
    FLAG_HELP_SHORT,
    FLAG_HELP_LONG,
    FLAG_VERSION_SHORT,
    FLAG_VERSION_LONG,
];

/// Leading-flag schema for `jj` subcommands (`clap` global options).
pub const JJ_SUBCOMMAND_CLI_SCHEMA: SubcommandCliSchema = SubcommandCliSchema {
    program_name: JJ_PROGRAM,
    global_options_with_values: JJ_GLOBAL_OPTIONS_WITH_VALUES,
    standalone_flags: JJ_STANDALONE_FLAGS,
    allows_plus_toolchain: false,
};

const GLOBAL_ONLY_SCHEMA: ProgramCliSchema = ProgramCliSchema {
    options_with_values: jj_options_with_values!(),
};

const EDIT_SCHEMA: ProgramCliSchema = ProgramCliSchema {
    options_with_values: jj_options_with_values!(FLAG_REV_SHORT, FLAG_REV_LONG,),
};

const LOG_SCHEMA: ProgramCliSchema = ProgramCliSchema {
    options_with_values: jj_options_with_values!(
        FLAG_REV_SHORT,
        FLAG_REV_LONG,
        FLAG_REVS_LONG,
        FLAG_LIMIT_SHORT,
        FLAG_LIMIT_LONG,
        "-T",
        "--template",
        "--tool",
        "--context",
    ),
};

/// Repository override parsed from `-R` / `--repository` on a `jj` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JjRepository {
    /// No `-R` / `--repository` flag was supplied; inherit the command's working directory.
    Inherited,
    /// A static, non-empty repository path supplied via `-R` / `--repository`.
    Static(PathBuf),
    /// A `-R` / `--repository` flag carried a shell expansion, empty string, or missing operand.
    Unresolvable,
}

/// Parsed Jujutsu CLI invocation with subcommand-specific option classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JjInvocation {
    subcommand: JjSubcommand,
    args: ParsedArgs,
    repository: JjRepository,
}

impl JjInvocation {
    /// Repository override passed via `-R` / `--repository`, or [`JjRepository::Inherited`].
    #[must_use]
    pub const fn repository(&self) -> &JjRepository {
        &self.repository
    }

    /// Target revision of a `jj edit` invocation, defaulting to `"@"` when omitted.
    #[must_use]
    pub fn edit_target_revision(&self) -> Option<&str> {
        if self.subcommand != JjSubcommand::Edit {
            return None;
        }
        Some(
            self.args
                .get_option(FLAG_REV_SHORT)
                .or_else(|| self.args.get_option(FLAG_REV_LONG))
                .or_else(|| self.args.positionals.get(1).map(String::as_str))
                .unwrap_or(WORKING_COPY_REV),
        )
    }

    /// Suggests a bounded `jj log` command when this invocation is an unbounded `jj log`, or
    /// `None` when it is not `jj log` or is already bounded.
    #[must_use]
    pub fn suggest_bounded_log(&self) -> Option<String> {
        if self.subcommand != JjSubcommand::Log
            || self.args.has_flag(FLAG_LIMIT_SHORT)
            || self.args.has_flag(FLAG_LIMIT_LONG)
        {
            return None;
        }

        let mut revisions = [FLAG_REV_SHORT, FLAG_REV_LONG, FLAG_REVS_LONG]
            .into_iter()
            .flat_map(|flag| self.args.option_values(flag))
            .map(String::as_str)
            .peekable();

        if revisions.peek().is_none() {
            if self.args.positionals.len() <= 1 {
                return None;
            }
            let trailing_paths = self.args.positionals[1..].join(" ");
            return Some(format!("jj log -r 'mutable()' {trailing_paths}"));
        }

        let first_unbounded = revisions.find(|revset| !is_bounded_jj_revset(revset))?;
        let bounded_revset = intersect_revset_with_mutable(first_unbounded);
        Some(format!("jj log -r '{bounded_revset}'"))
    }
}

/// Parses `command` as a Jujutsu subcommand invocation using two-stage subcommand dispatch,
/// returning `None` for non-`jj` commands or `--help` / `--version` queries.
#[must_use]
pub fn parse_jj_invocation(command: &InterceptedCommand) -> Option<JjInvocation> {
    if command.program.has_expansion || command.program_base_name() != JJ_PROGRAM {
        return None;
    }

    let subcommand_index = JJ_SUBCOMMAND_CLI_SCHEMA.find_subcommand_index(&command.arguments, 0)?;
    let subcommand_name = command.arguments[subcommand_index].text.as_str();
    let subcommand = JjSubcommand::from_name(subcommand_name);
    let args = command.parse_args(subcommand.schema());
    if JJ_STANDALONE_FLAGS.iter().any(|flag| args.has_flag(flag)) {
        return None;
    }

    let repository = match args.last_option_value(&[FLAG_REPO_SHORT, FLAG_REPO_LONG]) {
        LastOptionValue::Absent => JjRepository::Inherited,
        LastOptionValue::MissingValue => JjRepository::Unresolvable,
        LastOptionValue::Value {
            text,
            has_expansion,
        } => {
            if has_expansion || text.is_empty() {
                JjRepository::Unresolvable
            } else {
                JjRepository::Static(PathBuf::from(text))
            }
        }
    };

    Some(JjInvocation {
        subcommand,
        args,
        repository,
    })
}

/// Recognized Jujutsu subcommands inspected by command lint rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JjSubcommand {
    Edit,
    Log,
    Other,
}

impl JjSubcommand {
    fn from_name(name: &str) -> Self {
        match name {
            "edit" => Self::Edit,
            "log" => Self::Log,
            _ => Self::Other,
        }
    }

    const fn schema(self) -> &'static ProgramCliSchema {
        match self {
            Self::Edit => &EDIT_SCHEMA,
            Self::Log => &LOG_SCHEMA,
            Self::Other => &GLOBAL_ONLY_SCHEMA,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case::edit_positional("jj edit kpqxywon", Some("kpqxywon"), JjRepository::Inherited)]
    #[case::edit_default_working_copy("jj edit", Some("@"), JjRepository::Inherited)]
    #[case::edit_short_rev_and_repo(
        "jj edit -R ../ws2 -r main",
        Some("main"),
        JjRepository::Static(PathBuf::from("../ws2"))
    )]
    #[case::edit_leading_repo_flag(
        "jj --repository=/abs/ws edit @-",
        Some("@-"),
        JjRepository::Static(PathBuf::from("/abs/ws"))
    )]
    #[case::last_repo_flag_wins_across_spellings(
        "jj --repository /first edit -R /second @",
        Some("@"),
        JjRepository::Static(PathBuf::from("/second"))
    )]
    #[case::expanded_repo_flag_unresolvable(
        r#"jj -R "$OTHER_WS" edit @"#,
        Some("@"),
        JjRepository::Unresolvable
    )]
    #[case::tilde_repo_flag_unresolvable(
        "jj -R ~/other edit @",
        Some("@"),
        JjRepository::Unresolvable
    )]
    #[case::empty_repo_flag_unresolvable(
        r#"jj -R "" edit @"#,
        Some("@"),
        JjRepository::Unresolvable
    )]
    #[case::dangling_repo_flag_unresolvable("jj edit @ -R", Some("@"), JjRepository::Unresolvable)]
    #[case::repo_flag_after_terminator_ignored(
        "jj status -- -R /other",
        None,
        JjRepository::Inherited
    )]
    #[case::non_edit_subcommand(
        "jj status -R ../ws2",
        None,
        JjRepository::Static(PathBuf::from("../ws2"))
    )]
    fn test_parse_jj_edit_and_repository(
        #[case] raw: &str,
        #[case] expected_edit_rev: Option<&str>,
        #[case] expected_repo: JjRepository,
    ) {
        let command = InterceptedCommand::parse_all(raw).remove(0);
        let invocation = parse_jj_invocation(&command).expect("valid jj invocation");
        assert_eq!(invocation.edit_target_revision(), expected_edit_rev);
        assert_eq!(invocation.repository(), &expected_repo);
    }

    #[rstest::rstest]
    #[case::unbounded_ancestors("jj log -r ::@", Some("jj log -r '::@ & mutable()'"))]
    #[case::summary_flag_does_not_swallow_fileset(
        "jj log -s src/lib.rs",
        Some("jj log -r 'mutable()' src/lib.rs")
    )]
    #[case::bounded_with_limit("jj log -s -n 5 src/lib.rs", None)]
    #[case::bare_log_allowed("jj log -s", None)]
    fn test_suggest_bounded_log_preserves_boolean_summary_flag(
        #[case] raw: &str,
        #[case] expected_suggestion: Option<&str>,
    ) {
        let command = InterceptedCommand::parse_all(raw).remove(0);
        let invocation = parse_jj_invocation(&command).expect("valid jj invocation");
        assert_eq!(
            invocation.suggest_bounded_log().as_deref(),
            expected_suggestion
        );
    }

    #[rstest::rstest]
    #[case::non_jj("git status")]
    #[case::top_level_help("jj --help")]
    #[case::subcommand_help("jj edit --help")]
    #[case::clustered_short_version("jj -qV status")]
    #[case::flags_only("jj -R /tmp/ws --no-pager")]
    fn test_parse_jj_invocation_skips_non_subcommands(#[case] raw: &str) {
        let command = InterceptedCommand::parse_all(raw).remove(0);
        assert_eq!(parse_jj_invocation(&command), None);
    }
}
