//! Multi-file project layout, symbol indexing, fact extraction, and architectural conformance.

architecture_component!(CodeLintProject);

mod context;
mod foundation;
mod python;
mod rust;

#[cfg(test)]
pub(super) use self::context::discover_project_sources_in;
pub(super) use self::context::{
    DiscoveredProjectSources, derive_project_title, discover_project_sources,
    resolve_effective_paths,
};
#[cfg(test)]
pub(super) use self::foundation::CrossModuleRefFact;
pub(crate) use self::foundation::{
    ArchitectureConfig, compile_glob, is_rooted_path, lexical_normalize, resolve_against_cwd,
};
pub(super) use self::foundation::{
    ComponentFact, InvalidArchitectureComponentKind, ModuleFact, OverlayFileSource, ProjectFacts,
    ProjectSourceFile,
};
pub(crate) use self::python::PythonModuleContext;
#[cfg_attr(
    not(test),
    expect(
        unused_imports,
        reason = "re-exported so PythonModuleContext return types remain nameable across crate callers"
    )
)]
pub(super) use self::python::{DocumentedOverrideMethod, ResolvedClassAncestors};
pub(super) use self::python::{ImportedProtocolSummary, PythonClassId};

pub use self::context::{ProjectContext, ProjectIndex, ProjectLayout};
pub use self::foundation::{
    ArchitectureConfigError, ArchitectureError, ArchitectureSpec, CONFIG_FILE_NAME, ComponentSpec,
    ContextConfig, DiskFileSource, FileSource, MemoryFileSource, ProjectConfig, SubmoduleSpec,
};
