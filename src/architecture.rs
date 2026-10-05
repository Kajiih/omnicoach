//! Architectural component catalog and Directed Acyclic Graph (DAG) specification.
//!
//! Defines [`ArchitectureComponent`], [`ComponentDefinition`], and [`ARCHITECTURE_GRAPH`],
//! providing compile-time validation for [`crate::architecture_component!`] declarations
//! across the codebase and the canonical topology for `tests/architecture_conformance.rs`.
//!
//! The `define_architecture!` block at the end of this file is the single list of components
//! and their allowed dependencies; design rationale is in
//! `decisions/006_architectural_dag_and_conformance.md`. `tests/architecture_conformance.rs`
//! enforces:
//! - **Naming**: a component is named after its module path (`code_lint::ast` →
//!   `CodeLintAst`), and every source file declares its component.
//! - **Reachability**: a module only imports from components its component depends on.
//! - **Sibling isolation**: rule files inherit their parent's component and never import
//!   from sibling rules; shared logic moves down to `code_lint::semantic` or `code_lint::ast`.
//! - **Single public path**: every item has one canonical path. Cross-component imports use
//!   `crate::`; `super::` / `self::` stay within a component subtree. A visible re-export is
//!   allowed only from a private direct child (`mod child; pub use self::child::Item;`), and a
//!   `macro_rules!` macro declares its path with `pub(crate) use name;` next to its definition
//!   (`#[macro_export]` only in `src/lib.rs`).
//! - **Encapsulation**: only `code_lint::ast` uses `ast_grep_core`.

architecture_component!(Architecture);

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
    /// The component catalog and its dependency graph (`architecture`).
    Architecture        => [],
    /// Rule names, source spans, violation templates and diagnostic output (`diagnostic`).
    Diagnostic          => [],
    /// VCS detection and the changed lines of a working copy (`diff`).
    Diff                => [],
    /// What every rule states about itself: name, template, languages, options, classification and doc (`rule_declaration`).
    RuleDeclaration     => [Diagnostic],
    /// Project configuration resolved from `.omnilint.toml` (`config`).
    Config              => [Diagnostic, RuleDeclaration],

    // --- Static Code Analysis Domain (`code_lint`) ---
    /// Parsed files and the syntax queries rules share, with `ast_grep_core` encapsulated behind them (`code_lint::ast`).
    CodeLintAst         => [Diagnostic],
    /// Cross-language facts computed over the AST: bindings, calls, comments (`code_lint::semantic`).
    CodeLintSemantic    => [CodeLintAst],
    /// The code rule contract: a declaration, a file target and a check function (`code_lint::contract`).
    CodeLintContract    => [CodeLintSemantic, CodeLintAst, RuleDeclaration],
    /// Inline comment suppression tracker and directive policies (`code_lint::suppression`).
    CodeLintSuppression => [CodeLintContract, CodeLintSemantic, RuleDeclaration, Config],
    /// Suggestion policy shared by several code rules, built from AST facts (`code_lint::policy`).
    CodeLintPolicy      => [CodeLintAst],
    /// Concrete static analysis linter rules (`code_lint::rules`).
    CodeLintRules       => [CodeLintContract, CodeLintPolicy, RuleDeclaration],
    /// Static code linting multi-file orchestration runner (`code_lint::runner`).
    CodeLintRunner      => [CodeLintRules, CodeLintSuppression, Diff],

    // --- Command Safety Domain (`command_lint`) ---
    /// The jj client: repository state queries for command rules (`command_lint::vcs`).
    CommandLintVcs      => [],
    /// The intercepted command parser: shell input to commands and their arguments (`command_lint::command`).
    CommandLintCommand  => [],
    /// The command rule contract: a declaration and a check over one intercepted command (`command_lint::contract`).
    CommandLintContract => [CommandLintCommand, CommandLintVcs, RuleDeclaration],
    /// Concrete command safety linting rules (`command_lint::rules`).
    CommandLintRules    => [CommandLintContract, RuleDeclaration],
    /// Command linting orchestration and interception runner (`command_lint::runner`).
    CommandLintRunner   => [CommandLintRules, Config],

    // --- Rule Selection ---
    /// Taxonomy queries, config selector resolution and rule option validation (`rule_selection`).
    RuleSelection       => [RuleDeclaration, CodeLintRules, CodeLintSuppression, CommandLintRules, Config],
    /// Rule list and single-rule Markdown rendering for discovery commands (`rule_catalog`).
    RuleCatalog         => [RuleSelection, RuleDeclaration],

    // --- Test Harness & Entrypoints ---
    /// The `rule_test!` harness and snapshot fixtures (`test_utils`).
    TestUtils           => [CodeLintContract, CommandLintContract],
    /// CLI entrypoints (`bin::*`).
    Bin                 => [CodeLintRunner, CommandLintRunner, RuleSelection, RuleCatalog],
}
