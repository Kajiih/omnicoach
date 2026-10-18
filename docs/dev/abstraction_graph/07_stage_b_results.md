# Abstraction graph: 07 Stage B Prototypes, Evaluation, and Results

> **Status: ALPHA / EXPERIMENTAL — Active Investigation & Design** (Stage B Investigation — Phases 4, 5, 6, and 7; subject to change at any time).
> Builds on: [05_decisions.md](05_decisions.md), [06_stage_b_design.md](06_stage_b_design.md).

## 1. Stage B Implementation Summary (`TB1`–`TB5`)

| Layer | Implementation | Key Capabilities Added in Stage B |
| :--- | :--- | :--- |
| **`L0` Facts** | `src/code_lint/ast/scope_facts.rs`, `src/bin/scope_facts_dumper.rs`, `prototypes/abstraction_graph/src/facts.rs` | `ProjectFactsFile` with `ComponentFact`, `ModuleFact`, and `CrossModuleRefFact`. Extracts `define_architecture!` (`architecture.rs`) + `architecture_component!(...)` when present, or falls back (`D31`) to crate/package hierarchy. Resolves cross-file references for Rust (`crate::`, `super::`, `self::`, `pub use` re-exports) and Python (`import`, `from .mod import`). |
| **`L1` Model** | `prototypes/abstraction_graph/src/model.rs` | Added `GroupKind::Component`, `GroupKind::Module { path }`, `EdgeReason::CrossModule`, and `ReflexionStatus` (`Convergent`, `Absent`, `Divergent`, `Inferred`) on `SummaryEdge`. Enforces `count == 0` iff `reflexion == Some(Absent)` and compares `GroupNode.level` across sibling groups for non-`Divergent` summary edges. |
| **`L2` Builder** | `prototypes/abstraction_graph/src/shapes.rs` | `build_project_graph(project, depth: ProjectDepth)` supporting `ModuleOnly`, `PublicSurface`, and `Full` (`D28`, `D29`), 3-scale levelization (`Component` DAG $\to$ intra-component `Module` DAG $\to$ intra-module `Owner`/`Declaration` DAG), and 3-way Reflexion comparison (`D30`) against the transitive closure of `define_architecture!`. |
| **`L3` Adapters** | `prototypes/abstraction_graph/src/adapters.rs`, `prototypes/abstraction_graph/src/bin/export_benchmarks.rs` | Updated `Outline`, `CytoscapeJson` (`viewer.html`), `GraphML`, and `DOT` adapters for 5-level compound hierarchies, 3-way Reflexion edge styling (`convergent` green, `absent` dashed amber, `divergent` red, `inferred` blue), and multi-level `+1 Layer` progressive expansion. |

---

## 2. Quantitative Evaluation (`M1`, `M2`, `M3`, `M5`, `M7` & Reflexion Breakdown)

Exported by `cargo run --manifest-path prototypes/abstraction_graph/Cargo.toml --bin export_benchmarks` across all 4 project benchmarks and all 3 `ProjectDepth` modes (`12` project graphs total):

| Project (`ProjectDepth`) | Source Bytes (Lines) | M1 Exact | M2 Up (`Ord/Sum`) | M3 View (`N/E`) | Total (`Comp / Mod / Decl`) | Reflexion (`C / A / D / I`) | M5 Outline Bytes (Lines) | M5 Ratio | M7 Det |
| :--- | ---: | :---: | :---: | ---: | ---: | :---: | ---: | ---: | :---: |
| `proj_omnicoach_module_only` | 1,841,035 (47,627) | `true` | `0 / 0` | `21 / 68` | `21 / 108 / 0` | `67 / 1 / 0 / 0` | 84,410 (873) | **4%** | `true` |
| `proj_omnicoach_public_surface` | 1,841,035 (47,627) | `true` | `0 / 0` | `21 / 68` | `21 / 108 / 322` | `67 / 1 / 0 / 0` | 168,704 (2,406) | **9%** | `true` |
| `proj_omnicoach_full` | 1,841,035 (47,627) | `true` | `0 / 0` | `21 / 68` | `21 / 108 / 935` | `67 / 1 / 0 / 0` | 433,409 (6,423) | **23%** | `true` |
| `proj_ripgrep_ignore_module_only` | 245,080 (7,042) | `true` | `0 / 0` | `8 / 20` | `1 / 8 / 0` | `0 / 0 / 0 / 20` | 1,601 (37) | **<1%** | `true` |
| `proj_ripgrep_ignore_public_surface` | 245,080 (7,042) | `true` | `0 / 0` | `8 / 20` | `1 / 8 / 28` | `0 / 0 / 0 / 20` | 8,195 (195) | **3%** | `true` |
| `proj_ripgrep_ignore_full` | 245,080 (7,042) | `true` | `0 / 0` | `8 / 20` | `1 / 8 / 282` | `0 / 0 / 0 / 20` | 56,071 (928) | **22%** | `true` |
| `proj_cpython_zoneinfo_module_only` | 37,453 (1,174) | `true` | `0 / 0` | `4 / 5` | `1 / 4 / 0` | `0 / 0 / 0 / 5` | 750 (18) | **2%** | `true` |
| `proj_cpython_zoneinfo_public_surface` | 37,453 (1,174) | `true` | `0 / 0` | `4 / 5` | `1 / 4 / 10` | `0 / 0 / 0 / 5` | 2,041 (36) | **5%** | `true` |
| `proj_cpython_zoneinfo_full` | 37,453 (1,174) | `true` | `0 / 0` | `4 / 5` | `1 / 4 / 61` | `0 / 0 / 0 / 5` | 12,541 (191) | **33%** | `true` |
| `proj_cpython_urllib_module_only` | 141,833 (3,985) | `true` | `0 / 0` | `6 / 9` | `1 / 6 / 0` | `0 / 0 / 0 / 9` | 1,117 (24) | **<1%** | `true` |
| `proj_cpython_urllib_public_surface` | 141,833 (3,985) | `true` | `0 / 0` | `6 / 9` | `1 / 6 / 90` | `0 / 0 / 0 / 9` | 17,901 (342) | **12%** | `true` |
| `proj_cpython_urllib_full` | 141,833 (3,985) | `true` | `0 / 0` | `6 / 9` | `1 / 6 / 255` | `0 / 0 / 0 / 9` | 65,061 (1,029) | **45%** | `true` |

### Key Quantitative Takeaways

1. **Exact Fidelity (`M1 = 100%`) and Determinism (`M7 = 100%`)**: Every project graph across all 4 corpora and all 3 depths matches the raw component, module, cross-module reference, and cross-component reference counts (`M1 = true`) and produces byte-identical outputs across `Outline`, `CytoscapeJson`, `GraphML`, and `DOT` (`M7 = true`).
2. **Zero Upward Edges (`M2 = 0 / 0`)**: Three-scale levelization (`Component` $\to$ `Module` $\to$ Intra-module) yields `0` upward ordering edges and `0` upward sibling summary edges across all 12 configurations.
3. **Compact Default Overviews (`M3`)**:
   - Single-component packages (`ripgrep/crates/ignore`, `cpython/Lib/zoneinfo`, `cpython/Lib/urllib`) auto-expand their sole component wrapper to show their `4–8` constituent modules and `5–20` cross-module summary edges on the first screen (`<= 20` nodes).
   - `omnicoach` (`108` modules, `935` declarations, `47,627` lines of Rust) collapses its `108` modules into `21` architectural components (`L7` down to `L0`) with `68` component summary edges.
4. **Massive Context Compression at `ModuleOnly` and `PublicSurface` (`M5`)**:
   - On `omnicoach`, `ModuleOnly` compresses `1.84 MB` (`47,627` lines) of Rust into `84 KB` (`873` lines, **4%** of source bytes — a **21.8× reduction**), and `PublicSurface` adds all `322` public API declarations and cross-file contracts in `168 KB` (`2,406` lines, **9%** of source bytes — an **11× reduction**).
   - On `ripgrep/crates/ignore` (`245 KB`, `7,042` lines), `PublicSurface` is **3%** of source bytes (`8.2 KB`, `195` lines).

---

## 3. Blind `M4` Agent Evaluation (Project-Level Comprehension)

### 3.1 Initial 5-Question Unlimited-Budget Probe (`QB1`–`QB5`)

Two isolated `research` subagents were first spawned with `QB1`–`QB5` from [06_stage_b_design.md](06_stage_b_design.md) §2.2 without a tool-call cap:

| Question | Focus | Outline-Only (`proj_*.outline.md`) | Source-Only (Unlimited Budget) |
| :--- | :--- | :--- | :--- |
| **QB1** | `omnicoach` component hierarchy (`Lmax` entrypoints, `L0` foundations, max depth) | **1 / 1** (`[L7] Bin` down to 5 `[L0]` components: `Architecture`, `CommandLintCommand`, `CommandLintVcs`, `Diagnostic`, `Diff`; 8 layers `L0..L7`) | **1 / 1** (Identified `Bin` at `L7`, all 5 `L0` components, and the 8-layer topological chain `L0..L7`) |
| **QB2** | `omnicoach` 3-Way Reflexion audit (`Absent` unused declared edges & `Divergent` violations) | **1 / 1** (Identified the exact 1 `Absent` edge `CodeLintSuppression -> CodeLintContract [x0, reflexion=absent]` and `0` `Divergent` edges) | **1 / 1** (Manually cross-checked all 38 direct `define_architecture!` edges against `src/` imports to find `CodeLintSuppression -> CodeLintContract` unused and `0` transitive violations) |
| **QB3** | `RuleDeclaration` component $\to$ module drill-down & internal cycles | **1 / 1** (Listed all 4 modules, `[L1]` vs. `[L0]` levels, exact reference counts, and the 2-module cycle `rule_declaration <-> rule_declaration::options`) | **1 / 1** (Inspected all 4 files under `src/rule_declaration*` and found `use super::closest_match` in `options.rs` forming the 2-module cycle with `rule_declaration.rs`) |
| **QB4** | Blast radius of `CodeLintSemantic` across components & submodules | **1 / 1** (`CodeLintContract [x8]`, `CodeLintRules [x6]`, `CodeLintSuppression [x5]`, broken down by `bindings [x6]`, `calls [x6]`, `comments [x7]`) | **1 / 1** (Grepped all references to `code_lint::semantic` across `src/` to find the 3 direct dependent components and exact submodule imports) |
| **QB5** | Unannotated packages (`cpython/Lib/zoneinfo` DAG & `ripgrep/crates/ignore` SCC) | **1 / 1** (`zoneinfo` `L2 -> L1 -> L0` with 5 edges; `ignore` `[L0] default_types` + 7-module SCC at `[L1]`) | **1 / 1** (Read all 4 `.py` files in `zoneinfo` and all 8 `.rs` files in `ignore/src` to trace the 7-module SCC and `[L0] default_types`) |
| **Total Score** | **Accuracy on `QB1`–`QB5`** | **5 / 5 (100%)** | **5 / 5 (100%)** |
| **Efficiency** | **Tool Calls / Wall-Clock Time** | **5 tool calls (~65s)** | **37 tool calls (~300s — 7.4× more calls, 4.6× slower)** |

### 3.2 Expanded 24-Question Matrix (`6 Categories × 4 Projects`) Across 4 Conditions

To test where each `ProjectDepth` (`ModuleOnly`, `PublicSurface`, `Full`) adds or loses comprehension signal and how `Outline` compares to `Source-Only` under an **equal tool-call budget (`≤ 12` read calls)**, we evaluated 4 isolated `research` subagents across **24 questions (`96` graded responses)** spanning all 4 benchmark projects (`omnicoach`, `ripgrep_ignore`, `cpython_zoneinfo`, `cpython_urllib`):

- **`Q1` — Component & Module Layering**: `Lmax` to `L0` component/module hierarchy.
- **`Q2` — Reflexion Conformance & Cross-Module Cycles**: `Absent` / `Divergent` edges and module-level SCC cycles.
- **`Q3` — Cross-Module Blast Radius & Reference Counts**: Exact incoming cross-module dependents and reference counts.
- **`Q4` — Public API Surface & Entrypoint Layering**: Public declarations and their intra-module `[Lk]` levels.
- **`Q5` — Intra-Module Ownership**: `Owner(p)` vs. `Shared(S)` vs. `Unowned` partitions of private helpers.
- **`Q6` — Type Members, Typed Ordering Reasons & ADR 011 Findings**: Method-level ordering (`type_members`), `[containment]` / `[encapsulation]` chains, and placement findings (`[callee-before-caller]`, `[uncolocated-helper]`, `[private-before-public]`).

#### Summary by Category (`24` Questions × `4` Conditions = `96` Evaluations)

| Question Category (`4 projects` each) | `Outline: ModuleOnly` (`5` calls, `0.8%–4%` bytes) | `Outline: PublicSurface` (`7` calls, `3%–12%` bytes) | `Outline: Full` (`10` calls, `22%–57%` bytes) | `Source-Only (≤12-call budget)` (`11` calls, raw `.rs`/`.py`) |
| :--- | :---: | :---: | :---: | :---: |
| **`Q1` — Component & Module Layering** | **`4.0 / 4` (`100%`)** | **`4.0 / 4` (`100%`)** | **`4.0 / 4` (`100%`)** | **`4.0 / 4` (`100%`)** |
| **`Q2` — Reflexion & Cross-Module Cycles** | **`4.0 / 4` (`100%`)** | **`4.0 / 4` (`100%`)** | **`4.0 / 4` (`100%`)** | `3.0 / 4` (`75.0%`) |
| **`Q3` — Cross-Module Blast Radius & Counts** | **`4.0 / 4` (`100%`)** | **`4.0 / 4` (`100%`)** | **`4.0 / 4` (`100%`)** | `1.25 / 4` (`31.3%`) |
| **`Q4` — Public API Surface & Entrypoint Levels** | `0.0 / 4` (`0%`) | `3.5 / 4` (`87.5%`) | **`4.0 / 4` (`100%`)** | `3.25 / 4` (`81.3%`) |
| **`Q5` — Intra-Module Ownership (`Owner`/`Shared`/`Unowned`)** | `0.0 / 4` (`0%`) | `0.0 / 4` (`0%`) | **`4.0 / 4` (`100%`)** | `3.5 / 4` (`87.5%`) |
| **`Q6` — Type Members, Ordering Reasons & Findings** | `0.0 / 4` (`0%`) | `1.0 / 4` (`25.0%`) | **`4.0 / 4` (`100%`)** | `3.5 / 4` (`87.5%`) |
| **Total (`24` questions)** | **`12.0 / 24` (`50.0%`)** | **`16.5 / 24` (`68.8%`)** | **`24.0 / 24` (`100.0%`)** | **`18.5 / 24` (`77.1%`)** |
| **Macro Architecture (`Q1`–`Q3`, `12` items)** | **`12.0 / 12` (`100%`)** | **`12.0 / 12` (`100%`)** | **`12.0 / 12` (`100%`)** | `8.25 / 12` (`68.8%`) |
| **Macro + Public API (`Q1`–`Q4`, `16` items)** | `12.0 / 16` (`75.0%`) | **`15.5 / 16` (`96.9%`)** | **`16.0 / 16` (`100%`)** | `11.5 / 16` (`71.9%`) |
| **Intra-Module Detail (`Q4`–`Q6`, `12` items)** | `0.0 / 12` (`0%`) | `4.5 / 12` (`37.5%`) | **`12.0 / 12` (`100%`)** | `10.25 / 12` (`85.4%`) |

#### Item-by-Item Breakdown (`24` Questions)

| ID | Project & Target | `ModuleOnly` | `PublicSurface` | `Full` | `Source-Only (12 calls)` | Key Failure Mode / Difference |
| :--- | :--- | :---: | :---: | :---: | :---: | :--- |
| `Q1.omnicoach` | `src/` components (`[L7]`..`[L0]`) | `1.0` | `1.0` | `1.0` | `1.0` | All 4 conditions reconstructed `[L7] Bin` down to the 5 `[L0]` components. |
| `Q1.ripgrep_ignore` | `ignore/src` modules (`[L1]`..`[L0]`) | `1.0` | `1.0` | `1.0` | `1.0` | All 4 identified `[L0] default_types` and the 7 `[L1]` modules. |
| `Q1.cpython_zoneinfo` | `zoneinfo` modules (`[L2]`..`[L0]`) | `1.0` | `1.0` | `1.0` | `1.0` | All 4 identified `[L2] zoneinfo -> [L1] _zoneinfo -> [L0] _common, _tzpath`. |
| `Q1.cpython_urllib` | `urllib` modules (`[L3]`..`[L0]`) | `1.0` | `1.0` | `1.0` | `1.0` | All 4 identified `[L3] robotparser -> [L2] request -> [L1] error -> [L0] __init__, parse, response`. |
| `Q2.omnicoach` | Reflexion (`Absent`/`Divergent`) + `RuleDeclaration` cycle | `1.0` | `1.0` | `1.0` | `1.0` | All 4 found `CodeLintSuppression -> CodeLintContract` (`absent`), `0` `divergent`, and `rule_declaration <-> options`. |
| `Q2.ripgrep_ignore` | 7-module SCC & `pathutil -> walk` count | `1.0` | `1.0` | `1.0` | `0.5` | `Source-Only` found `pathutil -> walk` (`DirEntry`) but miscounted references (`1`/`2` vs. `x4`). |
| `Q2.cpython_zoneinfo` | 5 cross-module summary edges & counts | `1.0` | `1.0` | `1.0` | `0.5` | `Source-Only` found all 5 edges but miscounted 3 of 5 reference counts (`4` vs `x9`, `2` vs `x5`, `1` vs `x3`). |
| `Q2.cpython_urllib` | Cycles & `0`-outgoing modules | `1.0` | `1.0` | `1.0` | `1.0` | All 4 identified `0` cycles and the 3 leaf modules (`urllib`, `parse`, `response`). |
| `Q3.omnicoach` | Blast radius of `CodeLintSemantic` (`bindings`, `calls`, `comments`) | `1.0` | `1.0` | `1.0` | `0.25` | `Source-Only` lacked call budget to inspect `src/code_lint/rules/*`; hallucinated 6 rule modules (`commented_out_code`, `shadowed_variable`, `callee_before_caller`, etc.) instead of `abbreviated_name`, `single_letter_name`, `environment_variable_in_function`, `sleep_in_tests`. |
| `Q3.ripgrep_ignore` | Dependents of `pathutil` & max dependent of `lib` | `1.0` | `1.0` | `1.0` | `0.0` | `Source-Only` falsely claimed `walk -> pathutil`, missed `gitignore -> pathutil [x7]`, and named `dir` (`35`) instead of `walk` (`x53`) as top dependent of `lib`. |
| `Q3.cpython_zoneinfo` | Dependents of `_tzpath` & `_common` with counts | `1.0` | `1.0` | `1.0` | `0.5` | `Source-Only` found the right module pairs but miscounted references (`4`/`1` vs `x9`/`x3`, `2` vs `x5`). |
| `Q3.cpython_urllib` | Dependents of `error` & `parse` with counts | `1.0` | `1.0` | `1.0` | `0.5` | `Source-Only` found `request` and `robotparser`, but guessed `~45-50` (actual `x22`) and `~85-95` (actual `x60`) because `request.py` is 2,780 lines. |
| `Q4.omnicoach` | `code_lint::suppression` public API & levels | `0.0` | `1.0` | `1.0` | `1.0` | `PublicSurface`, `Full`, and `Source-Only` identified `[L1] SuppressionTracker` -> `[L0] contains_directive_prefix`. |
| `Q4.ripgrep_ignore` | `dir.rs` public API & `[L4]`..`[L0]` levels | `0.0` | `1.0` | `1.0` | `0.5` | `Source-Only` missed `IgnoreRef -> IgnoreMatch [signature_contract]`, placing `IgnoreRef` at `[L0]` (`Lmax=L3` instead of `L4`). |
| `Q4.cpython_zoneinfo` | `_tzpath.py` public API & `InvalidTZPathWarning` encapsulator | `0.0` | `0.5` | `1.0` | `0.75` | In `PublicSurface`, `reset_tzpath -> _parse_python_tzpath -> InvalidTZPathWarning` was hidden because `_parse_python_tzpath` is private (`F18`). |
| `Q4.cpython_urllib` | `robotparser.py` public API & `[L3]`..`[L0]` levels | `0.0` | `1.0` | `1.0` | `1.0` | `PublicSurface`, `Full`, and `Source-Only` matched all 8 public declarations and `[L3]`..`[L0]` levels. |
| `Q5.omnicoach` | `rule_selection.rs` `Shared(...)` buckets | `0.0` | `0.0` | `1.0` | `0.5` | `Source-Only` did not apply ADR 011's *private-only* reachability (`Roots(h)` stops at public items), merging `load_config`/`parse_config` into `parse_plan`'s owner set and misclassifying `rule_names` as shared. |
| `Q5.ripgrep_ignore` | `types.rs` `Shared(...)` buckets (`GlobInner`, `Selection`) | `0.0` | `0.0` | `1.0` | `1.0` | Both `Full` and `Source-Only` identified `Shared(Glob, Types) -> GlobInner` and `Shared(Types, TypesBuilder) -> Selection`. |
| `Q5.cpython_zoneinfo` | `_zoneinfo.py` owner groups, 4 private classes, 6 private fns | `0.0` | `0.0` | `1.0` | `1.0` | Both `Full` and `Source-Only` matched `Owner(ZoneInfo)` and all 10 private declarations. |
| `Q5.cpython_urllib` | `parse.py` 6-owner shared cluster & `Unowned` item | `0.0` | `0.0` | `1.0` | `1.0` | Both `Full` and `Source-Only` found all 6 URL-parsing functions and `_fix_result_transcoding` (`Unowned`). |
| `Q6.omnicoach` | `SuppressionTracker` methods, composed types & `ToolDirective` | `0.0` | `0.0` | `1.0` | `1.0` | Both `Full` and `Source-Only` matched all 4 methods, `ParsedDirective`/`DirectiveSite`/`DirectivePlacement`, and `ToolDirective` (`Unowned`). |
| `Q6.ripgrep_ignore` | `IgnoreBuilder` `[L1] -> [L0]` method call | `0.0` | `0.0` | `1.0` | `1.0` | Both `Full` and `Source-Only` identified `IgnoreBuilder::build [L1] -> IgnoreBuilder::build_with_cwd [L0]`. |
| `Q6.cpython_zoneinfo` | `_zoneinfo.py` 9 ADR 011 placement findings | `0.0` | `0.0` | `1.0` | `0.5` | `Source-Only` missed all 4 class-method findings (`ZoneInfo::_find_trans`, `_new_instance`, `_get_local_timestamp`, `_CalendarOffset::_ymd2ord`) and hallucinated 4 module-level `uncolocated-helper` false positives. |
| `Q6.cpython_urllib` | `build_opener` 10 encapsulated types & `HTTPPasswordMgr*` chain | `0.0` | `1.0` | `1.0` | `1.0` | Because all 10 handler types and the 3 `HTTPPasswordMgr*` classes are public, `PublicSurface`, `Full`, and `Source-Only` all scored `1.0`. |

### Key Insights from the 24-Question Evaluation

1. **Equal-budget `Source-Only` collapses on cross-module blast radius (`Q3`: `31.3%`) and non-local invariants (`Q5.omnicoach`, `Q6.cpython_zoneinfo`)**:
   - Under a 12-call budget across 4 projects, the `Source-Only` agent could not inspect 50+ rule files in `omnicoach` or read all 7,042 lines of `ripgrep/ignore` and 5,112 lines of `urllib`, leading to hallucinated rule names in `Q3.omnicoach` and wrong top-coupling modules in `Q3.ripgrep_ignore`.
   - Even when reading a single file in full (`rule_selection.rs` or `_zoneinfo.py`), manual mental simulation of ADR 011's *private-only* ownership reachability (`Roots(h)`) and class-method placement rules produced false positives that `Outline: Full` answered with **100% (`24 / 24`)** accuracy in 10 calls.
2. **Each `ProjectDepth` cleanly targets a distinct comprehension tier**:
   - **`ModuleOnly` (`0.8%–4%` of source bytes)**: **100% (`12 / 12`)** on macro architecture, reflexion conformance, cross-module cycles, and blast radius (`Q1`–`Q3`).
   - **`PublicSurface` (`3%–12%` of source bytes)**: **96.9% (`15.5 / 16`)** on macro + public API surface questions (`Q1`–`Q4`), missing only 0.5 points where a public function (`reset_tzpath`) encapsulated a public warning (`InvalidTZPathWarning`) transitively through a private helper (`_parse_python_tzpath`).
   - **`Full` (`22%–57%` of source bytes)**: **100% (`24 / 24`)** across all macro, public API, ownership, and intra-module questions (`Q1`–`Q6`).

---

## 4. Stage B Findings & Learnings (`F14`–`F18`)

1. **F14 — Real `Absent` Architecture Edge Discovered in `omnicoach` (`CodeLintSuppression -> CodeLintContract`)**:
   - Comparing `define_architecture!` in [src/architecture.rs](../../../src/architecture.rs) against actual cross-module references in `src/` immediately surfaced a stale dependency declaration: `CodeLintSuppression => [CodeLintContract, CodeLintSemantic, RuleDeclaration, Config]` declares `CodeLintContract`, but [src/code_lint/suppression.rs](../../../src/code_lint/suppression.rs) has **0** references to `crate::code_lint::contract`.
   - Furthermore, out of `67` convergent component pairs with `count > 0`, `39` are direct `define_architecture!` edges and `28` rely on transitive reachability (`define_architecture!` documents that *"transitive dependencies are computed automatically"*).
2. **F15 — Intra-Component Module Cycles Exposed by Module-Level SCCs**:
   - Even when the component-level architecture is a strict DAG (`M2 = 0`), two benchmark scopes contained hidden intra-component module cycles:
     - In `omnicoach`: `rule_declaration` (`src/rule_declaration.rs`) and `rule_declaration::options` (`src/rule_declaration/options.rs`) form a 2-module cycle (`rule_declaration` uses `options::*` types; `options.rs` imports `super::closest_match`).
     - In `ripgrep/crates/ignore`: 7 of its 8 modules (`lib`, `walk`, `dir`, `gitignore`, `overrides`, `types`, `pathutil`) form a single 7-module SCC because `lib.rs` re-exports `walk::*`, matcher modules import `crate::{Error, Match}` from `lib.rs`, and `pathutil.rs` imports `crate::walk::DirEntry`. Only `default_types.rs` is at `L0`.
3. **F16 — Multi-Level Compound Graph Edge Deduplication (`syncEdgeVisibility`)**:
   - When `cytoscape-expand-collapse` collapses nested compound levels (`Module` inside `Component`), it retargets child summary edges (`mod:A -> mod:B`) onto the outer collapsed `Component` nodes (`comp:X -> comp:Y`). Without deduplication, `omnicoach`'s initial component overview rendered `653` parallel edges across a `7028×8765` canvas instead of `68` component summary edges.
   - Recording `orig_source` and `orig_target` on every edge and deduplicating retargeted cross-level edges via `shownPairs` in `syncEdgeVisibility()` reduced the initial `omnicoach` canvas to `2185×1030` (`21` nodes, `68` edges) while preserving mixed-depth edges when a single component (`[L7] Bin`) is expanded (`24` nodes, `69` edges).
4. **F17 — `ProjectDepth::PublicSurface` as the Sweet-Spot Default for Project Exports**:
   - `ModuleOnly` (`Component → Module`) is ideal for pure architecture and import-cycle audits (`0.8%–4%` of source bytes), while `Full` (`Component → Module → Owner → Declarations → TypeMembers`) supports deep single-file or package drill-down (`22%–57%` of source bytes, `24/24` on `M4`).
   - `PublicSurface` (`Component → Module → Public API items`) provides the best balance for project-wide onboarding (`96.9%` on `Q1`–`Q4` at **3%–12%** of raw source bytes).
5. **F18 — Transitive Public-to-Public Summary Edges in `PublicSurface` Mode**:
   - In `Q4.cpython_zoneinfo`, `reset_tzpath` encapsulates `InvalidTZPathWarning` transitively via private helpers (`reset_tzpath -> _reset_tzpath -> _parse_python_tzpath -> InvalidTZPathWarning`). Because `PublicSurface` elides private helpers and previously retained only direct edges between public declarations, `reset_tzpath` appeared at `[L0]` with no edge to `InvalidTZPathWarning` in `PublicSurface`, whereas `Full` mode's `Owner(reset_tzpath) -> Owner(InvalidTZPathWarning)` summary edge placed `reset_tzpath` at `[L1]`.
   - **Fix / Design Rule**: When building `PublicSurface`, project `Owner(p) -> Owner(q)` summary edges onto public declarations `p -> q` (whenever `p` reaches `q` through `Owner(p)`'s private helpers) so indirect public-to-public dependencies are never lost when private helpers are folded away.
