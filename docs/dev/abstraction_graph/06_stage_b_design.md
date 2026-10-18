# Abstraction graph: 06 Stage B Understand and Design

> **Status: ALPHA / EXPERIMENTAL — Active Investigation & Design** (Stage B — Phases 1, 2, and 3; subject to change at any time).
> Builds on: [01_understand.md](01_understand.md), [02_references.md](02_references.md), [03_design.md](03_design.md), [04_prototypes.md](04_prototypes.md), [05_decisions.md](05_decisions.md).

## 1. Stage B Problem & Scope (Phase 1: Understand)

Stage A proved that `omnicoach`'s per-module ownership (`Roots(h)`) and ADR 011 abstraction ordering produce an accurate (`M1 = 100%`, `M2 = 0/0`), compact (`14%–50%` of source bytes), high-leverage (`M4 = 100%` vs. `72%`) abstraction view inside a single module.

**Stage B Goal**: Generalize the same hierarchical graph model to a **whole project or package** (`Crate/Package → Component → Module/File → Public Owner → Private Implementation → Type Members`), with configurable depth and 3-way Reflexion edges between declared components.

### 1.1 Existing Codebase Machinery [V]

| Capability | Where in `omnicoach` | What it provides for Stage B |
| :--- | :--- | :--- |
| **Declared Component DAG** | `define_architecture!` in [architecture.rs](../../../src/architecture.rs) (RI2) | `18` `ArchitectureComponent` variants with explicit allowed dependencies (`depends_on`), verified acyclic at compile time. |
| **File-to-Component Mapping** | `architecture_component!(...)` + `resolve_inherited_component_with_root` in [architecture_conformance.rs](../../../tests/architecture_conformance.rs) | Maps every `.rs` file under `src/` to its owning `ArchitectureComponent` by climbing its module path (`code_lint::rules::*` → `CodeLintRules`). |
| **Rust Cross-File References** | `summarize_rust_file` (`RustFileSummary`) in [rust.rs](../../../src/code_lint/ast/rust.rs) + `resolve_reference_path` in [architecture_conformance.rs](../../../tests/architecture_conformance.rs) (RI3) | Single-pass extraction of `external_mods`, `visible_uses` (`pub use`), and `referenced_paths` (`crate::`, `super::`, `self::`), normalized to canonical crate-relative module/item paths. |
| **Python Cross-File Imports** | `ImportMap` / `resolve_name` in [imports.rs](../../../src/code_lint/ast/imports.rs) | Resolves top-level `import a.b` and `from .mod import sym` bindings per `.py` file. |
| **Intra-Module Abstraction Graph** | `collect_scope_facts` in `src/code_lint/ast/scope_facts.rs` + `build_graph` in `prototypes/abstraction_graph/src/shapes.rs` | Stage A `P3_members` ownership + ordering graph per file. |

### 1.2 Stage B Decisions (User-Validated)

| ID | Decision | Source |
| :--- | :--- | :--- |
| **D28** | **Unified 5-level containment hierarchy** with configurable depth (`ProjectDepth`): `Component` → `Module` → `Owner`/`Shared`/`Unowned` → `Declaration`/`RecursionGroup` → `TypeMembers`. | User |
| **D29** | **Configurable depth cutoff (`ProjectDepth`)**:<br>1. `ModuleOnly` (`Component → Module`)<br>2. `PublicSurface` (`Component → Module → Public API items`)<br>3. `Full` (`Component → Module → Stage A P3_members hierarchy down to private helpers and methods`). | User |
| **D30** | **3-Way Reflexion edges between declared components (`Q11`)**:<br>1. `Convergent` (declared in `define_architecture!` **and** `count > 0` actual references)<br>2. `Absent` (declared in `define_architecture!`, but `count == 0` actual references)<br>3. `Divergent` (not allowed by `define_architecture!`, `count > 0` actual references)<br>4. `Inferred` (when no declared architecture exists, `Q10`). | User |
| **D31** | **Fallback for projects without `define_architecture!` (`Q10`)**: infer `Component` groups from crate/package/directory boundaries (e.g. `ripgrep/crates/*` or top-level module subtrees in Rust; package directories or module prefixes in Python). Evaluate on `omnicoach` (`src/`), `ripgrep` (`crates/ignore` + workspace crates), and `cpython` (`Lib/zoneinfo` + `Lib/urllib`). | User |

## 2. Stage B Reader Journeys (CUJ8–CUJ11) & Metrics

### 2.1 Critical User Journeys

| ID | Journey | Acceptance Criteria |
| :--- | :--- | :--- |
| **CUJ8** | **Component Architecture & Reflexion Audit**: A reader opens the project overview of `omnicoach` to see its `18` components from entrypoints (`Bin`, `TestUtils`) down to foundations (`Diagnostic`, `Diff`, `Architecture`), and checks whether `define_architecture!` matches actual imports. | Default view shows only `Component` nodes (`<= 20` nodes) ordered top-to-bottom (`M2 = 0` upward component edges), with `Convergent`, `Absent` (unused declared), and `Divergent` (violation) edges visually and textually distinct. |
| **CUJ9** | **Component → Module Drill-Down**: A reader expands a multi-file component (e.g. `CodeLintAst` or `CodeLintSemantic`) to see its constituent files and how they depend on one another. | Expanding a `Component` reveals its `Module` nodes ordered top-to-bottom by intra-component module dependencies (`ast.rs` above `ast/rust.rs` and `ast/python.rs`; `ast/rust.rs` above `ast/imports.rs`). |
| **CUJ10** | **Cross-Module API & Full Implementation Drill-Down**: A reader inspects which public declarations of a module are used across modules (`PublicSurface`) or drills all the way into a module's `Owner` groups and private helpers (`Full`). | In `PublicSurface` and `Full` modes, cross-file references resolve to the target module (and target public declaration when matched), and expanding a `Module` in `Full` mode reveals its Stage A `Owner`/`Shared` groups without layout crashes. |
| **CUJ11** | **Unannotated Rust & Python Package Fallback (`Q10`)**: A reader opens `ripgrep` or `cpython` packages that have no `define_architecture!` macro. | The builder groups files by crate/package/submodule hierarchy (`Inferred` reflexion status) and orders both components and modules top-to-bottom with `M2 = 0`. |

### 2.2 Evaluation Questions for M4 (Project-Level Agent Comprehension)

To test **M4** at project scale, isolated `Outline-only` vs. `Source-only` subagents answer:

- **QB1**: What are the top-level entry components and lowest-level (`L0`) foundation components of the project, and what is the maximum component dependency depth?
- **QB2**: Are there any **unused declared dependencies** (`Absent`: declared in `define_architecture!`, `0` actual code references) or **undeclared dependency violations** (`Divergent`) between components? Name every pair.
- **QB3**: Inside component `C` (e.g., `CodeLintAst` or `CodeLintSemantic`), which module is the façade/root and what is the internal dependency order among its child modules?
- **QB4**: Which external components and modules depend on component `X` (blast radius of changing `X`), and which specific public items of `X` do they reference most?
- **QB5**: In an unannotated package (`ripgrep/crates/ignore` or `cpython/Lib/urllib`), what is the module dependency DAG and are there any cross-module dependency cycles?

## 3. Ordered Execution Plan (Phase 3 & 4)

| # | Task | Verification |
| :--- | :--- | :--- |
| **TB1** | **Project Facts Dumper (`L0`)**: Add `ProjectFactsFile` (components, declared component edges, per-file `ScopeFacts`, and resolved cross-file references for Rust and Python) to `scope_facts.rs` and a `project_facts_dumper` binary; extend `prototypes/abstraction_graph/src/facts.rs`. | Unit tests on Rust (`crate::`/`super::`/`self::` + `pub use` re-exports) and Python (`import` / `from .mod import`) cross-file resolution. |
| **TB2** | **Benchmark Corpus Dump**: Dump `omnicoach` (`src/`), `ripgrep` (`crates/ignore`), and `cpython` (`Lib/zoneinfo` and `Lib/urllib`) into `prototypes/abstraction_graph/benchmark_projects/`. | Integration test `tests/project_benchmark_features.rs` verifying component counts, file counts, and cross-file edges. |
| **TB3** | **Model Extension (`L1`)**: Add `GroupKind::Component`, `GroupKind::Module`, `EdgeReason::CrossModule`, and `ReflexionStatus` (`Convergent`, `Absent`, `Divergent`, `Inferred`) to `SummaryEdge` in `model.rs`. | Unit tests for new invariants (every `Module` inside at most one `Component`, valid `SummaryEdge` reflexion status). |
| **TB4** | **Project Graph Builder (`L2`)**: Implement `build_project_graph(facts, depth: ProjectDepth)` in `shapes.rs` supporting `ModuleOnly`, `PublicSurface`, and `Full`, with 3-way Reflexion comparison and 3-scale levelization (`Component` → `Module` → Intra-module). | Unit tests for `ModuleOnly`, `PublicSurface`, `Full`, and 3-way Reflexion classification (`Convergent`, `Absent`, `Divergent`). |
| **TB5** | **Adapters & Interactive Viewer (`L3`)**: Update `Outline`, `CytoscapeJson` (`viewer.html`), `GraphML`, and `DOT` adapters to render `Component` and `Module` groups, 3-way Reflexion edge styles (solid blue / dashed grey / red violation), and multi-level `+1 Layer` expansion. | Deterministic adapter tests + headless `node` + `cytoscape` + `elkjs` + `resvg` visual verification on `omnicoach`, `ripgrep_ignore`, and `cpython_urllib`. |
| **TB6** | **Project Metrics & M4 Evaluation**: Export all project benchmarks across `ModuleOnly`, `PublicSurface`, and `Full` in `export_benchmarks.rs`, compute `M1`–`M7`, and run the blind `M4` agent comparison (`Outline-only` vs. `Source-only` on `QB1`–`QB5`). | Record quantitative tables and `M4` results in `07_stage_b_results.md`. |
