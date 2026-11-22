//! Jujutsu CLI parsing, revset boundedness analysis, and repository state queries.

architecture_component!(CommandLintVcs);

mod cli;
mod client;
mod revset;

pub(super) use self::cli::{
    JJ_SUBCOMMAND_CLI_SCHEMA, JjInteractivity, JjInvocation, JjRepository, parse_jj_invocation,
};
pub use self::client::{CachedJjSession, CommitSummary, JjCliClient, JjClient};
