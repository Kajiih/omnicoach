# Abstraction graph: 04 Prototypes

> **Status: ALPHA / EXPERIMENTAL — Active Investigation & Design** (Phases 4 & 5 evaluated; subject to change at any time). Plan: [03_design.md](03_design.md) §5. Next: [05_decisions.md](05_decisions.md).

Prototype code lives in the Jujutsu workspace `abstraction-graph` (`target/workspaces/abstraction-graph`), never in the main stack (D3, D22).

## 1. Task Log

| Task | Status | Notes |
| :--- | :--- | :--- |
| T1 | Done | Package `prototypes/abstraction_graph` added as a workspace member; no dependency on `omnicoach` (D19, D22). No smoke test: nothing to test yet, so the first RED tests are T2's. |
| T2 | Done | `code_lint::ast::scope_facts` dumps per scope: items, raw relations, public roots, recursion groups, levels, ordering edges with their ADR 011 §3.4 reason, and findings. RED confirmed against a stub (4/4 failing), then GREEN. |
| T3 | Done, partly by design | Composition versus behavior for types: closed (new `definition_callees`, Rust and Python). Parameter versus return for methods: dissolved (F6). Type-to-methods link: by scope label only; per-method references to module items deferred until a track needs them (F6). |
| T4 | Done | `scope_facts_dumper` example writes one JSON file per module; all 8 benchmark modules dumped. Feature checks (9 tests) confirm each module's reason for being in the benchmark. The Python class is `ZoneInfo` (zoneinfo); recursion groups exist in ripgrep `glob.rs`, `rule_selection.rs`, and `ast/rust.rs`, so no extra module is needed. |
| T5 | Done | Layer L1 (`src/model.rs`): tool-agnostic `AbstractionGraph` with disjoint `DeclarationNode` (leaf) and `GroupNode` (container) sets, `OrderingEdge`, `ReferenceEdge`, and `SummaryEdge`. Enforces all §5.2 invariants at construction time (7 RED→GREEN unit tests). |
| T6 | Done | Track `P1_flat` (`src/shapes.rs`): flat declaration graph with recursion-group containers (`GroupKind::RecursionGroup`) where `|SCC| > 1`. |
| T7 | Done | Track `P2_ownership` (`src/shapes.rs`): folds each private item into `Owner(p)` (`|Roots| = 1`), `Shared(S)` (`|Roots| > 1`), or `Unowned` (`|Roots| = 0`), and aggregates cross-group ordering edges into `SummaryEdge`s with reason and count. |
| T8 | Done | Track `P3_members` (`src/shapes.rs`): nests each type's `impl`/`class` methods as a `TypeMembers` subgroup inside the type's ownership group, preserving intra-type method ordering and findings. |
| T9 | Done | Track `P4_bands` (`src/shapes.rs`): wraps top-level ownership groups in `LevelBand { level }` containers (`Lmax` down to `L0`). |
| T10 | Done | Layer L3 (`src/adapters.rs`) & L4 (`src/lib.rs`): deterministic serializers for `.outline.md`, `.cytoscape.json`, `.graphml` (yEd/Gephi), `.dot` (Graphviz `cluster_*`), plus the standalone `viewer.html` shell (Cytoscape.js + `elkjs` + `cytoscape-elk` + `cytoscape-expand-collapse`) and `export_scope` facade (4 RED→GREEN unit tests). |
| T11 | Done | Metrics harness (`src/metrics.rs`) and `export_benchmarks` binary (`src/bin/export_benchmarks.rs`): exports all 36 `(scope, track)` combinations into `out/` and verifies M1, M2, M3, M5, and M7. |
| T12 | Done | M4 controlled evaluation across 4 modules × 8 questions (`32` total) comparing `Outline-only` (`P3_members`) vs `Source-only` against analyzer ground truth. |
| T13 | Done | Interactive walkthrough checkpoint (`M6`) via `prototypes/abstraction_graph/out/viewer.html`; refined with two-level levelization (F12) and adapter invariants (F13). |
| T14 | Done | Consolidated onto `P3_members` as the single canonical graph shape; archived `P1_flat`, `P2_ownership`, and `P4_bands` with rationale in §7 and cleaned up `prototypes/abstraction_graph/`. |

## 2. Findings So Far

| ID | Finding | Evidence |
| :--- | :--- | :--- |
| F1 | The analyzer counts a public item as its own root (`Roots(p) = {p}`), not an empty set. Ownership groups can use this directly: every item's roots name its group. | T2 test `module_scope_items_roots_and_levels` |
| F2 | Scopes had no label (module, class, or impl), so a dump could not say which container it describes. The dump adds `DeclarationScopeKind`; linking a type to its methods (D14) can then match an `impl` or `class` label to the module item of the same name. | T2 |
| F3 | Levels follow Lakos levelization over the ordering edges, with recursion groups as single units; on the fixture they match the intended reading order (entrypoint 3, encapsulating helper 2, contract type 1, leaf 0). | T2 tests |
| F4 | Writing the dump surfaced how strictly the repo's own rules apply to prototype code (nullable collection return, test structure, assertion count). Prototype code is held to the same bar. | T2 dogfood |
| F5 | The analyzer's recursion groups stop at public boundaries, so a cycle through a public entrypoint stays split into several groups. Levelizing over those groups loops forever (stack overflow on `rule_selection.rs`). The dump computes its own groups over all ordering edges. Consequence for the view: "recursion group" in the documentation means real cycles, which is broader than what the linter exempts. | T4 dump crash; test `cycle_through_public_entrypoint_terminates_with_one_level` |
| F6 | Today a struct + `impl` produces two separate scopes: the module scope (sibling functions and types) and the `impl` scope (only that type's methods). Inside the `impl` scope, methods only see other methods of the same type—and since a method's parameter or return type is a *type*, never another *method*, `param_callees` and `return_callees` between sibling methods are always empty. Meanwhile, in the module scope, references from a method inside `impl Foo` to a module helper or type (such as `Foo::parse` taking `Config` or calling `validate`) are rolled up onto the `Foo` type node as a whole rather than attributed to `Foo::parse`. Track P2 only shows `Foo` as a node, so rolled-up edges are sufficient; Track P3 (which expands `Foo`'s methods inside the module graph) will show whether rolled-up module edges + method-to-method edges are sufficient or if per-method-to-module edges are needed. | T3 audit |
| F7 | One file can produce more than one `Module` scope—either from an inline `mod` block (`playground/src/lib.rs` has the root module plus `mod wasm_abi`) or when a name is redefined (`split_at_redefinitions`). The graph model must not assume a 1:1 mapping between file and module scope. | T4 survey |
| F8 | **Disjoint container vs. leaf nodes (`GroupNode` vs `DeclarationNode`) eliminates compound-edge failures.** When a public item `p` is modeled as a leaf `DeclarationNode` inside an `Owner(p)` `GroupNode` rather than making `p` both a container and an edge endpoint, every ordering and reference edge connects leaf declarations and every summary edge connects sibling overview groups. Cytoscape/ELK, GraphML, and Graphviz `cluster_*` all accept the graph without parent-to-child edge errors. | T5, T10 |
| F9 | **Single flat module-wide levels cause upward group summary edges on `SignatureContract` when helpers fold into owners.** When `GroupNode.level` is taken as `max(member.global_level)`, `rule_selection` and `ast_rust` each produce `1` upward summary edge (`Owner(Location) [L2] -> Shared(...) [L4]` and `Owner(RustPathReference) [L3] -> Owner(summarize_rust_file) [L4]`) because a public contract type with no helpers of its own has a lower global level than an entrypoint with a deep private helper chain. | T11 initial run |
| F10 | **Shared-bucket proliferation on large utility modules.** Ownership grouping (`P2`/`P3`) compresses cohesive modules to `1–5` overview groups (`playground_lib`: `2`, `suppression`: `3`, `uninherited_protocol`: `1`, `ripgrep_glob`: `5`, `cargo_fix`: `5`, `cpython_zoneinfo`: `1`). On `ast/rust.rs` (`116` module declarations, `29` public functions/types), distinct combinations of public callers produce `18` separate `Shared(S)` buckets (`47` overview groups total), exceeding the ~20-node readability budget (CP7) and showing that utility hubs with many public functions need higher-level component grouping (Stage B) or sub-bucketing. | T11 metrics |
| F11 | **Rolled-up `impl` behavior edges propagate ownership through shared field types.** In `ripgrep_glob`, `Glob` holds a `Tokens` field (`containment`) while `GlobBuilder::build` calls `Tokens::to_regex_with`, whose `impl Tokens` body calls `char_to_escaped_literal`. Because module-scope behavior of `impl Tokens` is attached to `Tokens` (F6), `Roots(char_to_escaped_literal)` walks `Glob -> Tokens -> char_to_escaped_literal` and assigns `owners = [Glob, GlobBuilder]` even though only `GlobBuilder` calls the regex-compilation method on `Tokens`. | T12 M4 comparison |
| F12 | **Two-level levelization (inter-component `GroupNode.level` + intra-component local `DeclarationNode.level`) eliminates F9 (`M2 = 0 / 0` everywhere) and makes `P4` intra-owner layer bands meaningful.** Comparing declaration levels across unrelated owners is a category error. When `DeclarationNode.level` is computed locally within each ownership bucket (`Owner(p)`, `Shared(S)`, `Unowned`) and `GroupNode.level` is computed over the component summary DAG, upward ordering and summary edges drop to **`0 / 0` across all 36 `(scope, track)` combinations**, `P4_bands` nests `Layer L_k .. Layer L_0` *inside* each multi-layer owner, and UI collapse state is completely decoupled from `L1`. | T13 refinement |
| F13 | **Cytoscape.js + `cytoscape-elk` + `cytoscape-expand-collapse` require five adapter-level invariants in `L3` (`adapters.rs`)**: (1) Cytoscape 3.30.4's deprecated `'width': 'label'` uses a `poolIndex` style-hint hash that throws `TypeError: Cannot read properties of undefined (reading 'index')` when `cytoscape-expand-collapse` removes child nodes during `collapseAll()`, aborting layout on `P2`/`P3`/`P4` — fixed by precomputing `width` in `CytoscapeNodeData`; (2) `cytoscape-elk` layouts all passed elements including `display: none` edges unless restricted to `eles: cy.elements(':visible')` and excluding raw `reference` edges (`SignatureContract` reference edges point `callee -> ParamType`, opposite to `OrderingEdge`, which otherwise tricks ELK's cycle breaker into inverting layers); (3) `summary` edges must hide when either endpoint group is expanded so they do not attach to compound `:parent` borders alongside leaf edges; (4) `LevelBand` (`level_band:*`) containers must remain open as dashed swimlanes inside expanded owners; and (5) singleton `Owner` groups (1 public item with 0 private helpers) should stay collapsed as leaf-styled pills (`is_singleton = "true"`) rather than expanding into redundant 1-item boxes. | T13 visual audit |

## 3. Process Notes

| ID | Note |
| :--- | :--- |
| N1 | Running `jj` commands in two workspaces while builds were snapshotting produced a divergent working-copy commit and briefly hid the T3 edits. They were recovered from the divergent copy. Since then, `jj` commands in the prototype workspace run alone, never alongside commands in the main workspace. |

## 4. Out-of-Scope Defects Noticed

| ID | Defect | Where |
| :--- | :--- | :--- |
| O1 | Fixed: added the missing blank line between `build_rust_module_scope_item` and `enum RustModuleItem` (`rustfmt` collapses multiple blank lines to one, but preserves zero blank lines between adjacent top-level items). | Main stack & prototype workspace, `src/code_lint/ast/rust.rs` |

## 5. Quantitative Metrics (M1, M2, M3, M5, M7)

Generated by `cargo run -p abstraction-graph-proto --bin export_benchmarks` across all 9 benchmark scopes and 4 tracks (`36` combinations) with two-level levelization (F12):

- **M1 (Fidelity)**: `100%` (`true`) on all 36 combinations.
- **M2 (Level Consistency)**: **`0 / 0`** (`0` upward intra-owner declaration ordering edges and `0` upward inter-group summary edges) on **all 36 combinations**.
- **M7 (Determinism)**: `100%` (`true`) on all 36 combinations.

| Scope | Source B (Lines) | Track | M1 | M2 Up (Ord/Sum) | M3 Overview (N/E) | Total Decl (N/E) | M5 Outline B (Lines) | M5 Ratio | M7 |
| :--- | ---: | :--- | :---: | :---: | ---: | ---: | ---: | ---: | :---: |
| `playground_lib` | 10,845 (315) | `P1_flat` | true | 0 / 0 | 11 / 19 | 11 / 19 | 2,618 (57) | 24% | true |
| `playground_lib` | 10,845 (315) | `P2_ownership` | true | 0 / 0 | **2 / 2** | 11 / 19 | 3,097 (67) | 28% | true |
| `playground_lib` | 10,845 (315) | `P3_members` | true | 0 / 0 | **2 / 2** | 11 / 19 | 3,097 (67) | 28% | true |
| `playground_lib` | 10,845 (315) | `P4_bands` | true | 0 / 0 | **2 / 2** | 11 / 19 | 3,346 (71) | 30% | true |
| `suppression` | 35,513 (872) | `P1_flat` | true | 0 / 0 | 16 / 19 | 16 / 19 | 3,667 (61) | 10% | true |
| `suppression` | 35,513 (872) | `P2_ownership` | true | 0 / 0 | **3 / 1** | 16 / 19 | 4,277 (72) | 12% | true |
| `suppression` | 35,513 (872) | `P3_members` | true | 0 / 0 | **3 / 1** | 20 / 20 | 5,005 (79) | 14% | true |
| `suppression` | 35,513 (872) | `P4_bands` | true | 0 / 0 | **3 / 1** | 20 / 20 | 5,325 (84) | 14% | true |
| `uninherited_protocol` | 23,774 (570) | `P1_flat` | true | 0 / 0 | 6 / 8 | 6 / 8 | 1,347 (27) | 5% | true |
| `uninherited_protocol` | 23,774 (570) | `P2_ownership` | true | 0 / 0 | **1 / 0** | 6 / 8 | 1,523 (31) | 6% | true |
| `uninherited_protocol` | 23,774 (570) | `P3_members` | true | 0 / 0 | **1 / 0** | 6 / 8 | 1,523 (31) | 6% | true |
| `uninherited_protocol` | 23,774 (570) | `P4_bands` | true | 0 / 0 | **1 / 0** | 6 / 8 | 1,727 (35) | 7% | true |
| `rule_selection` | 28,071 (806) | `P1_flat` | true | 0 / 0 | 24 / 51 | 24 / 51 | 6,498 (136) | 23% | true |
| `rule_selection` | 28,071 (806) | `P2_ownership` | true | 0 / 0 | **17 / 36** | 24 / 51 | 11,881 (210) | 42% | true |
| `rule_selection` | 28,071 (806) | `P3_members` | true | 0 / 0 | **17 / 36** | 26 / 52 | 12,282 (215) | 43% | true |
| `rule_selection` | 28,071 (806) | `P4_bands` | true | 0 / 0 | **17 / 36** | 26 / 52 | 12,747 (224) | 45% | true |
| `ast_rust` | 126,896 (3,485) | `P1_flat` | true | 0 / 0 | 116 / 165 | 116 / 165 | 30,053 (465) | 23% | true |
| `ast_rust` | 126,896 (3,485) | `P2_ownership` | true | 0 / 0 | **47 / 61** | 116 / 165 | 54,781 (625) | 43% | true |
| `ast_rust` | 126,896 (3,485) | `P3_members` | true | 0 / 0 | **47 / 61** | 116 / 165 | 54,781 (625) | 43% | true |
| `ast_rust` | 126,896 (3,485) | `P4_bands` | true | 0 / 0 | **47 / 61** | 116 / 165 | 57,597 (679) | 45% | true |
| `ripgrep_glob` | 60,771 (1,686) | `P1_flat` | true | 0 / 0 | 10 / 19 | 10 / 19 | 3,434 (54) | 5% | true |
| `ripgrep_glob` | 60,771 (1,686) | `P2_ownership` | true | 0 / 0 | **5 / 9** | 10 / 19 | 4,615 (78) | 7% | true |
| `ripgrep_glob` | 60,771 (1,686) | `P3_members` | true | 0 / 0 | **5 / 9** | 46 / 47 | 14,692 (180) | 24% | true |
| `ripgrep_glob` | 60,771 (1,686) | `P4_bands` | true | 0 / 0 | **5 / 9** | 46 / 47 | 15,119 (185) | 24% | true |
| `cargo_fix` | 53,699 (1,433) | `P1_flat` | true | 0 / 0 | 23 / 31 | 23 / 31 | 6,924 (91) | 12% | true |
| `cargo_fix` | 53,699 (1,433) | `P2_ownership` | true | 0 / 0 | **5 / 4** | 23 / 31 | 7,853 (109) | 14% | true |
| `cargo_fix` | 53,699 (1,433) | `P3_members` | true | 0 / 0 | **5 / 4** | 30 / 32 | 9,522 (120) | 17% | true |
| `cargo_fix` | 53,699 (1,433) | `P4_bands` | true | 0 / 0 | **5 / 4** | 30 / 32 | 10,043 (129) | 18% | true |
| `cpython_zoneinfo` | 25,053 (779) | `P1_flat` | true | 0 / 0 | 11 / 16 | 11 / 16 | 3,680 (51) | 14% | true |
| `cpython_zoneinfo` | 25,053 (779) | `P2_ownership` | true | 0 / 0 | **1 / 0** | 11 / 16 | 3,893 (55) | 15% | true |
| `cpython_zoneinfo` | 25,053 (779) | `P3_members` | true | 0 / 0 | **1 / 0** | 45 / 31 | 12,762 (128) | 50% | true |
| `cpython_zoneinfo` | 25,053 (779) | `P4_bands` | true | 0 / 0 | **1 / 0** | 45 / 31 | 13,198 (133) | 52% | true |
| `cpython_ZoneInfo_class` | 25,053 (779) | `P1_flat` | true | 0 / 0 | 22 / 10 | 22 / 10 | 5,166 (48) | 20% | true |
| `cpython_ZoneInfo_class` | 25,053 (779) | `P2_ownership` | true | 0 / 0 | **17 / 8** | 22 / 10 | 7,435 (94) | 29% | true |
| `cpython_ZoneInfo_class` | 25,053 (779) | `P3_members` | true | 0 / 0 | **17 / 8** | 22 / 10 | 7,435 (94) | 29% | true |
| `cpython_ZoneInfo_class` | 25,053 (779) | `P4_bands` | true | 0 / 0 | **17 / 8** | 22 / 10 | 7,537 (96) | 30% | true |

## 6. M4 Agent Evaluation (Outline-Only vs. Source-Only)

Two isolated `research` subagents answered Q1–Q8 ([03_design.md](03_design.md) §1.3) across 4 representative modules (`suppression`, `rule_selection`, `ripgrep_glob`, `cpython_zoneinfo` — `32` questions total):

- **Condition A (`Outline-only`)**: Allowed to read only the 4 `P3_members.outline.md` files (`44,750` bytes / `606` lines total; `28,233` bytes in `P2_ownership`). Completed in 1 tool-call batch.
- **Condition B (`Source-only`)**: Allowed to read only the 4 raw `.rs`/`.py` source files (`121,955` bytes / `3,174` lines total). Required paginated reads on larger files.

| Question | `Outline-only` (`P3_members`) | `Source-only` (Raw Code) | Why `Source-only` diverged from analyzer ground truth |
| :--- | :---: | :---: | :--- |
| **Q1** (Public entrypoints & types) | **4 / 4** | **4 / 4** | Both identified all public types, functions, and methods (source-only also listed `pub const` items not tracked by the declaration-order analyzer). |
| **Q2** (Exclusive helpers of `X`) | **4 / 4** | **2 / 4** | On `suppression`, missed that `ToolDirective` is unowned (`owners=[]`). On `ripgrep_glob`, attributed `char_to_escaped_literal` and `bytes_to_escaped_literal` solely to `GlobBuilder` instead of `Shared(Glob, GlobBuilder)` (F11). |
| **Q3** (Shared & unowned items) | **4 / 4** | **1 / 4** | On `rule_selection`, included public callers `parse_config`/`load_config` as roots of `Plan` (whereas `Roots(h)` stops at the first public boundary). On `suppression` and `ripgrep_glob`, mirrored the Q2 differences. |
| **Q4** (Type composition, behavior, contracts) | **4 / 4** | **4 / 4** | Both accurately distinguished field composition, `impl` behavior dependencies, and signature contracts. |
| **Q5** (Impact of changing helper `h`) | **4 / 4** | **4 / 4** | Both traced the upward call/dependency chains to the affected public entrypoints and types. |
| **Q6** (`L0` lowest-level items) | **4 / 4** | **2 / 4** | On `suppression` and `rule_selection`, `Source-only` treated signature-contract types (`DirectivePlacement`, `Location`, `unknownLabel`) as `L0` leaves, whereas ADR 011 reverses `SignatureContract` (`Type -> Fn`), placing leaf functions at `L0` and their contract types above them. |
| **Q7** (Cycles / recursion groups) | **4 / 4** | **4 / 4** | Both found `Tokens <-> Token` in `ripgrep_glob` and confirmed no call cycles in the other modules (`Outline-only` additionally reported the mixed-edge SCC `cycle=9` in `rule_selection` and `cycle=5` in `ripgrep_glob`). |
| **Q8** (Misplaced items & lint reasons) | **4 / 4** | **2 / 4** | On `suppression` and `rule_selection` (`0` findings), both matched. On `ripgrep_glob` (`8` findings) and `cpython_zoneinfo` (`9` findings), `Source-only` could not mentally simulate rule dominance and exemptions, over-predicting `private-before-public` and `uncolocated-helper` violations. |
| **Total Exact Match** | **32 / 32 (100%)** | **23 / 32 (72%)** | **Context cost**: `44.8 KB` (`606` lines) for `P3_members` (`28.2 KB` for `P2_ownership`) vs. `122.0 KB` (`3,174` lines) for raw source (**2.7×–4.3× smaller**). |

## 7. Track Comparison and Consolidation (T14)

| Track | What Differs in `AbstractionGraph` (`L1`) | CUJ1–CUJ7 Coverage | Outcome |
| :--- | :--- | :--- | :--- |
| **`P1_flat`** | Module declarations and edges only; `0` `GroupNode`s. | Fails CUJ2 (`Owner`), CUJ3 (`Shared`), CUJ4 (`SummaryEdge`), and CP7 overview readability on modules with `> 15` items. | **Archived** — equivalent to expanding all groups in `P2`/`P3`, without component boundaries. |
| **`P2_ownership`** | Module declarations grouped into `Owner(p)`, `Shared(S)`, `Unowned`, and `RecursionGroup`; omits child `Impl`/`Class` scopes. | Covers CUJ1–CUJ4 and CUJ6–CUJ7 at module scope, but omits method hierarchies inside structs/classes (half of CUJ5). | **Subsumed by `P3_members`** — identical to `P3_members` when `TypeMembers` groups are collapsed (Level 0 and Level 1). |
| **`P3_members`** | `P2_ownership` plus child `ScopeKind::Impl` / `ScopeKind::Class` declarations attached under `GroupKind::TypeMembers { owner_type }`. | Covers **all CUJ1–CUJ7** across 3 progressive zoom levels: Level 0 (`Owner`/`Shared`/`Unowned`), Level 1 (owner declarations + `RecursionGroup`), Level 2 (`TypeMembers` methods). | **Selected as canonical graph shape.** |
| **`P4_bands`** | `P3_members` plus synthetic `GroupKind::LevelBand { level }` containers (`Layer L_k .. Layer L_0`) inside multi-layer owners. | Adds redundant box-in-box nesting in visual compound-graph viewers (`Owner -> Layer L_k -> node`) for vertical layering that ELK (`DOWN`) and `[L_k]` node labels already convey. | **Archived** — `DeclarationNode.level` (local intra-owner level) and `GroupNode.level` (inter-group level) remain on the nodes in `P3_members` without synthetic wrapper groups. |
