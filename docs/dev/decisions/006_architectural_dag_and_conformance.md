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

We define a strongly-typed enum `ArchitectureComponent` in [src/architecture.rs](../../../src/architecture.rs) representing the 25 bounded functional units across the repository, named directly after their module path (`code_lint::ast` $\to$ `CodeLintAst`):

- **Shared Foundations**: `Architecture`, `Diagnostic`, `Diff`, `RuleDeclaration`, `Config`
- **Static Code Analysis Domain (`code_lint`)**: `CodeLintAst`, `CodeLintSemantic`, `CodeLintProject`, `CodeLintContract`, `CodeLintSuppression`, `CodeLintPolicy`, `CodeLintRules`, `CodeLintRunner`, `CodeLintGraph`
- **Command Safety Domain (`command_lint`)**: `CommandLintVcs`, `CommandLintCommand`, `CommandLintContract`, `CommandLintRules`, `CommandLintRunner`
- **Selection, Discovery, Hooks, Test Harness & Entrypoints**: `RuleSelection`, `RuleCatalog`, `Hook`, `TestUtils`, `Cli`, `Bin`

### 2.2. Component-Root Declarations & Subtree Inheritance

Each component root file in `src/` explicitly declares its component identity at the top of the file:

```rust
architecture_component!(CodeLintRules);
```

In [src/lib.rs](../../../src/lib.rs), `#[macro_export] macro_rules! architecture_component` expands to a typed module-scoped constant:

```rust
const _ARCHITECTURE_COMPONENT: $crate::architecture::ArchitectureComponent =
    $crate::architecture::ArchitectureComponent::$component;
```

This gives immediate compile-time validation by `rustc` (catching misspelled variants with `E0599` and duplicate declarations in the same module with `E0428`), IDE hover documentation, and direct support in binary targets (`omnicoach::architecture_component!(Bin);`).

- **Subtree Inheritance**: Descendant leaf modules inside a component's subtree (e.g. `src/rule_declaration/*.rs`, `src/code_lint/rules/*.rs`, `src/code_lint/ast/*.rs`, `src/code_lint/semantic/*.rs`, `src/code_lint/project/*.rs`, `src/code_lint/graph/*.rs`, `src/command_lint/rules/*.rs`) inherit their component automatically from their nearest declared ancestor and are forbidden from redundantly re-declaring `architecture_component!(...)` (`invalid-architecture-component`).
- **Content-Verified Pure Namespace Routers**: Files outside any component subtree (`src/lib.rs`, `src/code_lint.rs`, `src/command_lint.rs`) require no hardcoded exemption list; instead, `unmapped-architecture-module` in [src/code_lint/project/architecture.rs](../../../src/code_lint/project/architecture.rs) verifies via the Rust CST that they are pure namespace routers containing only external `mod <name>;` declarations (plus `macro_rules!` in `src/lib.rs`) and zero production functions, types, constants, inline `mod { ... }` blocks, or `use` imports.

### 2.3. The `define_architecture!` Macro and Compile-Time Acyclicity by Declaration Order

In [src/architecture.rs](../../../src/architecture.rs) (`Architecture`), `define_architecture!` generates the `strum`-derived `ArchitectureComponent` enum (attaching the `///` doc comments to each variant), the `ARCHITECTURE_GRAPH` constant (`&[ComponentDefinition]`, recording `depends_on` and `owns_external`), and a compile-time `const` assertion requiring every dependency in `$node => [ $($dep),* ]` to be declared earlier (`(ArchitectureComponent::$dep as usize) < (ArchitectureComponent::$node as usize)`). This makes any cycle or forward edge a `rustc` compile error (`E0080`) during `cargo check`, with zero runtime cycle-detection code. Only `architecture_component!` is exported from [src/lib.rs](../../../src/lib.rs), because binary crates invoke it as `omnicoach::architecture_component!`.

See `define_architecture!` in [src/architecture.rs](../../../src/architecture.rs) for the authoritative component graph, including external-crate ownership (`owns_external` on `Diagnostic`, `CodeLintAst`, `CodeLintProject`, and `CommandLintCommand`) and the placement of `CodeLintProject` below `Config` so neither `CodeLintProject` nor `CodeLintGraph` depends on rule-selection `Config`.

### 2.4. Production Conformance Engine & Idiomatic Module Policies

Architectural conformance is implemented as a first-class production subsystem in `CodeLintProject` ([src/code_lint/project/architecture.rs](../../../src/code_lint/project/architecture.rs) and [src/code_lint/project/paths.rs](../../../src/code_lint/project/paths.rs)) and exposed through eight `Topic::ARCHITECTURE` lint rules in `src/code_lint/rules/`:

- **Single-Pass `RustFileSummary` Extraction ([src/code_lint/ast/rust/file_summary.rs](../../../src/code_lint/ast/rust/file_summary.rs))**: Each `.rs` file is parsed once via `ParsedFile::rust` and summarized by `summarize_rust_file` into plain `Send + Sync` value structs (`RustFileSummary`) cached in `ProjectIndex` ([src/code_lint/project/index.rs](../../../src/code_lint/project/index.rs)).
- **Automatic Module & Component Discovery (`unmapped-architecture-module`, `invalid-architecture-component`)**: Component roots are discovered from `architecture_component!(...)` declarations in `src/` (or configured via `[architecture]` in `.omnicoach.toml`), and child modules resolve their component by climbing their module path to the nearest declared ancestor (`resolve_inherited_component_with_root` in [src/code_lint/project/architecture.rs](../../../src/code_lint/project/architecture.rs)). Unannotated router modules (`src/lib.rs`, `src/code_lint.rs`, `src/command_lint.rs`) are verified both topologically (must be an ancestor of at least one declared component root) and structurally via the CST (may contain only external `mod <name>;` declarations, plus `macro_rules!` in `src/lib.rs`).
- **Complete Path Canonicalization & Transitive Reachability (`undeclared-component-dependency`, `unowned-external-dependency`, `private-component-import`)**: Every referenced path in production code—across expanded `use` trees, inline expressions/types (`scoped_identifier` / `scoped_type_identifier`), turbofish type arguments, and macro `token_tree` arguments—is normalized (`crate::`, `omnicoach::`, leading `::`, `self::`, `super::`, and local child `mod` prefixes) via `resolve_rust_reference_path` in [src/code_lint/project/paths.rs](../../../src/code_lint/project/paths.rs) and verified against the transitive reachability, `owns_external` encapsulation, and `exports` surface of `ArchitectureSpec`.
- **Universal Sibling Subtree Isolation (`sibling-module-dependency`)**: Every multi-unit component enforces a strict hub-and-spoke internal topology:
  - The component root (`code_lint::ast`, `code_lint::semantic`, `code_lint::project`, `code_lint::graph`, `code_lint::rules`, `command_lint::rules`) owns shared component contracts and coordinates its children.
  - Direct child subtrees under a component root (as well as multi-root siblings in `Bin`) may import from their component root, but **may never import sideways from sibling units**.
- **Idiomatic Intra-Component Relative Paths (`cross-component-relative-path`)**: Relative paths (`super::` and `self::`) are permitted in production code when their resolved canonical target stays within the enclosing file's component root subtree (e.g. `code_lint::ast::rust` referencing `super::AstNode`). Cross-component references must use canonical `crate::` paths.
- **Idiomatic Private-Child Facade Re-Exports (`reexported-public-item`)**: Every item maintains a single public path. A module may re-export items (`pub use` / `pub(crate) use`) when all targets come from a **private direct child submodule** (`mod child; pub use self::child::Item;`), enabling clean component facades without creating duplicate public paths or smuggling cross-component symbols.
- **Structural Production vs. Test Segregation**: `summarize_rust_file` skips inline `#[cfg(test)]` and `#[test]` CST subtrees (including their preceding attributes) directly during traversal, preserving production items that appear below a `#[cfg(test)]` item and preventing `TestUtils` from leaking into production logic.

---

## 3. Automated Verification

The graph definition is verified at compile time (`const` topological-order assertion in `define_architecture!` and `missing_docs = "deny"` on every `ArchitectureComponent` variant), while [src/code_lint/project/architecture.rs](../../../src/code_lint/project/architecture.rs) unit tests and [tests/architecture_conformance.rs](../../../tests/architecture_conformance.rs) enforce conformance of the source tree:

1. `test_production_architecture_rules_on_src` ([tests/architecture_conformance.rs](../../../tests/architecture_conformance.rs)): Runs all eight production `Topic::ARCHITECTURE` rules (`undeclared-component-dependency`, `sibling-module-dependency`, `cross-component-relative-path`, `unowned-external-dependency`, `private-component-import`, `reexported-public-item`, `unmapped-architecture-module`, and `invalid-architecture-component`) across `src/`.
2. `test_all_architecture_components_have_root_modules_in_src` ([tests/architecture_conformance.rs](../../../tests/architecture_conformance.rs)): Ensures every variant in `ArchitectureComponent::VARIANTS` is declared by at least one root module in `src/`.
3. `test_components_are_named_after_their_modules` ([tests/architecture_conformance.rs](../../../tests/architecture_conformance.rs)): Verifies that every component name is the `PascalCase` of its root module path (or shared parent module for `bin::*` $\to$ `Bin`).
4. `test_subtree_inheritance_resolves_leaf_modules_and_rejects_namespace_routers` ([tests/architecture_conformance.rs](../../../tests/architecture_conformance.rs)): Verifies ancestor component inheritance for leaf files and `None` resolution for pure namespace routers.
5. Comprehensive unit tests in [src/code_lint/project/architecture.rs](../../../src/code_lint/project/architecture.rs) and `rule_test!` suites in `src/code_lint/rules/*.rs`: Verify upward/cross-domain dependency rejection, sibling subtree isolation, relative-path boundaries, private-child facade re-exports, external-crate ownership (`owns_external`), exported surface checks (`exports`), and pure namespace router validation across Rust and Python.
