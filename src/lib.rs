//! Omni Lint Toolkit library.
//!
//! Exposes linter domains for static file analysis and command safety checks.

#[cfg(test)]
#[macro_use]
extern crate pretty_assertions;

/// Declares the [`crate::architecture::ArchitectureComponent`] that the enclosing source file belongs to.
///
/// Validated at compile time by `rustc` (rejecting unknown variants and duplicate declarations
/// within the same module) and inspected by `tests/architecture.rs` to enforce the DAG.
#[doc(hidden)]
#[macro_export]
macro_rules! architecture_component {
    ($component:ident) => {
        const _ARCHITECTURE_COMPONENT: $crate::architecture::ArchitectureComponent =
            $crate::architecture::ArchitectureComponent::$component;
    };
}

/// Builds a `&[ComponentDefinition<Component>]` graph slice from `node => [deps]` edges.
#[doc(hidden)]
#[macro_export]
macro_rules! architecture_graph {
    (@internal_deps) => {
        true
    };
    (@internal_deps no_internal_dependencies) => {
        false
    };
    ($(
        $node:expr => [ $( $dep:expr ),* $(,)? ] $( ($modifier:ident) )?
    ),* $(,)?) => {
        &[
            $(
                $crate::architecture::ComponentDefinition {
                    component: $node,
                    depends_on: &[ $( $dep ),* ],
                    allow_internal_dependencies: $crate::architecture_graph!(@internal_deps $($modifier)?),
                },
            )*
        ]
    };
}

pub mod architecture;
pub mod code_lint;
pub mod command_lint;
pub mod core;
pub mod diagnostic;
pub(crate) mod diff;

#[cfg(test)]
pub mod test_utils;
