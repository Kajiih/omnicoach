//! Project layout, module header index, cross-module symbol/MRO resolution, and architectural conformance.

architecture_component!(CodeLintProject);

mod architecture;
mod config;
mod context;
mod index;
mod layout;
mod source;

pub use self::architecture::{
    ArchitectureError, ArchitectureSpec, ComponentSpec, CrossComponentRelativePathFinding,
    ExportPattern, FileArchitectureFindings, InvalidArchitectureComponentFinding,
    InvalidArchitectureComponentKind, PrivateComponentImportFinding,
    SiblingModuleDependencyFinding, UndeclaredComponentDependencyFinding,
    UnmappedArchitectureModuleFinding, UnownedExternalDependencyFinding,
};
pub(crate) use self::config::compile_glob;
pub use self::config::{
    ArchitectureComponentConfig, ArchitectureConfig, ArchitectureConfigError, CONFIG_FILE_NAME,
    ContextConfig, ProjectConfig, is_rooted_path, lexical_normalize,
};
pub use self::context::ProjectContext;
pub use self::index::{
    ImportedProtocolSummary, InheritedDocstringMatch, ProjectIndex, PythonClassId,
    ResolvedClassAncestors, ResolvedPythonTarget,
};
pub use self::layout::{
    InheritedComponent, PathOrigin, ProjectLayout, ROOT_MODULE_TAG, ResolvedPath,
    ResolvedRelativePackage, is_in_module_subtree, is_python_package_init, is_rust_crate_root,
    normalize_relative_path, resolve_inherited_component_with_root, resolve_relative_python_module,
    resolve_relative_python_package, resolve_rust_reference_path, rust_module_from_relative,
    rust_module_path,
};
pub use self::source::{DiskFileSource, FileSource, MemoryFileSource, OverlayFileSource};
