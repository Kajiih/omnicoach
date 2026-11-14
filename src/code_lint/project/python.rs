//! Python-specific project layout, module index, cross-module class/MRO context, architectural
//! conformance evaluation, and project fact extraction.

mod architecture;
mod facts;
mod index;
mod layout;
mod paths;
mod query;

pub(in crate::code_lint::project) use self::facts::collect_python_project_facts;
pub(in crate::code_lint::project) use self::index::PythonIndex;
pub(in crate::code_lint::project) use self::layout::{PythonLayout, climb_package_init_chain};

pub use self::index::PythonClassId;
pub use self::query::{
    DocumentedOverrideMethod, ImportedProtocolSummary, PythonModuleContext, ResolvedClassAncestors,
};
