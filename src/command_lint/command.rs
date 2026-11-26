//! Intercepted shell command parsing and generic CLI argument classification.

architecture_component!(CommandLintCommand);

mod cli_args;
mod model;
mod shell;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod wrappers;

pub(super) use self::cli_args::{
    CliSpec, FlagName, LastOptionValue, ParsedCli, PreSubcommandSpec, SubcommandCliSchema,
    extract_leading_global_flags,
};
pub use self::model::{
    CommandArg, CommandRedirect, EnvAssignment, InterceptedCommand, RedirectKind, RedirectTarget,
    ShellScope,
};
pub use self::shell::ParsedCommand;
