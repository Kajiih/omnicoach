//! Static file analysis and code linting domain.

pub mod ast;
pub mod rule;
pub mod rules;
pub mod runner;
pub(crate) mod semantic;
pub(crate) mod suppression;
