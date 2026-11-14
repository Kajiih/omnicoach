//! Rust-specific project layout, module header index, architectural conformance evaluation, and
//! project fact extraction (`CodeLintProjectRust`).

architecture_component!(CodeLintProjectRust);

mod architecture;
mod facts;
mod index;
mod layout;
mod paths;

pub(in crate::code_lint::project) use self::facts::collect_rust_project_facts;
pub(in crate::code_lint::project) use self::index::RustIndex;
pub(in crate::code_lint::project) use self::layout::{
    CARGO_TOML as MANIFEST_FILE_NAME, RustLayout,
};
