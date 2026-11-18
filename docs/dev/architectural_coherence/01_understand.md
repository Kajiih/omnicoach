# Phase 1: Understand — Preventing & Detecting Architectural Duplication, Layer Misplacement, and Dead Exports

## TL;DR

- **Problem**: During the initial implementation of recursive intra-component submodule DAGs (subsequently refactored and squashed into commit `42a41cfa` on `main`), an autonomous coding agent passed `cargo test`, `cargo clippy --all-targets`, and `omnicoach code`, yet extended an unreferenced `pub` constant (`ARCHITECTURE_GRAPH`), duplicated recursive AST structs and DAG validators, and placed language-agnostic `foundation` queries inside `project/python/architecture.rs`.
- **Root Causes Uncovered by Forensic & Codebase Audit**:
  1. **`omnicoach graph` Navigation Blind Spots**: `ScopeFacts` skipped `const` and `static` items (`omnicoach graph slice ARCHITECTURE_GRAPH` returned `0` matches), `CodeLintGraph` omitted `<-` lines on `pub` exports with zero incoming project edges (making unreferenced `ComponentDefinition` look identical to live exports), and `ProjectFacts` discarded `ComponentSpec::submodules`—rendering intra-component `modules: { ... }` hierarchies flat with `[inferred]` edges.
  2. **Compiler Dead-Code Boundaries**: `rustc`'s `dead_code` lint stops at `pub` items reachable from `src/lib.rs` and treats `#[derive(PartialEq, Eq)]` as reading every struct field (`SubmoduleSpec::depends_on`).
  3. **Greedy File-Cursor Patchwork**: Without explicit structural visibility or domain-agnostic design guardrails on shared foundation ownership and self-similar recursive types, the agent split nominal child structs (`DefinedSubmoduleEntry`), forcing duplicate parsers and validators downstream.
- **Goal**: Combine **structural visibility upgrades in `omnicoach graph` and workspace conformance checks** with **concise, domain-agnostic skill guardrails**, validated via a **double-blind Development + Unhinted Holdout subagent benchmark** pairing positive traps against adversarial negative controls ([04_double_blind_benchmark.md](04_double_blind_benchmark.md)).

---

## 1. Specimen Analysis: Initial Recursive Submodule DAG Implementation (`42a41cfa`)

**Context**: Audits the four structural defects introduced by an autonomous coding agent during the initial implementation of recursive intra-component submodule DAGs (before manual cleanup into commit `42a41cfa`), mapping each defect to the static-analysis blind spot and workflow gap that permitted it.

| Failure Category | Concrete Defect Before Cleanup | Tooling & Static Analysis Gap | Agent Workflow Gap |
| :--- | :--- | :--- | :--- |
| **Unreferenced Workspace Exports** | Added `pub struct SubmoduleDefinition` to populate `pub const ARCHITECTURE_GRAPH: &[ComponentDefinition]` in [src/architecture.rs](../../../src/architecture.rs), even though `ARCHITECTURE_GRAPH` and `ComponentDefinition` had zero callers across all workspace targets since commit `fb24710b`. | `rustc`'s `dead_code` lint stops at `pub` items reachable from `src/lib.rs`. `ScopeFacts` dropped `const`/`static` items, single-file `omnicoach graph <file>` inspected only intra-file edges, and multi-file `render_graph` silently omitted `<-` when a `pub` item had zero project callers. | Extended an existing `pub` structure by local pattern symmetry without checking whether it had live workspace consumers. |
| **Derive-Masked Unread Struct Field** | Added `depends_on: Vec<String>` on `pub struct SubmoduleSpec` in [foundation/architecture.rs](../../../src/code_lint/project/foundation/architecture.rs), written in two struct literals and read zero times. | `#[derive(PartialEq, Eq)]` synthesizes `self.depends_on == other.depends_on`, which `rustc`'s `dead_code` pass counts as a field read even when `SubmoduleSpec` is never compared for equality, and `SubmoduleSpec` used wider visibility than `pub(in crate::code_lint::project)`. | Copied fields from `ArchitectureSubmoduleConfig` onto `SubmoduleSpec` after precomputing `allowed_siblings`. |
| **Adapter-Layer Duplication of Foundation Queries** | Implemented `is_declared_python_submodule_path` and `python_submodule_dag_violation` inside [python/architecture.rs](../../../src/code_lint/project/python/architecture.rs) (alongside `append_declared_submodule_isolation_rules` in [rust/architecture.rs](../../../src/code_lint/project/rust/architecture.rs)) instead of on `ComponentSpec` in [foundation/architecture.rs](../../../src/code_lint/project/foundation/architecture.rs). | `ProjectFacts` discarded `ComponentSpec::submodules` and rendered all `CodeLintProject` modules flat with `[inferred]` edges, hiding the `foundation` $\leftarrow$ `{python, rust}` submodule DAG during navigation. | Placed language-agnostic queries inside whichever language adapter file was open in the editor cursor rather than on the shared `foundation` type ([ADR 012](../decisions/012_module_placement_by_abstraction.md) P2/P6). |
| **Nominal Child-Type Split & Duplicate Traversals** | Created `DefinedSubmoduleEntry` + `collect_defined_submodules` in [file_summary.rs](../../../src/code_lint/ast/rust/file_summary.rs) and `validate_submodule_dag` alongside `validate_component_dag` in [foundation/config.rs](../../../src/code_lint/project/foundation/config.rs). | Splitting `DefinedComponentEntry` and `DefinedSubmoduleEntry` into two nominal Rust types forced duplicate CST extraction loops, and no parameterized DAG validation helper was shared across `ArchitectureComponentConfig` and `ArchitectureSubmoduleConfig`. | Modeled syntactic nesting by duplicating structs and functions per level rather than unifying self-similar CST nodes (`Vec<Self>`) and parameterizing shared traversals. |

**Analysis & Interpretation**: Every defect above compiled cleanly and passed both `cargo clippy --all-targets` and single-file/component-level `omnicoach code` checks because module-level constants, intra-component submodule DAGs, and workspace-wide export consumers were invisible in `omnicoach graph` and outside `rustc`'s crate-local `dead_code` boundary.

---

## 2. Goals & Non-Goals

### Goals

1. **Surface Submodule DAGs, Constants, and Zero-Caller Exports in `omnicoach graph`**: Upgrade `ScopeFacts`, `ProjectFacts`, and `CodeLintGraph` so recursive `modules: { ... }` / `submodules` DAGs (with `Convergent`, `Absent`, and `Divergent` Reflexion statuses), `const`/`static` items, and `(0 in-scope callers)` markers on exported symbols appear directly in `omnicoach graph` and `omnicoach graph slice`.
2. **Enforce Workspace Export Liveness & Deduplicate Pre-Existing Adapter Logic in `omnicoach`**: Add a fast workspace-wide export liveness test in [tests/architecture_conformance.rs](../../../tests/architecture_conformance.rs) covering all Cargo workspace members (`src/`, `tests/`, `benches/`, `crates/`, `playground/`), and eliminate pre-existing `python`/`rust` cross-component and AST traversal duplications.
3. **Establish Concise, Domain-Agnostic Skill Guardrails**: Formulate compact (~110 tokens total), instance-agnostic guidance for structural caller verification, foundation-layer ownership across parallel adapters, and unified internal recursion without wire-schema collapse.
4. **Validate via Double-Blind Multi-Split Benchmarking**: Measure both `omnicoach graph` upgrades and skill guardrails across disjoint **Development** (Python) and **Unhinted Holdout** (Rust) multi-layer tasks seeded with both positive defects and adversarial boundary negative controls.

### Non-Goals

1. **No Unsound Untyped AST Lint Rules That Induce Destructive Compliance**: Do not ship untyped `.field` dead-code rules, single-module feature-envy heuristics that violate [ADR 012](../decisions/012_module_placement_by_abstraction.md) P7 (`CodeLintPolicy`, `CodeLintRules`, `RuleCatalog`), or child-struct subset rules that pressure agents to collapse invariant-carrying domain types (`ComponentSpec` vs. `SubmoduleSpec`).
2. **No Whole-Workspace Reverse-Index Scans Inside Single-File `omnicoach hook`**: Preserve the `~30 ms` `omnicoach hook` latency budget and `unused-suppression` invariants; keep whole-workspace dead-export conformance in [tests/architecture_conformance.rs](../../../tests/architecture_conformance.rs) and `omnicoach graph` until a workspace-aggregation rule phase is introduced ([ROADMAP.md](../../../ROADMAP.md)).
3. **No Standalone `TODO` / `FIXME` Comment-Tag Rule**: Comment-tag linting is orthogonal to multi-layer architectural coherence and is already covered by Ruff `flake8-fixme` (`FIX`) / `flake8-todos` (`TD`) and `clippy::todo`.

---

## 3. References

- [ADR 006: Architectural Dependency Graph and Automated Conformance](../decisions/006_architectural_dag_and_conformance.md) — Defines `omnicoach`'s compile-time component and submodule DAG macros (`define_architecture!`, `define_submodules!`) and workspace conformance suite.
- [ADR 012: Module Placement by Abstraction Vocabulary](../decisions/012_module_placement_by_abstraction.md) — Establishes abstraction-vocabulary placement (P1–P2), shared foundation generalization (P6), and separation of AST facts from rule/policy layers (P7).
- [CodePlan: Repository-Level Coding using LLMs and Planning (Bairi et al., ACM FSE 2024)](https://arxiv.org/abs/2309.12499) — Shows how repository-level dependency graphs prevent local file-cursor patchwork during multi-file LLM edits.
