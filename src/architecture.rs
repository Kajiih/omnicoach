//! Architectural component catalog and Directed Acyclic Graph (DAG) specification.
//!
//! Defines [`ArchitectureComponent`], [`ComponentDefinition`], and [`ARCHITECTURE_GRAPH`],
//! providing compile-time validation for [`crate::architecture_component!`] declarations
//! across the codebase and the canonical topology for `tests/architecture_conformance.rs`.

architecture_component!(FoundationPrimitives);

use strum::{AsRefStr, Display, EnumString, VariantArray};

/// Bounded specification of an architectural component and its direct dependencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentDefinition<Component: 'static = ArchitectureComponent> {
    /// The target architectural component being defined.
    pub component: Component,
    /// Directly permitted dependency components (transitive dependencies are computed automatically).
    pub depends_on: &'static [Component],
}

/// Builds a `&[ComponentDefinition<Component>]` graph slice from `node => [deps]` edges.
macro_rules! architecture_graph {
    ($(
        $node:expr => [ $( $dep:expr ),* $(,)? ]
    ),* $(,)?) => {
        &[
            $(
                ComponentDefinition {
                    component: $node,
                    depends_on: &[ $( $dep ),* ],
                },
            )*
        ]
    };
}

/// Defines `pub enum ArchitectureComponent` and `pub const ARCHITECTURE_GRAPH`
/// by delegating graph slice construction to `architecture_graph!`.
macro_rules! define_architecture {
    ($(
        $(#[$meta:meta])*
        $node:ident => [ $( $dep:ident ),* $(,)? ]
    ),* $(,)?) => {
        /// Architectural components representing bounded functional units across the codebase.
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Display,
            EnumString,
            AsRefStr,
            VariantArray,
        )]
        pub enum ArchitectureComponent {
            $(
                $(#[$meta])*
                $node,
            )*
        }

        /// The canonical architectural Directed Acyclic Graph (DAG) specification.
        pub const ARCHITECTURE_GRAPH: &[ComponentDefinition<ArchitectureComponent>] =
            architecture_graph! {
                $(
                    ArchitectureComponent::$node => [ $( ArchitectureComponent::$dep ),* ]
                ),*
            };
    };
}

define_architecture! {
    // --- Shared Foundations ---
    /// Zero-dependency foundational primitives (`architecture`, `diagnostic`, `diff`).
    FoundationPrimitives  => [],
    /// What every rule states about itself: name, template, languages, options, classification and doc (`rule_declaration`).
    RuleDeclaration       => [FoundationPrimitives],
    /// Project configuration resolved from `.omnilint.toml` (`config`).
    Config                => [FoundationPrimitives, RuleDeclaration],

    // --- Static Code Analysis Domain (`code_lint`) ---
    /// Encapsulated AST syntax adapters and language parsers (`code_lint::ast`).
    CodeSyntaxAdapters    => [FoundationPrimitives],
    /// Semantic analysis engines (`code_lint::semantic`).
    CodeSemanticEngines   => [CodeSyntaxAdapters],
    /// The code rule contract: a declaration, a file target and a check function (`code_lint::rule`).
    CodeRuleContracts     => [CodeSemanticEngines, CodeSyntaxAdapters, RuleDeclaration],
    /// Inline comment suppression tracker and directive policies (`code_lint::suppression`).
    CodeSuppressionEngine => [CodeRuleContracts, CodeSemanticEngines, RuleDeclaration, Config],
    /// Concrete static analysis linter rules (`code_lint::rules`).
    CodeLintRules         => [CodeRuleContracts, RuleDeclaration],
    /// Static code linting multi-file orchestration runner (`code_lint::runner`).
    CodeLintRunner        => [CodeLintRules, CodeSuppressionEngine],

    // --- Command Safety Domain (`command_lint`) ---
    /// VCS interaction and repository diff adapters (`command_lint::vcs`).
    CommandVcsAdapters    => [FoundationPrimitives],
    /// The command rule contract and intercepted command schemas (`command_lint::rule`).
    CommandRuleContracts  => [CommandVcsAdapters, RuleDeclaration],
    /// Concrete command safety linting rules (`command_lint::rules`).
    CommandLintRules      => [CommandRuleContracts, RuleDeclaration],
    /// Command linting orchestration and interception runner (`command_lint::runner`).
    CommandLintRunner     => [CommandLintRules, Config],

    // --- Rule Selection ---
    /// Taxonomy queries, config selector resolution and rule option validation (`rule_selection`).
    RuleSelection         => [RuleDeclaration, CodeLintRules, CodeSuppressionEngine, CommandLintRules, Config],
    /// Rule list and single-rule Markdown rendering for discovery commands (`rule_catalog`).
    RuleCatalog           => [RuleSelection, RuleDeclaration],

    // --- Test Harness & Entrypoints ---
    /// Test harness and snapshot fixtures (`test_utils`).
    TestingHarness        => [CodeRuleContracts, CommandRuleContracts],
    /// CLI application entrypoint binaries (`src/bin/*`).
    ApplicationBinaries   => [CodeLintRunner, CommandLintRunner, RuleSelection, RuleCatalog],
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Detects cycles in a component graph using depth-first search.
    fn detect_cycle<Component: Copy + Ord + 'static>(
        graph: &[ComponentDefinition<Component>],
    ) -> Option<Vec<Component>> {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum State {
            Unvisited,
            Visiting,
            Visited,
        }

        let mut state = BTreeMap::new();
        for definition in graph {
            state.insert(definition.component, State::Unvisited);
        }

        let mut parent = BTreeMap::new();

        for definition in graph {
            if state.get(&definition.component) == Some(&State::Unvisited) {
                let mut stack = vec![(definition.component, 0usize)];
                state.insert(definition.component, State::Visiting);

                while let Some((node, dependency_index)) = stack.last_mut() {
                    let node = *node;
                    let dependencies: &[Component] = graph
                        .iter()
                        .find(|item| item.component == node)
                        .map_or(&[], |item| item.depends_on);

                    if *dependency_index < dependencies.len() {
                        let next = dependencies[*dependency_index];
                        *dependency_index += 1;

                        match state.get(&next) {
                            Some(State::Visiting) => {
                                let mut cycle = vec![next];
                                let mut current = node;
                                while current != next {
                                    cycle.push(current);
                                    current = parent[&current];
                                }
                                cycle.push(next);
                                cycle.reverse();
                                return Some(cycle);
                            }
                            Some(State::Unvisited) => {
                                parent.insert(next, node);
                                state.insert(next, State::Visiting);
                                stack.push((next, 0));
                            }
                            _ => {}
                        }
                    } else {
                        state.insert(node, State::Visited);
                        stack.pop();
                    }
                }
            }
        }
        None
    }

    #[test]
    fn test_architecture_graph_is_acyclic() {
        if let Some(cycle) = detect_cycle(ARCHITECTURE_GRAPH) {
            let formatted_cycle = cycle
                .iter()
                .map(ArchitectureComponent::as_ref)
                .collect::<Vec<_>>()
                .join(" -> ");
            panic!("Architectural cycle detected in ARCHITECTURE_GRAPH: {formatted_cycle}");
        }
    }

    #[test]
    fn test_cycle_detector_identifies_cycles() {
        let cyclic_graph = architecture_graph! {
            "alpha" => ["beta"],
            "beta"  => ["alpha"],
        };
        assert_eq!(
            detect_cycle(cyclic_graph),
            Some(vec!["alpha", "beta", "alpha"])
        );
    }
}
