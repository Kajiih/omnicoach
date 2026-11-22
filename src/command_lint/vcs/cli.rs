//! Jujutsu (`jj`) CLI option and subcommand schemas.

use crate::command_lint::command::{ProgramCliSchema, SubcommandCliSchema};

macro_rules! jj_global_options_with_values {
    ($($extra:literal,)*) => {
        &[
            $($extra,)*
            "-R",
            "--repository",
            "--at-operation",
            "--at-op",
            "--color",
            "--config",
            "--config-toml",
            "--config-file",
        ]
    };
}

/// Global Jujutsu CLI options that consume a value argument.
const JJ_GLOBAL_OPTIONS_WITH_VALUES: &[&str] = jj_global_options_with_values!();

/// Leading-flag schema for `jj` subcommands (`clap` global options).
pub const JJ_SUBCOMMAND_CLI_SCHEMA: SubcommandCliSchema = SubcommandCliSchema {
    program_name: "jj",
    global_options_with_values: JJ_GLOBAL_OPTIONS_WITH_VALUES,
    standalone_flags: &["-h", "--help", "-V", "--version"],
    allows_plus_toolchain: false,
};

/// CLI schema definition for Jujutsu (`jj`) commands.
pub const JJ_CLI_SCHEMA: ProgramCliSchema = ProgramCliSchema {
    program_name: "jj",
    options_with_values: jj_global_options_with_values!(
        "-r",
        "--revision",
        "--revisions",
        "-n",
        "--limit",
        "-T",
        "--template",
        "--tool",
        "--context",
        "-m",
        "--message",
    ),
};
