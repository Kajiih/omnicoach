# Phase 3: Architecture, Conformance & Skill Guardrail Design

## TL;DR

Based on the full-repository empirical audit ([02_references.md](02_references.md)), `omnicoach` **rejects untyped per-file AST heuristics** for unread fields, downward feature envy, and child-struct subsets, and addresses multi-layer architectural coherence across three complementary surfaces:

1. **Upgrade `omnicoach graph` (`CodeLintAst` + `CodeLintProject` + `CodeLintGraph`)**:
   - Render recursive intra-component `Submodule` DAGs (`modules: { ... }` / `submodules`) with `Convergent`, `Absent`, and `Divergent` Reflexion statuses via a single recursive container hierarchy builder.
   - Extract module-level `const` and `static` declarations in coalesced `ScopeFacts` so `omnicoach graph`, `find`, and `slice` resolve constants (`ARCHITECTURE_GRAPH` and Python `UPPER_SNAKE_CASE` / `Final` constants).
   - Explicitly render `//   <- (0 in-scope callers)` on exported (`pub`) declarations with zero incoming project edges in multi-file graphs, and append an intra-file scope notice to single-file `omnicoach graph <file>` headers.
2. **Enforce Workspace Export Liveness & Deduplicate Pre-Existing Adapter Logic**:
   - Add `test_workspace_public_exports_have_live_consumers` in [tests/architecture_conformance.rs](../../../tests/architecture_conformance.rs) scanning all Cargo workspace members (`src/` production code and rustdoc fences, `tests/`, `benches/`, `crates/`, `playground/`), remove unreferenced `SourceSpan::from_range` in [src/diagnostic.rs](../../../src/diagnostic.rs), and update [ADR 006](../decisions/006_architectural_dag_and_conformance.md).
   - Unify cross-component dependency + `public_exports` checking (`ArchitectureSpec::check_cross_component_reference`) and submodule-DAG isolation (`ComponentSpec::submodule_dag_violation`) across [rust/architecture.rs](../../../src/code_lint/project/rust/architecture.rs) and [python/architecture.rs](../../../src/code_lint/project/python/architecture.rs) onto [foundation/architecture.rs](../../../src/code_lint/project/foundation/architecture.rs), and unify `find_nested_functions` / `find_nested_classes` in [ast/python/tree.rs](../../../src/code_lint/ast/python/tree.rs).
3. **Domain-Agnostic Skill Guardrails (~110 Tokens Total) Validated Double-Blind**:
   - Pair concise guidance on caller liveness checks, foundation-layer ownership across parallel adapters, and recursive internal IRs vs. distinct wire schemas with a 16-trial double-blind Development + Unhinted Holdout benchmark ([04_double_blind_benchmark.md](04_double_blind_benchmark.md)).

---

## 1. Critical User Journeys & Target Metrics

| Journey | Scenario | Acceptance Criteria | Target Metrics |
| :--- | :--- | :--- | :--- |
| **Structural Navigation & Export Liveness in `omnicoach`** | Run `omnicoach graph` / `slice` and `cargo test --test architecture_conformance` on `omnicoach` (including recursive `CodeLintProject` / `Hook` submodule DAGs and workspace exports). | 1. `omnicoach graph src/code_lint/project` renders nested `foundation`, `python`, `rust`, and `context` submodule containers and intra-component `convergent`/`absent`/`divergent` edges.<br>2. `omnicoach graph slice` resolves `pub const` / `static` items, and multi-file `omnicoach graph --depth public` marks zero-caller `pub` exports explicitly.<br>3. `architecture_conformance` catches dead `pub` items across all workspace members (`playground/`, `crates/`, `tests/`, `benches/`) while adding `0 ms` to `omnicoach code` and `omnicoach hook`. | - **`omnicoach hook` latency delta**: `0 ms`<br>- **Dead-export recall on pre-cleanup specimen**: `100%`<br>- **False positives across workspace members**: `0` |
| **Multi-Layer Feature Authoring Under Positive Traps & Adversarial Negative Controls** | Task blind subagents with implementing multi-layer features in unfamiliar Python (Development) and Rust (Unhinted Holdout) repositories containing dead legacy exports, parallel dialect/codec adapters over a shared foundation, self-similar internal IRs, and locked boundary schemas. | 1. Avoids extending unreferenced legacy exports while **preserving** locked public SDK entrypoints.<br>2. Places dialect/codec-agnostic operations in the shared foundation module while **keeping** dialect/codec-specific wire quoting or escaping in its adapter.<br>3. Uses one recursive internal IR + parameterized validator while **keeping** strict root/nested wire schemas distinct so invalid child keys are rejected. | - **Architectural & Boundary Alignment (`%`)** on pre-committed out-of-workspace oracles<br>- **Negative-Control Boundary Preservation**: `100%`<br>- **Planner Steps & Output Tokens** from `transcript.jsonl` |

---

## 2. Structural Tooling & Conformance Design

### 2.1 `omnicoach graph` Upgrades (`CodeLintAst` $\to$ `CodeLintProject` $\to$ `CodeLintGraph`)

| Component | Change | Architectural Invariant ([ADR 012](../decisions/012_module_placement_by_abstraction.md)) |
| :--- | :--- | :--- |
| **`CodeLintAst`** ([call_clusters.rs](../../../src/code_lint/ast/call_clusters.rs), [call_clusters/scope_graph.rs](../../../src/code_lint/ast/call_clusters/scope_graph.rs), [rust/declaration_order.rs](../../../src/code_lint/ast/rust/declaration_order.rs), [python/declaration_order.rs](../../../src/code_lint/ast/python/declaration_order.rs), [scope_facts.rs](../../../src/code_lint/ast/scope_facts.rs)) | Add `ScopeItemKind::Constant` (module-level `const`/`static` in Rust and `UPPER_SNAKE_CASE` / `Final` module constants in Python) to `ScopeExtractionMode::CoalescedModule`, filtered out of `scope_findings` so callable/type ordering rules remain unchanged. | Pure syntax fact in `CodeLintAst`; makes `ARCHITECTURE_GRAPH` and Python module constants queryable in `graph`, `find`, and `slice`. |
| **`CodeLintProject`** ([foundation/facts.rs](../../../src/code_lint/project/foundation/facts.rs), [rust/facts.rs](../../../src/code_lint/project/rust/facts.rs), [python/facts.rs](../../../src/code_lint/project/python/facts.rs)) | Preserve recursive `ComponentSpec::submodules` on `ComponentFact` and record each `ModuleFact`'s `submodule_path` within its component. | Language-agnostic architectural fact owned by `foundation/facts.rs`. |
| **`CodeLintGraph`** ([graph.rs](../../../src/code_lint/graph.rs), [model.rs](../../../src/code_lint/graph/model.rs), [builder.rs](../../../src/code_lint/graph/builder.rs), [index.rs](../../../src/code_lint/graph/index.rs), [render.rs](../../../src/code_lint/graph/render.rs), [query.rs](../../../src/code_lint/graph/query.rs)) | 1. Nest recursive `GroupKind::Submodule` groups inside `GroupKind::Component` groups via a single shared `ContainerHierarchyBuilder` (`build_sibling_reflexion_edges` shared across top-level components, nested submodule containers, and leaf declared modules) and compute `ReflexionStatus` (`Convergent`, `Absent`, `Divergent`) on declared sibling edges.<br>2. Add `DeclarationKind::Constant`.<br>3. Render `//   <- (0 in-scope callers)` on project-backed `pub` module-level declarations when `in_edges.is_empty()`, contract `ProjectDepth::PublicSurface` (`--depth public`) reference edges from private helpers onto their `Roots(h)` public owners, and emit ``// Intra-file edges only; run `omnicoach graph slice <symbol>` for cross-module callers.`` below single-file `omnicoach graph <file>` `# Scope:` headers. | Keeps `CodeLintGraph` purely descriptive (no normative lint diagnostics in `graph`), while closing all three structural visibility gaps from [01_understand.md](01_understand.md). |

### 2.2 Evaluated Options Rejected or Deferred (With Triggers)

| Option | Decision | Rationale & Future Trigger |
| :--- | :---: | :--- |
| **`misplaced-layer-function` (`ProjectCodeRule`)** | **Rejected** | Produced `59` false positives across `12` components (`96.6%` FP rate), flagging `CodeLintPolicy`, `CodeLintRules`, and `RuleCatalog` in violation of `ADR 012` P7, plus `50%` false positives in `project/python/` on `&str` module-path helpers. |
| **`redundant-child-struct` (`CodeRule`)** | **Rejected** | `0%` precision on clean `main` (`SubmoduleSpec` $\subset$ `ComponentSpec`); pressures agents to collapse distinct domain types into fat structs that permit invalid states, and breaks `rule_test!` repetition. |
| **Write-only struct field AST rule** | **Rejected** | `0%` recall on homonymous fields (`depends_on`) without HIR/Python type inference; false positives on closures, `thiserror` format fields, RAII guards, and `NamedTuple`/`TypedDict`/`Pydantic` models. |
| **4-gram token-Dice `duplicate-sibling-function` (`CodeRule`)** | **Deferred** | Lexer-token shingling missed `validate_submodule_dag` (`0.798`) while flagging 13-line symmetrical `token_before_double_colon` (`1.000`), and breaks `rule_test!` repetition. *Trigger to revisit*: a cross-language AST statement-tree sub-isomorphism design with `RepeatCheck` support achieving $\ge 85\%$ precision across `omnicoach` and external corpora. |
| **Workspace-wide `unused-public-export` (`ProjectCodeRule`)** | **Deferred** | Tracked in [ROADMAP.md](../../../ROADMAP.md) §1. *Trigger*: once `CodeLintRunner` introduces a workspace-aggregation rule phase separate from single-file `omnicoach hook` and multi-crate Cargo/`uv` workspace federation lands in `ProjectIndex`. |
| **Mandatory Per-Symbol Ownership Markdown Table in Phase 3** | **Rejected** | Overfits to recursive AST shapes, adds `500–1,500` tokens of boilerplate on routine tasks, and duplicates `omnicoach graph`. Replaced by ~110 tokens of domain-agnostic skill guidance. |

### 2.3 Domain-Agnostic Skill Guardrails (~110 Tokens Total)

1. **`omnicoach` Skill (§2, under `omnicoach graph`)**:
   - Before adding fields, variants, or companion types to existing exported types, constants, or functions, inspect component and submodule DAGs with `omnicoach graph <dir>` and batch candidate symbols in `omnicoach graph slice <symbol>...`—never build parallel child types or new fields onto unreferenced internal remnants (preserve public facade/SDK entrypoints, remove exports your own refactor orphaned, and leave pre-existing unreferenced remnants untouched unless asked).
2. **`software-design` Skill (+2 principles)**:
   - *Own shared domain operations in the foundation layer across parallel adapters*: When isolated sibling modules (such as per-language, per-format, or per-dialect adapters) depend on a shared foundation layer, place operations whose contract uses only foundation types plus primitive parameters in that foundation layer rather than duplicating them per adapter; keep adapter-specific wire syntax, quoting, or escaping inside its owning adapter.
   - *Use one recursive or parameterized internal model while preserving distinct wire schemas*: Model self-similar internal hierarchies with one recursive type and one traversal/validator; keep root and child wire/config schemas distinct when external field validation or domain invariants differ, while still sharing one parameterized traversal or validator across both.
3. **`phased-engineering-workflow` Skill (Phase 3 & Phase 5)**:
   - *Phase 3*: When a change spans multiple layers or parallel sibling adapters, state which shared foundation module owns the common representation and validation so adapters only translate syntax or wire formats.
   - *Phase 5*: Audit the diff for newly introduced parallel types or functions (within a file or across sibling modules) that belong in one recursive type or parameterized helper, and verify modified exports have live callers.

---

## 3. References

- [Software Reflexion Models: Bridging the Gap between Source and High-Level Models (Murphy, Notkin & Sullivan, ACM SIGSOFT FSE 1995)](https://doi.org/10.1145/222124.222136) — Recursive architectural container edges (`Convergent`, `Absent`, `Divergent`) generalized from top-level components to intra-component submodule DAGs.
- [Knip: Entry Files & Workspace Analysis](https://knip.dev/overview/entry-files) — Multi-target workspace export reachability across library, binary, test, benchmark, and member-package roots.
- [MIPROv2 (Opsahl-Ong et al., EMNLP 2024)](https://arxiv.org/abs/2406.11695) & [TextGrad (Yuksekgonul et al., *Nature* 2025)](https://arxiv.org/abs/2406.07496) — Three-way `Calibration` / `Development` / `Holdout` split and instance-agnostic skill phrasing.
- [UK AISI Inspect AI — Sandboxing](https://inspect.aisi.org.uk/sandboxing.html), [Specification Gaming (Krakovna et al., DeepMind)](https://deepmind.google/discover/blog/specification-gaming-the-flip-side-of-ai-ingenuity/), & [Adding Error Bars to Evals (Miller, 2024)](https://arxiv.org/abs/2411.00640) — Hermetic per-sample trial isolation, pre-committed out-of-workspace behavioral + AST oracles with adversarial negative controls, and paired multi-replicate evaluation.
