//! Project layout, source discovery, module header index, multi-file `ProjectFacts`, cross-module
//! symbol/MRO resolution, and architectural conformance.

architecture_component!(CodeLintProject);

mod context;
mod foundation;
mod python;
mod rust;

#[cfg(test)]
pub(in crate::code_lint) use self::context::discover_project_sources_in;
pub(in crate::code_lint) use self::context::{
    DiscoveredProjectSources, derive_project_title, discover_project_sources,
    resolve_effective_paths,
};
pub(crate) use self::foundation::{ArchitectureConfig, compile_glob};
#[cfg(test)]
pub(in crate::code_lint) use self::foundation::{ComponentFact, CrossModuleRefFact};
pub(in crate::code_lint) use self::foundation::{
    InvalidArchitectureComponentKind, ModuleFact, ProjectFacts, ProjectSourceFile,
};

pub use self::context::{ProjectContext, ProjectIndex, ProjectLayout};
pub use self::foundation::{
    ArchitectureConfigError, ArchitectureError, ArchitectureSpec, CONFIG_FILE_NAME, ComponentSpec,
    ContextConfig, DiskFileSource, FileSource, MemoryFileSource, OverlayFileSource, ProjectConfig,
    ProjectTomlError, is_rooted_path, lexical_normalize, resolve_against_cwd,
};
pub use self::python::{
    DocumentedOverrideMethod, ImportedProtocolSummary, PythonClassId, ResolvedClassAncestors,
};
pub use self::rust::{InheritedComponent, resolve_inherited_component_with_root};
