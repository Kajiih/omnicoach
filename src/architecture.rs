//! Architectural component catalog and Directed Acyclic Graph (DAG) specification.
//!
//! Defines [`ArchitectureComponent`], [`ComponentDefinition`], and [`ARCHITECTURE_GRAPH`],
//! providing compile-time validation for [`crate::architecture_component!`] declarations
//! across the codebase and the canonical topology for `tests/architecture_conformance.rs`.

architecture_component!(FoundationPrimitives);

use strum::{AsRefStr, Display, EnumString, VariantArray};

/// Bounded specification of an architectural component and its direct dependencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentDefinition {
    /// The target architectural component being defined.
    pub component: ArchitectureComponent,
    /// Directly permitted dependency components (transitive dependencies are computed automatically).
    pub depends_on: &'static [ArchitectureComponent],
}

/// Defines `pub enum ArchitectureComponent`, `pub const ARCHITECTURE_GRAPH`, and a compile-time
/// `const` assertion requiring every dependency to be declared before its dependent (making the
/// graph acyclic by construction).
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
        pub const ARCHITECTURE_GRAPH: &[ComponentDefinition] = &[
            $(
                ComponentDefinition {
                    component: ArchitectureComponent::$node,
                    depends_on: &[ $( ArchitectureComponent::$dep ),* ],
                },
            )*
        ];

        const _: () = {
            $(
                $(
                    assert!(
                        (ArchitectureComponent::$dep as usize) < (ArchitectureComponent::$node as usize),
                        concat!(
                            "ArchitectureComponent::",
                            stringify!($node),
                            " cannot depend on ArchitectureComponent::",
                            stringify!($dep),
                            ": dependencies must be declared before their dependents in define_architecture! to guarantee acyclicity",
                        ),
                    );
                )*
            )*
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
