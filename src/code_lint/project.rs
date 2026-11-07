//! Project layout, source abstraction, and architectural DAG conformance (`CodeLintProject`).

architecture_component!(CodeLintProject);

mod architecture;
mod layout;
mod source;

pub use self::architecture::{
    ArchitectureError, ArchitectureSpec, ComponentSpec, CrossComponentRelativePathFinding,
    ExportPattern, FileArchitectureFindings, InvalidArchitectureComponentFinding,
    InvalidArchitectureComponentKind, PrivateComponentImportFinding,
    SiblingModuleDependencyFinding, UndeclaredComponentDependencyFinding,
    UnmappedArchitectureModuleFinding, UnownedExternalDependencyFinding,
    evaluate_file_architecture, evaluate_file_architecture_with_summary,
    evaluate_stray_rust_component_markers, resolve_architecture_spec,
};
pub use self::layout::{
    InheritedComponent, PathOrigin, ProjectLayout, ROOT_MODULE_TAG, ResolvedPath,
    ResolvedRelativePackage, configured_source_roots, is_in_module_subtree, is_python_package_init,
    is_rust_crate_root, normalize_relative_path, resolve_inherited_component_with_root,
    resolve_relative_python_module, resolve_relative_python_package, resolve_rust_reference_path,
    rust_module_from_relative, rust_module_path,
};
pub use self::source::{DiskFileSource, FileSource, MemoryFileSource, OverlayFileSource};
