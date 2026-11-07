//! Rust module path normalization and architectural component inheritance (`CodeLintProject`).

architecture_component!(CodeLintProject);

mod layout;

pub use self::layout::{
    InheritedComponent, PathOrigin, ResolvedPath, is_in_module_subtree, is_rust_crate_root,
    normalize_relative_path, resolve_inherited_component_with_root, resolve_rust_reference_path,
    rust_module_path,
};
