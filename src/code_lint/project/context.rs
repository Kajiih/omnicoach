//! Unified cross-language project layout, memoized summary index, rule query context, and
//! multi-file source discovery.

mod discovery;
mod index;
mod layout;
mod query;

#[cfg(test)]
pub(in crate::code_lint) use self::discovery::discover_project_sources_in;
pub(in crate::code_lint) use self::discovery::{
    DiscoveredProjectSources, derive_project_title, discover_project_sources,
    resolve_effective_paths,
};
pub use self::index::ProjectIndex;
pub use self::layout::ProjectLayout;
pub use self::query::ProjectContext;
