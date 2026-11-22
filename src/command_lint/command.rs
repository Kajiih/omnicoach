//! Intercepted shell command parsing and generic CLI argument classification.

architecture_component!(CommandLintCommand);

mod cli_args;
mod shell;

pub(super) use self::cli_args::{
    ProgramCliSchema, SubcommandCliSchema, extract_leading_global_flags,
};
pub use self::shell::{
    CommandArg, CommandRedirect, EnvAssignment, InterceptedCommand, ParsedCommand, RedirectKind,
    RedirectTarget, ShellScope,
};
