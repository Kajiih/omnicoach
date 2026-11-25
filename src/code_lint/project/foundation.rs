//! Language-agnostic project foundation: virtual/disk file sources, `.omnicoach.toml` schema,
//! architectural DAG model, and multi-file project fact containers.

mod architecture;
mod config;
mod facts;
mod source;

pub(in crate::code_lint::project) use self::architecture::{
    CrossComponentReference, ROOT_MODULE_TAG, build_architecture_spec, deduplicate_findings,
};
pub(in crate::code_lint) use self::architecture::{
    CrossComponentRelativePathFinding, FileArchitectureFindings,
    InvalidArchitectureComponentFinding, InvalidArchitectureComponentKind,
    ReexportedPublicItemFinding, SiblingModuleDependencyFinding,
    UndeclaredComponentDependencyFinding, UnmappedArchitectureModuleFinding,
};
pub(in crate::code_lint::project) use self::config::{
    ArchitectureComponentConfig, ArchitectureSubmoduleConfig, ProjectTomlError, WorkspaceAnchor,
    normalize_relative_path,
};
pub(in crate::code_lint) use self::facts::{
    ComponentFact, CrossModuleRefFact, ModuleFact, ProjectFacts, ProjectSourceFile,
};
pub(in crate::code_lint::project) use self::facts::{
    KnownModulePrefix, ProjectGraphScope, ResolvedModuleTarget, SplitModulePath,
    longest_known_module_prefix, resolve_unique_module_paths, split_module_and_segments,
};
pub(in crate::code_lint) use self::source::OverlayFileSource;
pub(in crate::code_lint::project) use self::source::is_vcs_boundary;

pub use self::architecture::{ArchitectureError, ArchitectureSpec, ComponentSpec, SubmoduleSpec};
pub use self::config::{
    ArchitectureConfig, ArchitectureConfigError, CONFIG_FILE_NAME, ContextConfig, ProjectConfig,
    compile_glob,
};
pub use self::source::{DiskFileSource, FileSource, MemoryFileSource};
