//! Omni Lint Toolkit library.
//!
//! Exposes linter domains for static file analysis and command safety checks.

#[cfg(test)]
#[macro_use]
extern crate pretty_assertions;

pub mod code_lint;
pub mod command_lint;
pub mod core;
pub mod diagnostic;
pub(crate) mod diff;
pub mod rules;

#[cfg(test)]
pub(crate) mod test_utils;
