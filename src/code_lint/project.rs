//! Project layout, source discovery, module header index, multi-file `ProjectFacts`, cross-module
//! symbol/MRO resolution, and architectural conformance.

architecture_component!(CodeLintProject);

mod architecture;
mod context;
mod facts;
mod foundation;
mod index;
mod layout;
mod paths;

#[cfg(test)]
pub(in crate::code_lint) use self::facts::discover_project_sources_in;
pub(in crate::code_lint) use self::facts::{
    DiscoveredProjectSources, derive_project_title, discover_project_sources,
    resolve_effective_paths,
};
pub(crate) use self::foundation::{ArchitectureConfig, compile_glob};
#[cfg(test)]
pub(in crate::code_lint) use self::foundation::{ComponentFact, CrossModuleRefFact};
pub(in crate::code_lint) use self::foundation::{
    InvalidArchitectureComponentKind, ModuleFact, ProjectFacts, ProjectSourceFile,
};

pub use self::architecture::{InheritedComponent, resolve_inherited_component_with_root};
pub use self::context::{DocumentedOverrideMethod, ProjectContext};
pub use self::foundation::{
    ArchitectureConfigError, ArchitectureError, ArchitectureSpec, CONFIG_FILE_NAME, ComponentSpec,
    ContextConfig, DiskFileSource, FileSource, MemoryFileSource, OverlayFileSource, ProjectConfig,
    ProjectTomlError, is_rooted_path, lexical_normalize, resolve_against_cwd,
};
pub use self::index::{
    ImportedProtocolSummary, ProjectIndex, PythonClassId, ResolvedClassAncestors,
};
pub use self::layout::ProjectLayout;
