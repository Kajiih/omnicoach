//! Static file analysis and code linting domain.

pub mod ast;
pub(crate) mod bindings;
pub(crate) mod calls;
pub(crate) mod comments;
pub mod rule;
pub mod rules;
pub mod runner;
pub(crate) mod suppression;
