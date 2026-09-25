# Phase 1: Understand — Architectural DAG Specification & Conformance Design

This document records **Phase 1 (Understand)** of the design cycle for `omni`'s architectural specification and conformance verification system.

**Status**: Validated — ready for **Phase 2 (Gather Resources and Reference)**.

---

## 1. Context & Problem Statement

In [tests/architecture.rs](../../../tests/architecture.rs) and [decisions/006_architectural_dag_and_conformance.md](../../../decisions/006_architectural_dag_and_conformance.md), we replaced the legacy 1D `LAYERS: &[&[&str]]` array with a Directed Acyclic Graph (DAG) of 14 `ArchitectureComponent`s and colocated `architecture_component!(...)` declarations in `src/`.

However, the current design evolved incrementally through ad-hoc tactical edits rather than a holistic SOTA-informed design pass. Stepping back reveals four structural tensions:

1. **Macro vs. Type-System Role**:
   - In `tests/architecture.rs`, we introduced `define_architecture!` and `architecture_graph!` to avoid duplicating the 14 component names between `enum ArchitectureComponent` and `const ARCHITECTURE_GRAPH`.
   - Yet in `src/lib.rs`, `macro_rules! architecture_component { ($component:ident) => {}; }` is a no-op macro that discards its token—meaning `rustc` does not type-check `architecture_component!(...)` in `src/`, and `tests/architecture.rs` falls back to naive string-splitting (`extract_architecture_component`) to parse component names and strip `#[cfg(test)]` blocks.
2. **Coupling vs. Decoupling of Abstractions**:
   - The pure graph model (`ComponentDefinition`, cycle detection, transitive closure) must be cleanly separated from `omni`'s domain catalog (`ArchitectureComponent` and `ARCHITECTURE_GRAPH`), while still providing an ergonomic, readable way to construct graphs in both the specification and unit tests.
3. **Direction of Derivation (Discovery vs. Generation)**:
   - Currently, `src/` defines modules (`pub mod ...`) and tags files (`architecture_component!(...)`), and `tests/architecture.rs` discovers them bottom-up. Deriving module organization directly from the graph is recorded on the [ROADMAP.md](../../../ROADMAP.md) for a future cycle.
4. **Fragmented Architectural Rules**:
   - `tests/architecture.rs` mixes four separate mechanisms: (a) the `ARCHITECTURE_GRAPH` DAG for internal modules, (b) a hardcoded `AST_GREP_OWNERS: &[&str]` array for third-party crate encapsulation (`ast_grep_core`), (c) line-by-line string scanning for `pub use` / `#[macro_export]`, and (d) line-by-line string scanning for `use super::`.

---

## 2. Goals & Explicit Non-Goals

### Goals
- **G1 — Single Source of Truth for Architectural Topology & Identity**:
  - *Reason*: Adding, renaming, or rewiring an architectural component or module must not require synchronizing multiple tables, match arms, or duplicate enum/graph lists.
- **G2 — Clean Separation Between Generic Graph Engine and Crate-Specific Specification**:
  - *Reason*: Graph primitives (directed edges, cycle detection, transitive reachability, graph construction macro/builder) must be decoupled from `omni`'s specific component catalog so each abstraction has a single responsibility and can be tested in isolation.
- **G3 — Idiomatic, Self-Explanatory Declaration Syntax (RICR)**:
  - *Reason*: A developer reading either the central architecture specification or an individual source file must immediately understand the component boundaries, allowed dependencies, and internal isolation policy without deciphering macro magic or ambiguous terminology.
- **G4 — Hermetic Enforcement of Domain, Layer, Leaf, and External Crate Isolation**:
  - *Reason*: Prevent upward dependencies, cross-domain leaks (`code_lint` $\leftrightarrow$ `command_lint`), test-harness leaks into production code, sibling coupling among isolated leaves, and unencapsulated third-party library leaks (`ast_grep_core`).
- **G5 — SOTA-Grounded Design**:
  - *Reason*: Ensure our DSL, colocation mechanism, and verification model take advantage of proven patterns from state-of-the-art architecture enforcement tools across Rust and other ecosystems.

### Explicit Non-Goals
- **NG1 — Splitting `omni` into a Multi-Crate Cargo Workspace**:
  - *Reason*: Splitting a 42-file codebase into 14 micro-crates introduces heavy Cargo boilerplate, slower incremental metadata overhead, and cross-crate orphan-rule friction. (See [decisions/003_project_structure.md](../../../decisions/003_project_structure.md)).
- **NG2 — Building a Standalone External Crate / Proc-Macro Library**:
  - *Reason*: Avoiding a separate proc-macro crate keeps build times fast and dependencies minimal.
- **NG3 — Restructuring `src/` Directory Layout for 1-to-1 Module Isomorphism in This Cycle**:
  - *Reason*: Moving files in `src/` (e.g., grouping `diagnostic` and `diff` under `src/primitives/` or generating `pub mod` trees from the graph) is tracked in [ROADMAP.md](../../../ROADMAP.md) for a dedicated follow-up cycle so this cycle stays focused on the architecture specification, colocation mechanism, and verification engine.

---

## 3. Numbered Decisions (D1–D8)

- **D1 — Naming**: Use full, self-explanatory domain identifiers (`ArchitectureComponent`, `ComponentDefinition`, `no_internal_dependencies`); ban abbreviations like `arch`.
- **D2 — Documentation Colocation**: Component prose descriptions live once as standard `///` Rust doc comments on the component variants (accessible via `strum::EnumMessage`), never duplicated as string literals in graph tables.
- **D3 — No Hardcoded `root_modules()` Table**: Module-to-component membership is derived from the `src/` declarations rather than duplicated in a hardcoded `match` table in `tests/architecture.rs`.
- **D4 — Leaf-Level Isolation Semantics**: For components with `allow_internal_dependencies = false` (`(no_internal_dependencies)`), isolation applies uniformly to all **leaf modules** of that component.
- **D5 — Cycle Scope**: Focus this cycle on the architecture declaration DSL, component colocation mechanism, and holistic verification engine in `tests/architecture.rs` + `src/lib.rs`, deferring `src/` directory restructuring to the roadmap (NG3).
- **D6 — Evaluate Compile-Time Component Resolution**: Investigate in Phase 2/3 whether resolving `architecture_component!(...)` against the real `ArchitectureComponent` enum (for compile-time validation, IDE hover docs, and Go-To-Definition) is superior to a test-only enum.
- **D7 — Holistic `tests/architecture.rs` Scope**: Include external crate encapsulation (`AST_GREP_OWNERS`) and the line-based checks (`second_path_declarations`, `no_relative_imports`, `strip_inline_tests`) in the SOTA/RICR evaluation alongside the component DAG.
- **D8 — Work Directory**: Record all phased design artifacts in `docs/dev/architecture_dag/`.

---

## 4. Open Questions for Phase 2 (SOTA Research) & Phase 3 (Design)

- **Q1 (DSL & Macro Decomposition)**: In SOTA declarative architecture tools and idiomatic Rust macro design, what is the cleanest way to structure the specification so that (a) the generic graph model/macro is reusable and decoupled from the domain enum, (b) the domain enum and graph constant do not duplicate component names, and (c) the syntax has minimal macro magic?
- **Q2 (Compile-Time vs. Test-Time Colocation)**: How do SOTA Rust/modular-monolith tools (and Rust's module/visibility system) handle colocated component annotations? What are the exact trade-offs of making `ArchitectureComponent` visible to `src/` under `#[cfg(test)]` or `pub(crate)` (for compiler type-checking + IDE hover/goto-def) vs. keeping it strictly in `tests/architecture.rs`?
- **Q3 (External Crate Encapsulation in the Graph)**: Should third-party crate encapsulation (`AST_GREP_OWNERS` for `ast_grep_core`) be modeled directly as part of the architectural specification (e.g., declaring encapsulated external dependencies on components) or kept as a separate orthogonal rule?
- **Q4 (AST-Aware vs. Line-Based Verification in `tests/architecture.rs`)**: How should `tests/architecture.rs` handle `#[cfg(test)]` stripping, single-path enforcement (`pub use` / `#[macro_export]`), and relative-import checks (`use super::`) so they are robust, idiomatic, and coherent with the rest of the verification engine?
