//! Language-agnostic project foundation: virtual/disk file sources, `.omnicoach.toml` schema,
//! architectural DAG model, and multi-file project fact containers (`CodeLintProjectFoundation`).

architecture_component!(CodeLintProjectFoundation);

mod architecture;
mod config;
mod facts;
mod source;

pub(in crate::code_lint) use self::architecture::{
    CrossComponentRelativePathFinding, FileArchitectureFindings,
    InvalidArchitectureComponentFinding, InvalidArchitectureComponentKind,
    PrivateComponentImportFinding, ReexportedPublicItemFinding, SiblingModuleDependencyFinding,
    UndeclaredComponentDependencyFinding, UnmappedArchitectureModuleFinding,
};
pub(in crate::code_lint::project) use self::architecture::{
    ExportPattern, ROOT_MODULE_TAG, build_architecture_spec, deduplicate_findings,
};
pub(crate) use self::config::{
    ArchitectureComponentConfig, ArchitectureConfig, ProjectTomlError, compile_glob,
    is_rooted_path, lexical_normalize, resolve_against_cwd,
};
pub(in crate::code_lint::project) use self::config::{WorkspaceAnchor, normalize_relative_path};
#[cfg(test)]
pub(in crate::code_lint) use self::facts::ComponentFact;
pub(in crate::code_lint) use self::facts::{
    CrossModuleRefFact, ModuleFact, ProjectFacts, ProjectSourceFile,
};
pub(in crate::code_lint::project) use self::facts::{
    KnownModulePrefix, ProjectGraphScope, ResolvedModuleTarget, SplitModulePath,
    longest_known_module_prefix, resolve_unique_module_paths, split_module_and_segments,
};
pub(in crate::code_lint) use self::source::OverlayFileSource;
pub(in crate::code_lint::project) use self::source::is_vcs_boundary;

pub use self::architecture::{ArchitectureError, ArchitectureSpec, ComponentSpec};
pub use self::config::{ArchitectureConfigError, CONFIG_FILE_NAME, ContextConfig, ProjectConfig};
pub use self::source::{DiskFileSource, FileSource, MemoryFileSource};
