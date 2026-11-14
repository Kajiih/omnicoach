//! Intercepted shell command parsing, CLI argument classification, and Jujutsu revset analysis.

architecture_component!(CommandLintCommand);

mod cli_args;
mod revset;
mod shell;

pub use self::cli_args::{
    JJ_CLI_SCHEMA, LeadingGlobalFlags, ParsedArgs, ProgramCliSchema, extract_leading_global_flags,
};
pub use self::revset::{intersect_revset_with_mutable, is_bounded_jj_revset};
pub use self::shell::{
    CommandArg, CommandRedirect, EnvAssignment, InterceptedCommand, ParsedCommand, RedirectKind,
    RedirectTarget, ShellScope,
};
