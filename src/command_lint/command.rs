//! The intercepted command parser: shell input parsed into [`ParsedCommand`] and
//! [`InterceptedCommand`] invocations, plus CLI argument classification ([`ParsedArgs`],
//! [`ProgramCliSchema`]).

architecture_component!(CommandLintCommand);

mod cli_args;
mod shell;

pub use self::cli_args::{
    JJ_CLI_SCHEMA, LeadingGlobalFlags, ParsedArgs, ProgramCliSchema, extract_leading_global_flags,
};
pub use self::shell::{
    CommandArg, CommandRedirect, EnvAssignment, InterceptedCommand, ParsedCommand, RedirectKind,
    RedirectTarget, ShellScope,
};
