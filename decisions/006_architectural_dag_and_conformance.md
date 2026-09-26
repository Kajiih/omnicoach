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
We define a strongly-typed enum `ArchitectureComponent` representing the 14 bounded functional units across the repository:
- **Shared Foundations**: `FoundationPrimitives`, `CoreVocabulary`
- **Static Code Analysis Domain (`code_lint`)**: `CodeSyntaxAdapters`, `CodeSemanticEngines`, `CodeRuleContracts`, `CodeSuppressionEngine`, `CodeLintRules`, `CodeLintRunner`
- **Command Safety Domain (`command_lint`)**: `CommandVcsAdapters`, `CommandRuleContracts`, `CommandLintRules`, `CommandLintRunner`
- **Entrypoints & Verification**: `TestingHarness`, `ApplicationBinaries`

### 2.2. Compile-Time Colocated Reflexion Declarations
Every non-root `.rs` file in `src/` explicitly declares its component identity at the top of the file:
```rust
architecture_component!(CodeLintRules);
```
In `src/lib.rs`, `#[macro_export] macro_rules! architecture_component` expands to a typed module-scoped constant:
```rust
const _ARCHITECTURE_COMPONENT: $crate::architecture::ArchitectureComponent =
    $crate::architecture::ArchitectureComponent::$component;
```
This gives immediate compile-time validation by `rustc` (catching misspelled variants with `E0599` and duplicate declarations in the same module with `E0428`), IDE hover documentation, and direct support in binary targets (`omni::architecture_component!(ApplicationBinaries);`).

**Per-file declarations over module-root inheritance**: Every file declares its component, rather than only component root files with submodules inheriting it. Opening any file shows its component without inspecting parent modules, and `test_all_source_files_declare_architecture_component` rejects a submodule whose component differs from its parent's, so the repetition cannot drift. Removing the need for per-file declarations is tracked in `ROADMAP.md` under *Deriving Module Organization from the Architectural Graph*.

### 2.3. The Composable `define_architecture!` and `architecture_graph!` Macros
In `src/architecture.rs` (`FoundationPrimitives`), `define_architecture!` generates both the `strum`-derived `ArchitectureComponent` enum (attaching the `///` doc comments to each variant) and the `ARCHITECTURE_GRAPH` constant by delegating slice construction to `architecture_graph!`. Both macros are local to `src/architecture.rs` (not `#[macro_export]`): `architecture_graph!` is also reused by the cycle-detector unit test to build synthetic graphs. Only `architecture_component!` is exported from `src/lib.rs`, because binary crates invoke it as `omni::architecture_component!`:
```rust
define_architecture! {
    // --- Shared Foundations ---
    /// Zero-dependency foundational primitives (`architecture`, `diagnostic`, `diff`).
    FoundationPrimitives  => [],
    /// Shared domain vocabulary, config, and tags (`core`).
    CoreVocabulary        => [FoundationPrimitives],

    // --- Static Code Analysis Domain (`code_lint`) ---
    /// Encapsulated AST syntax adapters and language parsers (`code_lint::ast`).
    CodeSyntaxAdapters    => [FoundationPrimitives],
    /// Semantic analysis engines (`bindings`, `calls`, `comments`).
    CodeSemanticEngines   => [CodeSyntaxAdapters] (no_internal_dependencies),
    /// Contract traits and execution interfaces for code linting (`code_lint::rule`).
    CodeRuleContracts     => [CoreVocabulary, CodeSemanticEngines, CodeSyntaxAdapters],
    /// Inline comment suppression tracker and directive policies (`code_lint::suppression`).
    CodeSuppressionEngine => [CodeRuleContracts, CodeSemanticEngines],
    /// Concrete static analysis linter rules (`code_lint::rules`).
    CodeLintRules         => [CodeRuleContracts, CodeSuppressionEngine] (no_internal_dependencies),
    /// Static code linting multi-file orchestration runner (`code_lint::runner`).
    CodeLintRunner        => [CodeLintRules],

    // --- Command Safety Domain (`command_lint`) ---
    /// VCS interaction and repository diff adapters (`command_lint::vcs`).
    CommandVcsAdapters    => [FoundationPrimitives],
    /// Contract traits and intercepted command schemas (`command_lint::rule`).
    CommandRuleContracts  => [CoreVocabulary, CommandVcsAdapters],
    /// Concrete command safety linting rules (`command_lint::rules`).
    CommandLintRules      => [CommandRuleContracts] (no_internal_dependencies),
    /// Command linting orchestration and interception runner (`command_lint::runner`).
    CommandLintRunner     => [CommandLintRules],

    // --- Test Harness & Entrypoints ---
    /// Test harness and snapshot fixtures (`test_utils`).
    TestingHarness        => [CodeRuleContracts, CommandRuleContracts],
    /// CLI application entrypoint binaries (`src/bin/*`).
    ApplicationBinaries   => [CodeLintRunner, CommandLintRunner, CoreVocabulary] (no_internal_dependencies),
}
```

### 2.4. Automatic Discovery, Transitive Reachability & Leaf Isolation
- **Automatic Module Discovery**: Component roots and leaf submodules are discovered directly from the colocated `architecture_component!(...)` declarations in `src/` via `omni::code_lint::ast` CST inspection, avoiding any hardcoded module-path tables in `tests/architecture_conformance.rs`.
- **Transitive Reachability ($\to^+$)**: An allowed import is computed via reachability on the DAG. For example, `CodeLintRules` depends directly on `[CodeRuleContracts, CodeSuppressionEngine]`, transitively granting access to `CodeSemanticEngines`, `CodeSyntaxAdapters`, `CoreVocabulary`, and `FoundationPrimitives` without manual configuration.
- **Leaf Isolation (`(no_internal_dependencies)`)**: Components flagged with `(no_internal_dependencies)` (`allow_internal_dependencies = false`) forbid leaf modules within the component from depending on each other. Concrete linter rules cannot import other linter rules; semantic engines cannot import other semantic engines; binaries cannot import other binaries.
- **Domain Hermeticity**: `code_lint` and `command_lint` have disjoint graph paths. A command rule attempting to import code ASTs or vice versa is immediately blocked.
- **Structural Production vs. Test Segregation**: Production code is verified by blanking out inline `#[cfg(test)]` and `#[test]` CST byte spans (`collect_inline_test_ranges`) before AST validation, preserving production items that appear below a `#[cfg(test)]` item and preventing `TestingHarness` from leaking into production logic.

---

## 3. Automated Test Suite
Tests are split by what they check: the graph definition itself is unit-tested next to it, while checks that read the whole `src/` tree live in the integration test.

`src/architecture.rs` (`#[cfg(test)] mod tests`) verifies the graph definition:
1. `test_architecture_graph_is_acyclic`: Verifies that `ARCHITECTURE_GRAPH` is acyclic using 3-color DFS.
2. `test_cycle_detector_identifies_cycles`: Guards against vacuous cycle detection using `architecture_graph!`.
3. `test_all_architecture_components_have_descriptions`: Ensures every `ArchitectureComponent` variant has a non-empty doc comment accessible via `strum::EnumMessage`.

`tests/architecture_conformance.rs` enforces conformance of the source tree:
1. `test_all_source_files_declare_architecture_component`: Ensures 100% of non-root files declare exactly one valid component, submodules are coherent with their parent module's component, and every component variant is backed by source files.
2. `test_architecture_conformance`: Verifies all source files comply with the DAG reachability and leaf-isolation rules.
3. `test_architecture_rules_detect_forbidden_dependencies`: Guards against vacuous conformance passes by injecting upward, cross-domain, and sibling-leaf dependencies.
4. `test_strip_inline_tests_preserves_production_code_after_conditional_test_item`: Guards against mid-file `#[cfg(test)]` truncation.
5. `test_ast_grep_is_encapsulated`: Ensures only designated adapter modules handle `ast_grep_core`.
6. `test_items_have_a_single_path`: Ensures no item is given multiple visibility paths (bans `pub use` re-exports and `#[macro_export]` outside `src/lib.rs`).
7. `test_no_relative_imports_in_production_code`: Enforces absolute `crate::` canonical paths across all production code.
8. `test_second_path_declarations_are_detected`: Guards against vacuous second-path passes and false positives in comments/strings.
9. `test_structural_extractors_ignore_strings_and_catch_after_conditional_test`: Guards against false positives in string literals/comments and verifies extraction after `#[cfg(test)]` items.
