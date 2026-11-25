//! Architectural component catalog and Directed Acyclic Graph (DAG) specification.
//!
//! Defines [`ArchitectureComponent`] and compile-time topological acyclicity assertions for
//! [`crate::architecture_component!`] declarations and intra-component `modules` DAGs across
//! the codebase.
//!
//! The `define_architecture!` block at the end of this file is the single list of components
//! and their allowed dependencies; design rationale is in
//! `docs/dev/decisions/006_architectural_dag_and_conformance.md`. `tests/architecture_conformance.rs`
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
//! - **Encapsulation**: only `paths`, `diagnostic`, `rule_declaration`, `rule_selection`,
//!   `code_lint::ast`, `code_lint::project`, and `command_lint::command` use component-owned
//!   external crates (`owns_external`).

architecture_component!(Architecture);

use strum::{AsRefStr, Display, EnumString, VariantArray};

macro_rules! define_submodules {
    ($(
        $sub:ident => [ $( $sub_dep:ident ),* $(,)? ] $( (modules: { $($nested:tt)* }) )?
    ),* $(,)?) => {
        const _: () = {
            struct _SubmoduleOrder {
                $( $sub: usize, )*
            }
            let mut _index = 0usize;
            let _order = _SubmoduleOrder {
                $(
                    $sub: {
                        let current = _index;
                        _index += 1;
                        current
                    },
                )*
            };
            let _ = ( $( _order.$sub, )* );
            $(
                $(
                    assert!(
                        _order.$sub_dep < _order.$sub,
                        concat!(
                            "submodule `",
                            stringify!($sub),
                            "` cannot depend on sibling `",
                            stringify!($sub_dep),
                            "`: dependencies must be declared before their dependents in `modules: {{ ... }}` to guarantee acyclicity",
                        ),
                    );
                )*
                define_submodules!($( $($nested)* )?);
            )*
        };
    };
}

/// Defines `pub enum ArchitectureComponent` and compile-time `const` assertions requiring every
/// component and submodule dependency to be declared before its dependent (making the graph
/// acyclic by construction).
macro_rules! define_architecture {
    ($(
        $(#[$meta:meta])*
        $node:ident => [ $( $dep:ident ),* $(,)? ]
            $( (owns_external: [ $( $ext:literal ),* $(,)? ]) )?
            $( (modules: { $($mods:tt)* }) )?
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

        const _: () = {
            $(
                let _ = ( $( $( $ext, )* )? );
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
                define_submodules!($( $($mods)* )?);
            )*
        };
    };
}

define_architecture! {
    // --- Shared Foundations ---
    /// The component catalog and its dependency graph (`architecture`).
    Architecture        => [],
    /// Rule names, source spans, violation templates and diagnostic output (`diagnostic`).
    Diagnostic          => [] (owns_external: ["line_index"]),
    /// VCS detection and the changed lines of a working copy (`diff`).
    Diff                => [],
    /// Host-independent lexical path normalization and anchoring utilities (`paths`).
    Paths               => [] (owns_external: ["path_clean"]),
    /// What every rule states about itself: name, template, languages, options, classification and doc (`rule_declaration`).
    RuleDeclaration     => [Diagnostic] (owns_external: ["tagonomy"]) (modules: {
        documentation => [],
        options => [],
        taxonomy => [],
    }),

    // --- Static Code Analysis Domain (`code_lint`) ---
    /// Parsed files and the syntax queries rules share, with parser crates encapsulated behind them (`code_lint::ast`).
    CodeLintAst         => [Diagnostic] (owns_external: ["ra_ap_syntax", "ruff_python_ast", "ruff_python_parser", "ruff_text_size"]),
    /// Cross-language facts computed over the AST: bindings, calls, comments (`code_lint::semantic`).
    CodeLintSemantic    => [CodeLintAst] (modules: {
        bindings => [],
        calls => [],
        comments => [],
        type_annotations => [],
    }),
    /// Multi-file project layout, symbol indexing, fact extraction, and architectural conformance (`code_lint::project`).
    CodeLintProject     => [CodeLintAst, Diagnostic, Paths] (owns_external: ["ignore"]) (modules: {
        foundation => [] (modules: {
            config => [],
            architecture => [config],
            source => [config],
            facts => [architecture],
        }),
        python => [foundation] (modules: {
            paths => [],
            layout => [paths],
            architecture => [layout, paths],
            index => [architecture, layout, paths],
            facts => [architecture, index, layout, paths],
            query => [index],
        }),
        rust => [foundation] (modules: {
            paths => [],
            layout => [paths],
            architecture => [layout, paths],
            index => [architecture, layout, paths],
            facts => [index, layout, paths],
        }),
        context => [foundation, python, rust] (modules: {
            layout => [],
            index => [layout],
            discovery => [index, layout],
            query => [index],
        }),
    }),
    /// The code rule contract: a declaration, a file target and a check function (`code_lint::contract`).
    CodeLintContract    => [CodeLintProject, CodeLintSemantic, CodeLintAst, RuleDeclaration],
    /// Rule-selection and per-rule override configuration resolved from `.omnicoach.toml` (`config`).
    Config              => [CodeLintProject, Diagnostic, RuleDeclaration],
    /// Inline comment suppression tracker and directive policies (`code_lint::suppression`).
    CodeLintSuppression => [CodeLintContract, CodeLintSemantic, RuleDeclaration, Config],
    /// Policy shared by several code rules or by a rule and the test harness, built from AST and project facts (`code_lint::policy`).
    CodeLintPolicy      => [CodeLintProject, CodeLintAst, RuleDeclaration],
    /// Concrete static analysis linter rules (`code_lint::rules`).
    CodeLintRules       => [CodeLintProject, CodeLintContract, CodeLintPolicy, RuleDeclaration],
    /// Static code linting multi-file orchestration runner (`code_lint::runner`).
    CodeLintRunner      => [CodeLintProject, CodeLintRules, CodeLintSuppression, Diff, Paths],
    /// Alpha / experimental multi-level abstraction graph model, builder, and text renderers (`code_lint::graph`).
    CodeLintGraph       => [CodeLintProject, CodeLintAst, Diagnostic] (modules: {
        model => [],
        builder => [model],
        index => [model],
        query => [builder, index, model],
        render => [index, model],
    }),

    // --- Command Safety Domain (`command_lint`) ---
    /// The intercepted command parser: shell input to commands and their arguments (`command_lint::command`).
    CommandLintCommand  => [Diagnostic, Paths] (owns_external: ["brush_parser"]) (modules: {
        model => [],
        cli_args => [model],
        shell => [model] (modules: {
            span_projection => [],
            lowering => [span_projection],
        }),
    }),
    /// Jujutsu CLI parsing, revset boundedness, and repository state queries (`command_lint::vcs`).
    CommandLintVcs      => [CommandLintCommand] (modules: {
        revset => [],
        cli => [revset],
        client => [],
    }),
    /// The command rule contract: a declaration and a check over a parsed command (`command_lint::contract`).
    CommandLintContract => [CommandLintCommand, CommandLintVcs, RuleDeclaration],
    /// Concrete command safety linting rules (`command_lint::rules`).
    CommandLintRules    => [CommandLintContract, RuleDeclaration],
    /// Command linting orchestration and interception runner (`command_lint::runner`).
    CommandLintRunner   => [CommandLintRules, Config],

    // --- Rule Selection ---
    /// Taxonomy queries, config selector resolution and rule option validation (`rule_selection`).
    RuleSelection       => [Diff, Paths, RuleDeclaration, CodeLintRules, CodeLintSuppression, CommandLintRules, Config] (owns_external: ["tagonomy"]),
    /// Rule list and single-rule Markdown rendering for discovery commands (`rule_catalog`).
    RuleCatalog         => [RuleSelection, RuleDeclaration],

    // --- Agent Lifecycle Hooks (`hook`) ---
    /// Agent-neutral tool-call evaluation, per-agent protocol adapters, deferred warning queue, and in-memory virtual file patching (`hook`).
    Hook                => [CodeLintRunner, CommandLintRunner, RuleSelection, RuleCatalog, Diagnostic, Paths] (modules: {
        event => [],
        warning_queue => [],
        virtual_edit => [event],
        runner => [event, virtual_edit, warning_queue],
        antigravity => [event, runner, warning_queue],
    }),

    // --- Test Harness & Entrypoints ---
    /// The `rule_test!` harness and snapshot fixtures (`test_utils`).
    TestUtils           => [CodeLintContract, CodeLintProject, CodeLintPolicy, CommandLintContract],
    /// Command-line argument parsing, dispatch, and BrokenPipe-safe output (`cli`).
    Cli                 => [CodeLintRunner, CodeLintGraph, CommandLintRunner, Config, Diagnostic, Hook, RuleSelection, RuleCatalog],
    /// CLI entrypoints (`bin::*`).
    Bin                 => [Cli],
}
