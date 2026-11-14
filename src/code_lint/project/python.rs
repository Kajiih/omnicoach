//! Python-specific project layout, module index, cross-module class/MRO context, architectural
//! conformance evaluation, and project fact extraction (`CodeLintProjectPython`).

architecture_component!(CodeLintProjectPython);

mod architecture;
mod facts;
mod index;
mod layout;
mod paths;
mod query;

pub(in crate::code_lint::project) use self::facts::collect_python_project_facts;
pub(in crate::code_lint) use self::index::PythonClassId;
pub(in crate::code_lint::project) use self::index::PythonIndex;
pub(in crate::code_lint::project) use self::layout::{
    PYPROJECT_TOML as MANIFEST_FILE_NAME, PythonLayout, climb_package_init_chain,
};
pub(in crate::code_lint) use self::query::ImportedProtocolSummary;
pub use self::query::PythonModuleContext;
#[cfg_attr(
    not(test),
    expect(
        unused_imports,
        reason = "re-exported so PythonModuleContext return types remain nameable across crate callers"
    )
)]
pub(in crate::code_lint) use self::query::{DocumentedOverrideMethod, ResolvedClassAncestors};
