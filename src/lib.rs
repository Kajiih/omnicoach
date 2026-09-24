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

#[cfg(test)]
pub mod test_utils;
