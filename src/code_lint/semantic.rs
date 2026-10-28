//! Semantic analysis engines built on top of the syntax adapters.

architecture_component!(CodeLintSemantic);

pub(super) mod bindings;
pub(super) mod calls;
pub(super) mod comments;
pub(super) mod type_annotations;
