//! Project layout, source discovery, module header index, multi-file `ProjectFacts`, cross-module
//! symbol/MRO resolution, and architectural conformance.

architecture_component!(CodeLintProject);

mod architecture;
mod config;
mod context;
mod facts;
mod index;
mod layout;
mod source;

pub use self::architecture::{
    ArchitectureError, ArchitectureSpec, ComponentSpec, CrossComponentRelativePathFinding,
    ExportPattern, FileArchitectureFindings, InvalidArchitectureComponentFinding,
    InvalidArchitectureComponentKind, PrivateComponentImportFinding, ReexportedPublicItemFinding,
    SiblingModuleDependencyFinding, UndeclaredComponentDependencyFinding,
    UnmappedArchitectureModuleFinding, UnownedExternalDependencyFinding,
};
pub(crate) use self::config::compile_glob;
pub use self::config::{
    ArchitectureComponentConfig, ArchitectureConfig, ArchitectureConfigError, CONFIG_FILE_NAME,
    ContextConfig, ProjectConfig, ProjectTomlError, is_rooted_path, lexical_normalize,
};
pub use self::context::{DocumentedOverrideMethod, ProjectContext};
#[cfg(test)]
pub(in crate::code_lint) use self::facts::{
    ComponentFact, CrossModuleRefFact, discover_project_sources_in,
};
pub(in crate::code_lint) use self::facts::{
    DiscoveredProjectSources, ModuleFact, ProjectFacts, ProjectSourceFile, derive_project_title,
    discover_project_sources, resolve_effective_paths,
};
pub use self::index::{
    ImportedProtocolSummary, InheritedDocstringMatch, ProjectIndex, PythonClassId,
    ResolvedClassAncestors,
};
pub use self::layout::{InheritedComponent, ProjectLayout, resolve_inherited_component_with_root};
pub use self::source::{DiskFileSource, FileSource, MemoryFileSource, OverlayFileSource};
