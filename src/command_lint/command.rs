//! Intercepted shell command parsing and generic CLI argument classification.

architecture_component!(CommandLintCommand);

mod cli_args;
mod model;
mod shell;

pub(super) use self::cli_args::{
    LastOptionValue, ParsedArgs, PreSubcommandSpec, ProgramCliSchema, SubcommandCliSchema,
    extract_leading_global_flags,
};
pub use self::model::{
    CommandArg, CommandRedirect, EnvAssignment, InterceptedCommand, RedirectKind, RedirectTarget,
    ShellScope,
};
pub use self::shell::ParsedCommand;
