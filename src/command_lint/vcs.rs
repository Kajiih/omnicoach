//! Jujutsu CLI schemas, revset boundedness analysis, and repository state queries.

architecture_component!(CommandLintVcs);

mod cli;
mod client;
mod revset;

pub(super) use self::cli::{JJ_CLI_SCHEMA, JJ_SUBCOMMAND_CLI_SCHEMA};
#[cfg(test)]
pub use self::client::CommitInfo;
pub use self::client::{JjCliClient, JjClient};
pub(super) use self::revset::{intersect_revset_with_mutable, is_bounded_jj_revset};
