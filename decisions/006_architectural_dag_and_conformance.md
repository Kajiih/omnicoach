# Architectural DAG Specification and Conformance Checking

## 1. Problem Statement
Previously, architectural layering in `omni` was enforced via a crude 1D slice array in `tests/architecture.rs`:
```rust
const LAYERS: &[&[&str]] = &[
    &["diagnostic", "diff"],
    &["core", "code_lint::ast", "command_lint::vcs"],
    &["code_lint::bindings", "code_lint::calls", "code_lint::comments"],
    &["code_lint::rule", "command_lint::rule", "test_utils"],
    &["code_lint::rules", "code_lint::suppression", "command_lint::rules"],
    &["code_lint::runner", "command_lint::runner"],
];
```

This 1D layering suffered from multiple architectural deficiencies:
1. **Total Order Fallacy**: Slices forced two completely independent domain paths (`code_lint` and `command_lint`) into the same horizontal tiers, creating an artificial total order that permitted cross-domain contamination.
2. **Invisible Intent & Lack of Self-Documentation**: Module files lacked any self-declaring architectural identity; an engineer reading a file in `src/` could not tell which component or boundary it belonged to without cross-referencing an external test file.
3. **Overly Permissive Test Utilities**: Placing `test_utils` in an intermediate layer permitted production rules to import test harness code in production logic.
4. **Maintenance Friction**: Modifying or introducing an abstraction required manual calculations of allowed dependencies and ad-hoc exception lists.

---

## 2. Decision: The Reflexion Architectural DAG

We model and enforce the architecture as a **strongly-typed Directed Acyclic Graph (DAG)** with **colocated component declarations**:

### 2.1. Canonical Architecture Components
We define a strongly-typed enum `ArchitectureComponent` representing the 15 bounded functional units across the repository:
- **Shared Foundations**: `FoundationPrimitives`, `RuleDeclaration`, `Config`
- **Static Code Analysis Domain (`code_lint`)**: `CodeSyntaxAdapters`, `CodeSemanticEngines`, `CodeRuleContracts`, `CodeSuppressionEngine`, `CodeLintRules`, `CodeLintRunner`
- **Command Safety Domain (`command_lint`)**: `CommandVcsAdapters`, `CommandRuleContracts`, `CommandLintRules`, `CommandLintRunner`
- **Selection, Discovery, Test Harness & Entrypoints**: `RuleSelection`, `RuleCatalog`, `TestingHarness`, `ApplicationBinaries`

### 2.2. Component-Root Declarations & Subtree Inheritance
Each component root file in `src/` explicitly declares its component identity at the top of the file:
```rust
architecture_component!(CodeLintRules);
```
In `src/lib.rs`, `#[macro_export] macro_rules! architecture_component` expands to a typed module-scoped constant:
```rust
const _ARCHITECTURE_COMPONENT: $crate::architecture::ArchitectureComponent =
    $crate::architecture::ArchitectureComponent::$component;
```
This gives immediate compile-time validation by `rustc` (catching misspelled variants with `E0599` and duplicate declarations in the same module with `E0428`), IDE hover documentation, and direct support in binary targets (`omni::architecture_component!(ApplicationBinaries);`).

- **Subtree Inheritance**: Descendant leaf modules inside a component's subtree (e.g. `src/rule_declaration/*.rs`, `src/code_lint/rules/*.rs`, `src/code_lint/ast/*.rs`, `src/code_lint/semantic/*.rs`, `src/command_lint/rules/*.rs`) inherit their component automatically from their nearest declared ancestor and are forbidden from redundantly re-declaring `architecture_component!(...)`.
- **Content-Verified Pure Namespace Routers**: Files outside any component subtree (`src/lib.rs`, `src/code_lint.rs`, `src/command_lint.rs`) require no hardcoded exemption list; instead, `test_all_source_files_declare_architecture_component` verifies via the Rust CST that they are pure namespace routers containing only external `mod <name>;` declarations (plus `macro_rules!` in `src/lib.rs`) and zero production functions, types, constants, inline `mod { ... }` blocks, or `use` imports.

### 2.3. The `define_architecture!` Macro and Compile-Time Acyclicity by Declaration Order
In `src/architecture.rs` (`FoundationPrimitives`), `define_architecture!` generates the `strum`-derived `ArchitectureComponent` enum (attaching the `///` doc comments to each variant), the `ARCHITECTURE_GRAPH` constant, and a compile-time `const` assertion requiring every dependency in `$node => [ $($dep),* ]` to be declared earlier (`(ArchitectureComponent::$dep as usize) < (ArchitectureComponent::$node as usize)`). This makes any cycle or forward edge a `rustc` compile error (`E0080`) during `cargo check`, with zero runtime cycle-detection code. Only `architecture_component!` is exported from `src/lib.rs`, because binary crates invoke it as `omni::architecture_component!`:
```rust
define_architecture! {
    /// Zero-dependency foundational primitives (`architecture`, `diagnostic`, `diff`).
    FoundationPrimitives  => [],
    /// Rule declaration, options, faceted classification, and documentation (`rule_declaration`).
    RuleDeclaration       => [FoundationPrimitives],
    /// Project configuration and resolved rule/path state (`config`).
    Config                => [FoundationPrimitives, RuleDeclaration],

    /// Encapsulated AST syntax adapters and language parsers (`code_lint::ast`).
    CodeSyntaxAdapters    => [FoundationPrimitives],
    /// Semantic analysis engines (`code_lint::semantic`).
    CodeSemanticEngines   => [CodeSyntaxAdapters],
    /// Rule contracts and execution interfaces for code linting (`code_lint::rule`).
    CodeRuleContracts     => [CodeSemanticEngines, CodeSyntaxAdapters, RuleDeclaration],
    /// Inline comment suppression tracker and directive policies (`code_lint::suppression`).
    CodeSuppressionEngine => [CodeRuleContracts, CodeSemanticEngines, RuleDeclaration, Config],
    /// Concrete static analysis linter rules (`code_lint::rules`).
    CodeLintRules         => [CodeRuleContracts, RuleDeclaration],
    /// Static code linting multi-file orchestration runner (`code_lint::runner`).
    CodeLintRunner        => [CodeLintRules, CodeSuppressionEngine],

    /// VCS interaction and repository diff adapters (`command_lint::vcs`).
    CommandVcsAdapters    => [FoundationPrimitives],
    /// Rule contracts and intercepted command schemas (`command_lint::rule`).
    CommandRuleContracts  => [CommandVcsAdapters, RuleDeclaration],
    /// Concrete command safety linting rules (`command_lint::rules`).
    CommandLintRules      => [CommandRuleContracts, RuleDeclaration],
    /// Command linting orchestration and interception runner (`command_lint::runner`).
    CommandLintRunner     => [CommandLintRules, Config],

    /// Faceted taxonomy queries, hierarchical Model B selector planner, and config resolution (`rule_selection`).
    RuleSelection         => [RuleDeclaration, CodeLintRules, CodeSuppressionEngine, CommandLintRules, Config],
    /// Rule catalog, terminal documentation renderer, and discovery CLI surface (`rule_catalog`).
    RuleCatalog           => [RuleSelection, RuleDeclaration],
    /// Test harness and snapshot fixtures (`test_utils`).
    TestingHarness        => [CodeRuleContracts, CommandRuleContracts],
    /// CLI application entrypoint binaries (`src/bin/*`).
    ApplicationBinaries   => [CodeLintRunner, CommandLintRunner, RuleSelection, RuleCatalog],
}
```

### 2.4. Native Single-Pass CST Conformance Engine & Idiomatic Module Policies
- **Single-Pass `RustFileSummary` Extraction (`src/code_lint/ast/rust.rs`)**: All `.rs` files in `src/` are parsed once via `ParsedFile::rust` and summarized by `summarize_rust_file` into plain `Send + Sync` value structs (`RustFileSummary`) cached in a `static LazyLock` in `tests/architecture_conformance.rs`.
- **Automatic Module Discovery**: Component roots are discovered directly from the `architecture_component!(...)` declarations in `src/`, and child modules resolve their component by climbing their module path to the nearest declared ancestor. Unannotated router modules (`src/lib.rs`, `src/code_lint.rs`, `src/command_lint.rs`) are verified both topologically (must be an ancestor of at least one declared component root) and structurally via the CST (may contain only external `mod <name>;` declarations, plus `macro_rules!` in `src/lib.rs`).
- **Complete Path Canonicalization & Transitive Reachability ($\to^+$)**: Every referenced path in production code—across expanded `use` trees, inline expressions/types (`scoped_identifier` / `scoped_type_identifier`), turbofish type arguments, and macro `token_tree` arguments—is normalized (`crate::`, `omni::`, `self::`, `super::`, and local child `mod` prefixes) to a canonical module path and verified against the transitive reachability of `ARCHITECTURE_GRAPH`.
- **Universal Sibling Subtree Isolation**: Every multi-unit component enforces a strict hub-and-spoke internal topology:
  - The component root (`code_lint::ast`, `code_lint::semantic`, `code_lint::rules`, `command_lint::rules`) owns shared component contracts and coordinates its children.
  - Direct child subtrees under a component root (as well as multi-root siblings in `FoundationPrimitives` and `ApplicationBinaries`) may import from their component root, but **may never import sideways from sibling units**.
- **Idiomatic Intra-Component Relative Paths (`super::` / `self::`)**: Relative paths (`super::` and `self::`) are permitted in production code when their resolved canonical target stays within the enclosing file's `ArchitectureComponent` root subtree (e.g. `code_lint::ast::rust` referencing `super::AstNode`). Cross-component references must use canonical `crate::` paths.
- **Idiomatic Private-Child Facade Re-Exports (`pub use`)**: Every item maintains a single public path. A module may re-export items (`pub use` / `pub(crate) use`) when all targets come from a **private direct child submodule** (`mod child; pub use self::child::Item;`), enabling clean component facades without creating duplicate public paths or smuggling cross-component symbols.
- **Structural Production vs. Test Segregation**: `summarize_rust_file` skips inline `#[cfg(test)]` and `#[test]` CST subtrees (including their preceding attributes) directly during traversal, preserving production items that appear below a `#[cfg(test)]` item and preventing `TestingHarness` from leaking into production logic.

---

## 3. Automated Verification
The graph definition is verified at compile time (`const` topological-order assertion in `define_architecture!` and `missing_docs = "deny"` on every `ArchitectureComponent` variant), while `tests/architecture_conformance.rs` enforces conformance of the source tree:
1. `test_all_source_files_declare_architecture_component`: Ensures every file in `src/` either belongs to a valid component subtree (with no redundant child declarations) or is a topologically valid, CST-verified pure namespace router (`mod`-only items), and every component variant is backed by at least one root file.
2. `test_architecture_conformance`: Verifies all source files comply with the DAG reachability and universal sibling subtree isolation rules.
3. `test_architecture_rules_detect_forbidden_dependencies`: Guards against vacuous conformance passes by injecting upward, cross-domain, turbofish, macro-argument, direct-child sibling (via `super::`), and multi-root sibling dependencies.
4. `test_subtree_inheritance_resolves_leaf_modules_and_rejects_namespace_routers`: Verifies ancestor component inheritance for leaf files and `None` resolution for pure namespace routers.
5. `test_namespace_validator_accepts_pure_routers_and_rejects_code_or_orphan_routers`: Guards against production items, imports, or orphan namespace routers sneaking into `src/`.
6. `test_ast_grep_is_encapsulated`: Ensures only designated adapter modules handle `ast_grep_core`.
7. `test_items_have_a_single_path`: Enforces single public paths while permitting private-child facade re-exports (`mod child; pub use self::child::Item;`).
8. `test_second_path_detection_allows_private_child_facades_and_rejects_duplicates`: Guards against vacuous second-path passes, duplicate public paths (`pub mod` + `pub use`), and cross-component re-exports.
9. `test_relative_paths_stay_within_component`: Enforces that `super::` and `self::` stay within their enclosing component root subtree while cross-component references use `crate::`.
10. `test_relative_path_boundary_allows_intra_component_and_rejects_cross_component`: Guards against vacuous relative-path passes and false positives in comments/strings/tests.
