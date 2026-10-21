//! Static file analysis and code linting domain.

pub mod ast;
pub mod contract;
pub mod graph;
pub(crate) mod policy;
pub mod rules;
pub mod runner;
mod semantic;
pub mod suppression;
