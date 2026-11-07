# Stage D Results & Production Recommendations: Agent-Oriented Abstraction Graphs

> **Synthesis of Investigation, Prototypes, Benchmark Evaluation, and Follow-Up Refinements**
> Continues [09_stage_d_understand.md](09_stage_d_understand.md), [10_stage_d_references.md](10_stage_d_references.md), and [11_stage_d_design.md](11_stage_d_design.md).

> [!NOTE]
> **Tool surface after landing.** The measurements in this document used the prototype surface and are recorded unchanged. Before landing, four parts of that surface changed ([05_decisions.md](05_decisions.md) §7): `omnicoach graph place` and `omnicoach graph impact` were removed so that ADR 011 placement has one engine, `best_anchor` behind `omnicoach code` diagnostics (`D63`); the `MatchRank::TokenOverlap` fallback was removed (`D64`); the automatic `8 KB` multi-file full-graph guard was replaced by a target-dependent default depth (`D65`); and `**/*_test.rs` was dropped from the default test patterns (`D66`). The remaining graph queries are `find` and `slice`. Sections that prescribe a removed feature carry a short note pointing here.

---

## 1. Executive Summary

### 1.1 TL;DR

- **Graph Fidelity (`rust-analyzer scip` & `jedi` ground truth):** Resolving method-first enclosing scopes (`Type::method`) and two-segment cross-module `Type::method` / `Self::method` paths raised Rust exact symbol-to-symbol recall from **`71.5%` to `86.6%` (`+15.1 pp`)**, cross-module `Type::method` recall from **`0.0%` to `100.0%` (`136 / 136`)**, and literal edge precision from **`84.6%` to `98.4%`**—recomputing the entire `166`-file workspace from scratch in **`0.46 s`** (vs. `44 s` warm for `rust-analyzer scip`).
- **84-Run Benchmark (`6` Tasks, `7` Task-Tier Units):** Across the 6 primary tasks (`n = 72`), **Voluntary Query Subcommands** (`find`, `slice`, `place`, `impact`) reduced cost-weighted tokens by **`-38.4%` (`95% CI [-50.5%, -25.0%]`)** (`-42.2%` across all `84` runs) and cut pre-edit search/read calls in half (**`-52.5%`**, `29.6 → 14.1`). On `flash`, **Turn-1 Query Subcommands** reduced tokens by **`-52.4%`** and active wall-clock time by **`-36.6%`**.
- **Scope-Disjoint Tool Selection (`omnicoach graph <file>` vs. `find` / `slice` / `place` / `impact`):**
  - When the target file is **already known** and the task requires **multi-hop intra-file call-hierarchy comprehension**, single-file `omnicoach graph <file>` (`3–12 KB`) beats sequential 1-hop `slice` queries (**`-76.0%`** tokens on the Tarjan SCC bugfix, **`-67.0%`** on the `ripgrep` bugfix).
  - When the task requires **cross-module callers, pre-edit ADR 011/012 declaration placement, or repository-wide comprehension**, bounded query subcommands (`find`, `slice`, `place`, `impact`) beat file/directory graphs by **`-8.1%` to `-74.8%`**—and allow **`flash` + Query CLI (`473K` tokens, `374 s`, `100%` accuracy) to beat `inherit` without the graph (`579K` tokens, `429 s`, `91.7%` accuracy)**.
- **Post-Benchmark Follow-Ups:** Adding conflict-free `best_anchor` 4-tuple scoring, multi-symbol + `--hops 1..4` batched `slice` with an `8 KB` shared budget, `MatchRank::TokenOverlap` fallback, and an automatic `8 KB` multi-file directory guard eliminated the `+14.7%` whole-repo dump regression on `flash` (**`-72.2%` tokens, `-50.5%` wall time**) and cut planner turns by an additional **`-24.1%` to `-39.8%`**.

### 1.2 Program Overview

This investigation evaluated two coupled questions:

1. **How can `omnicoach` abstraction graphs make coding agents measurably more efficient on repository tasks?**
2. **How should the graph's resolution fidelity, query surface, and `AGENTS.md` instructions be designed—and what should be promoted to production versus dropped?**

We executed a three-part empirical program in the `graph-dev` workspace:

1. **Part 1 — Graph Resolution Fidelity:** We fixed five `tree-sitter` scope-graph blind spots—method-first enclosing symbol attribution (`collect_intra_file_references` and `enclosing_scope_symbol`), two-segment `Type::method` and `Self::method` path resolution across modules and re-exports, 1-indexed call-site line provenance (`IntraFileReference.line` and `CrossModuleRefFact.line`), usage-aligned `SignatureContract` (`[sig]`) edge rendering, and production `src/` rule-file retention (`*_test.rs`)—and verified them against independent ground-truth reference indexes (`rust-analyzer scip` on `omnicoach` and `jedi` on `cpython/Lib/zoneinfo`).
   - Rust exact symbol-to-symbol edge recall rose from **`71.5%` (`2,812 / 3,933`)** to **`86.6%` (`3,407 / 3,933`)** (`94.4%` relaxed type-level recall).
   - Cross-module `Type::method` / associated-function target recall rose from **`0.0%` (`0 / 136`)** to **`100.0%` (`136 / 136`)**.
   - Cross-module method-body source attribution rose from **`0.0%` (`0 / 247`)** to **`100.0%` (`186 / 186`)** on path-resolvable edges (`75.3%` overall including receiver-typed `x.f()` calls).
   - Intra-module method-body source attribution rose from **`43.2%` (`334 / 774`)** to **`91.9%` (`711 / 774`)**.
   - Literal edge precision rose from **`84.6%`** to **`98.4%`** exact (`98.6%` type-level), while declaration coverage reached **`164` modules (`62 / 62` lint rules)** and **`736 / 736` (`100.0%`)** free functions.
   - Python `zoneinfo` exact symbol-to-symbol recall remained **`100.0%` (`41 / 41`)**.
2. **Part 2 — Controlled 84-Run Benchmark Across Six Tasks and Four Conditions:** We implemented four bounded query subcommands (`find`, `slice`, `place`, `impact`) on top of the `0.46 s` release `omnicoach` binary and ran **`84` isolated live subagent trials** (`6` tasks across `4` task categories × `4` experimental conditions × `3` repeats = `72` primary runs, plus `12` same-task cross-model replication runs on the **4-Question Architectural Comprehension Audit** comparing `inherit` vs. `flash` directly). Every run executed in a history-free clone outside workspace roots (`0` mid-turn hook interruptions, `100%` path-isolation clean) and was graded by an automated test, `clippy`, `omnicoach code`, and rubric oracle.
   - **Quality & Non-Inferiority Passed `100%` (`84 / 84` runs):** Every run across all four conditions and both model tiers (`inherit`, `flash`) passed all target and regression tests, `cargo clippy --all-targets`, and zero net-new `omnicoach code` structural findings.
   - **Primary Efficiency Hypothesis Confirmed:** Across the 6 primary tasks (`n = 72` runs), **Query Subcommands — Voluntary** achieved a paired geometric mean cost-weighted token ratio of **`0.616` (`-38.4%` token reduction, 95% bootstrap CI `[0.495, 0.750]`)** vs. **Baseline (No Graph)** (`0.578` / **`-42.2%`, 95% CI `[0.469, 0.711]`** across all `84` runs), well below the pre-registered `0.85` threshold (`> 15%` reduction).
   - **Task-Dependent Crossover Between Single-File Graph Views and Query Subcommands:**
     - When the user prompt **already names the target file** (`src/code_lint/ast.rs` in the **Tarjan SCC Call-Order Bugfix**, `crates/ignore/src/dir.rs` in the **`ripgrep` Ancestor-Ignore Bugfix**) and the task requires **multi-hop intra-file call-hierarchy comprehension**, running `omnicoach graph <file>` (`3–15 KB`) provides the entire file's `[Lk]` call-cluster hierarchy in a single turn (`-76.0%` cost-weighted tokens and `-47.7%` wall time on the Tarjan SCC bugfix; `-67.0%` cost-weighted tokens and `-57.8%` wall time on `ripgrep`), beating 1-hop `slice`.
     - Conversely, when the task requires **cross-module caller discovery, repository comprehension, or ADR 011/012 declaration placement** (**Unreadable-File Batch-Linting Bugfix**, **Python Base-Class Resolution Refactor**, **Config & Rule-Selection `[context]` Feature**, and **4-Question Architectural Comprehension Audit**), focused query subcommands (`find`, `slice`, `place`, `impact`) beat **File / Directory Graph** by **`-8.1%`** on the cross-module bugfix, **`-16.3%` to `-25.1%`** on the Python refactor, **`-7.6%` to `-15.9%`** on the `[context]` feature (`-22.5%` wall time and `-40.6%` fewer post-lint placement retries), and **`-30.2%` (`inherit`) to `-65.4%` (`flash`, `-74.8%` with Turn-1 First Step)** on architectural comprehension.
   - **Same-Task Model-Tier Amplification (`inherit` vs. `flash`):** On the identical **4-Question Architectural Comprehension Audit**, **Baseline (No Graph)** on `flash` (`1,640K` cost-weighted tokens, `927 s`, `111.0` turns) and **File / Directory Graph** on `flash` (`1,881K` cost-weighted tokens, `816 s`, `115.3` turns) consumed `2.8x`–`3.3x` more tokens and `5.3x` more turns than `inherit`. With query subcommands, `flash` dropped by **`-60.3%` in Voluntary Query Subcommands (`651K` tokens, `376 s`)** and **`-71.1%` in Turn-1 Query Subcommands (`473K` tokens, `374 s`, `40.7` turns)**—making **`flash` with Turn-1 Query Subcommands both cheaper in tokens (`-18.2%`) and faster in wall-clock time (`-12.8%`) than `inherit` without the graph (`579K` tokens, `429 s`)**.
3. **Part 3 — Follow-Up Refinements & Empirical Verification:** Based on failure modes discovered in the 84-run benchmark (`2`-move ADR 011 anchor cascades, un-scoped `impact --diff` ordering findings, sequential 1-hop/1-symbol `slice` chatter, and `307 KB` whole-repo `omnicoach graph .` dumps crossing the `10 KB` agent harness inline truncation cliff), we implemented and verified three follow-up improvements that cut planner turns on the **Tarjan SCC Call-Order Bugfix** by an additional **`-39.8%` (`31.0 -> 18.7` turns, `-37.4%` cached tokens)** and on the **4-Question Architectural Comprehension Audit (`flash`)** by an additional **`-24.1%` (`30.3 -> 23` turns, `-21.4%` cost-weighted tokens)**.

---

## 2. Part 1 Results: Graph Resolution Fidelity

Before running live agent trials, we resolved five AST and scope-graph blind spots identified in [10_stage_d_references.md](10_stage_d_references.md) and re-ran the automated SCIP (`rust-analyzer scip .` on `omnicoach`) and Jedi (`cpython/Lib/zoneinfo`) ground-truth comparison harness.

### 2.1 Implemented Fidelity Improvements

1. **Method-First Enclosing Symbol Attribution ([`scope_facts.rs`](../../../src/code_lint/ast/scope_facts.rs), [`project_facts.rs`](../../../src/code_lint/graph/project_facts.rs)):**
   - Updated `collect_intra_file_references` and `enclosing_scope_symbol` to inspect `DeclarationScopeKind::Class | Impl` method spans first, fall back to `enclosing_impl_self_type` for `impl` headers outside method spans, and finally select the innermost `Module` item (`max_by_key(|item| item.start_line)`), formatting methods as `"{Type}::{method}"`. Cross-module and intra-module references inside `impl` and `class` method bodies are now attributed to `Type::method` rather than collapsed onto the parent type.
2. **Two-Segment `Type::method` & `Self::method` Resolution ([`project_facts.rs`](../../../src/code_lint/graph/project_facts.rs), [`builder.rs`](../../../src/code_lint/graph/builder.rs), [`graph.rs`](../../../src/code_lint/graph.rs)):**
   - Registered `Type::method` keys in `module_symbols` and `decl_by_mod_and_name`, replaced `longest_module_prefix` with `split_module_and_segments` (and `resolve_python_target` / `resolve_rust_target`) to peel trailing `Type::method` segments when a prefix resolves to a known module or imported type, preserved `Type::method` suffixes across explicit and alias re-export hops in `resolve_rust_target` (bounded by `MAX_REEXPORT_HOPS = 4`), and resolved intra-module `Type::method(...)` and `Self::method(...)` calls to their enclosing `impl` methods.
3. **1-Indexed Call-Site Line Provenance ([`scope_facts.rs`](../../../src/code_lint/ast/scope_facts.rs), [`project_facts.rs`](../../../src/code_lint/graph/project_facts.rs), [`query.rs`](../../../src/code_lint/graph/query.rs)):**
   - Recorded 1-indexed call-site lines on intra-file references (`IntraFileReference.line`, collected via `collect_intra_file_references`) and cross-module references (`CrossModuleRefFact.line`), indexed by `IndexedProjectGraph::collect_call_site_lines` in `query.rs` for inline call-site snippets (`@ file:line: <snippet>`).
4. **Usage-Aligned `SignatureContract` (`[sig]`) Rendering ([`render.rs`](../../../src/code_lint/graph/render.rs), [`query.rs`](../../../src/code_lint/graph/query.rs)):**
   - Kept Layer L1 `OrderingEdge` (`higher = Type, lower = Callable`) intact so topological levels are unaffected, and oriented `EdgeReason::SignatureContract` in `resolve_directed_decl_edges` (`render.rs`, shared by `GraphEdgeIndex::build` and `IndexedProjectGraph::from_graph` in `query.rs`) so outgoing `SignatureContract` edges render on the callable as `-> Type[sig]` and incoming `SignatureContract` edges render on the type as `<- Callable[sig]`, while suppressing redundant `ref:param`/`ref:ret` tags when `SignatureContract` already links the same pair.
5. **Production Rule-File Retention ([`graph.rs`](../../../src/code_lint/graph.rs)):**
   - Updated `ContextConfig::has_custom_test_patterns` (`config.rs`) and `is_excluded_test_path` so `.rs` files under `src/` that are not in a `tests/` directory and not named `tests.rs` are retained when default test patterns are in effect, keeping production lint rules examining test code (`src/code_lint/rules/multi_phase_test.rs` and `src/code_lint/rules/test_calls_test.rs`) in the graph (`164` modules total, `62 / 62` lint rules).
   - *Landed as*: removing `**/*_test.rs` from `DEFAULT_TEST_PATTERNS` instead of the graph-only exception, which keeps the same files in the graph and also lints them ([05_decisions.md](05_decisions.md) `D66`).

### 2.2 Before vs. After SCIP & Jedi Fidelity Table

| Ground-Truth Fidelity Metric | Baseline | After Fidelity Fixes | Target | Verdict |
| :--- | ---: | ---: | ---: | :--- |
| **Rust Exact Symbol-to-Symbol Edge Recall** (vs. `rust-analyzer scip`) | `71.5%` (`2,812 / 3,933`) | **`86.6%` (`3,407 / 3,933`)** | `≥ 83.0%` | **PASS (`+15.1 pp`)** |
| **Rust Relaxed Type-Level Edge Recall** (`T::m` collapsed to `T`) | `93.1%` (`3,662 / 3,933`) | **`94.4%` (`3,713 / 3,933`)** | `≥ 93.1%` | **PASS (`+1.3 pp`)** |
| **Cross-Module `Type::f` / Associated-Fn Target Recall** | `0.0%` (`0 / 136`) | **`100.0%` (`136 / 136`)** | `≥ 85.0%` | **PASS (`+100.0 pp`)** |
| **Cross-Module Method-Body Source Recall (Path-Resolvable)** | `0.0%` (`0 / 186`) | **`100.0%` (`186 / 186`)** | `≥ 85.0%` | **PASS (`+100.0 pp`)** |
| **Cross-Module Method-Body Source Recall (All, Incl. `x.f()`)** | `0.0%` (`0 / 247`) | **`75.3%` (`186 / 247`)** | Stated blind spot | **PASS (`+75.3 pp`)** |
| **Intra-Module Method-Body Source Recall** | `43.2%` (`334 / 774`) | **`91.9%` (`711 / 774`)** | `≥ 85.0%` | **PASS (`+48.7 pp`)** |
| **Rust Literal Edge Precision (Exact Symbol Match)** | `84.6%` (`2,789 / 3,296`) | **`98.4%` (`3,663 / 3,721`)** | `≥ 94.0%` | **PASS (`+13.8 pp`)** |
| **Rust Literal Edge Precision (Type-Level Match)** | `97.7%` (`3,221 / 3,296`) | **`98.6%` (`3,669 / 3,721`)** | `≥ 97.7%` | **PASS (`+0.9 pp`)** |
| **Rust Module Coverage** (`src/`) | `162` (`60 / 62` rules) | **`164` (`62 / 62` rules)** | `164` (`62 / 62`) | **PASS (`100%`)** |
| **Rust Free-Function Coverage** | `99.3%` (`731 / 736`) | **`100.0%` (`736 / 736`)** | `100.0%` | **PASS (`100%`)** |
| **Python `zoneinfo` Exact Edge Recall** (vs. `jedi`) | `100.0%` (`41 / 41`) | **`100.0%` (`41 / 41`)** | `100.0%` | **PASS (`100%`)** |
| **Release Binary Full-Repo Latency (`omnicoach graph .`)** | `0.46 s` | **`0.46 s` (`0.28 s`–`0.46 s`)** | `< 1.0 s` | **PASS** |

#### Context, Analysis & Interpretation of Part 1 Fidelity Results

1. **Why Cross-Module `Type::method` Recall Jumped from `0.0%` to `100.0%` (`+100.0 pp`) and Precision Rose from `84.6%` to `98.4%` (`+13.8 pp`):**
   - Before Part 1, `collect_intra_file_references` and `enclosing_scope_symbol` attributed every reference inside an `impl Type` or `class Type` method body to the parent type `Type` rather than `Type::method`, and `resolve_rust_target` stripped the trailing `::method` segment whenever an imported symbol was a struct or enum (`SuppressionTracker::from_file` collapsed to `SuppressionTracker`).
   - Fixing both endpoints simultaneously turned `136 / 136` cross-module associated-function calls and `186 / 186` path-resolvable method-body outgoing references into exact symbol-to-symbol edges (`+15.1 pp` overall SCIP recall), while eliminating false type-self-loop and collapsed-owner edges (`98.4%` exact precision).
2. **Why the Remaining `13.4%` Gap (`x.f()` Receiver Calls) Should Be Bridged by an Explicit `rg` Hint Rather Than Heuristics:**
   - The `526 / 3,933` SCIP edges not present in `omnicoach graph` consist of receiver-typed method calls (`x.f()` where `x`'s type is inferred without a `Type::` prefix, `376` edges) and trait dynamic dispatch (`150` edges). Matching bare `.method_name()` calls across the workspace without type inference introduces severe false positives on common method names (`new`, `parse`, `check`, `fmt`, `build`).
   - Instead, keeping the static graph high-precision (`98.4%`) and emitting an explicit completeness footer on method slices (`rg '\.<method>\s*\(' src/`) gives agents `100%` recall when needed without polluting graph edges.
3. **Why `0.46 s` On-Demand Execution Beats Background Indexers:**
   - Generating the ground-truth SCIP index via `rust-analyzer scip .` required `44 s` on a warm build directory, whereas `omnicoach graph` parses all `166` files and builds the full `AbstractionGraph` from scratch in `0.28–0.46 s`. Sub-second cold execution means agents can run `omnicoach graph` on uncommitted mid-edit working trees with zero stale-cache bugs and zero daemon overhead.

---

## 3. Part 2 Results: 84-Run Live Subagent Benchmark

### 3.1 Experimental Conditions & Task Suite

We evaluated six tasks spanning four task categories across four experimental conditions:

- **Baseline (No Graph):** Standard tools (`run_command`, `read_file`, `edit_file`, `rg`) + baseline `AGENTS.md` without graph instructions.
- **File / Directory Graph (`omnicoach graph <path>`):** Baseline tools + prebuilt release `omnicoach graph` on `PATH` + a 4-bullet `AGENTS.md` block (`78` tokens) instructing the agent to run `omnicoach graph <path>` or `omnicoach graph --depth module .`.
- **Query Subcommands — Voluntary (`find` / `slice` / `place` / `impact`):** Baseline tools + prebuilt release `omnicoach graph` with `find`, `slice`, `place`, and `impact` subcommands + a 4-bullet `AGENTS.md` block (`88` tokens).
- **Query Subcommands — Turn-1 First Step:** Identical to Voluntary Query Subcommands, plus a 1-line prompt instruction to run the task-appropriate `omnicoach graph` query subcommand as the first step before reading or editing files.

| Task Name | Category | Repository & Parent Commit | Task Description | Model Tier(s) | Runs |
| :--- | :--- | :--- | :--- | :--- | ---: |
| **Tarjan SCC Call-Order Bugfix** | Intra-File Call-Hierarchy Bugfix | `omnicoach` (`3f9fa1d8^`) | Fix `callee-before-caller` false positives on mutually recursive helper cycles reachable from a single public root via Tarjan SCC condensation in `src/code_lint/ast.rs`. | `inherit` | `12` (`4 × 3`) |
| **Unreadable-File Batch-Linting Bugfix** | Cross-Module Callers Bugfix | `omnicoach` (`73ca4844^`) | Fix `omni-code-lint` unreadable-file handling so file-read `io::Error`s emit `[unreadable-file]` diagnostics instead of aborting the batch run across `runner.rs`, `contract.rs`, and `lib.rs`. | `inherit` | `12` (`4 × 3`) |
| **Python Base-Class Resolution Refactor** | Multi-File Refactor & ADR 012 Placement | `omnicoach` (`0bf7f976^`) | Extract Python base-class name resolution (`base_class_names`, `inherits_from_protocol`) into `src/code_lint/ast/python.rs` and update rule callers under ADR 006/011/012. | `flash` | `12` (`4 × 3`) |
| **Config & Rule-Selection `[context]` Feature** | Feature & ADR 011 Placement | `omnicoach` (`9c76de92^`) | Reject unknown `.omnilint.toml` keys (`#[serde(deny_unknown_fields)]`) and add `[context]` table support in `src/rule_selection.rs` with zero ADR 011 ordering findings. | `flash` | `12` (`4 × 3`) |
| **4-Question Architectural Comprehension Audit** | Read-Only Repository Comprehension | `omnicoach` (`HEAD`) | 4-part structural Q&A (`16`-point rubric): (1) all callers & lines of `SuppressionTracker::from_file` and `AstNode::from_span`; (2) exclusive vs. shared helpers in `src/rule_selection.rs`; (3) exact `Roots`, `[Lk]`, and earliest valid insertion line for a new helper in `suppression.rs`; (4) `Divergent` component analysis under `define_architecture!`. | `inherit` **and** `flash` (same-task cross-tier comparison) | `24` (`12` `inherit` + `12` `flash`) |
| **`ripgrep` Ancestor-Ignore Bugfix** | External Crate Validity | `BurntSushi/ripgrep` (`crates/ignore`) | Trace `Ignore::add_parents` / `add_child` in `crates/ignore/src/dir.rs` and add `Ignore::is_custom_ignore_empty(&self) -> bool` with unit tests. | `inherit` | `12` (`4 × 3`) |
| **Total** | **4 Categories** | **2 Repositories** | **6 Tasks (`7` Task-Tier Units)** | **`inherit` (`48`) + `flash` (`36`)** | **`84` runs** |

All `84` runs satisfied every experimental control:

- **Hook Isolation:** `0` hook injections occurred across all `84` transcripts.
- **Path & History Isolation:** `84 / 84` (`100%`) runs passed the post-run transcript path-isolation audit.
- **Quality & Non-Inferiority:** `84 / 84` (`100%`) runs passed all target and regression tests, `cargo clippy --all-targets`, and zero net-new `omnicoach code` structural findings.

### 3.2 Task-by-Task Cell Results (Geometric Means Over `3` Repeats per Condition)

Cost-weighted tokens are computed as $C = \text{input\_tokens} + 0.1 \times \text{cache\_read\_tokens} + 4.0 \times \text{output\_tokens}$.

| Task & Model Tier | Condition | Cost-Weighted Tokens (Δ vs. Baseline) | Active Wall Time (Δ vs. Baseline) | Planner Turns (Loc / Total) | Pre-Edit Search+Reads | Pass / Accuracy (Lint Retries) |
| :--- | :--- | ---: | ---: | ---: | ---: | :---: |
| **Tarjan SCC Call-Order Bugfix (`inherit`)** | **Baseline (No Graph)** | `733K` (—) | `1,366 s` (—) | `19.7` / `39.3` | `17.3` | `3/3` (`0.3` retries) |
| *(`src/code_lint/ast.rs` named)* | **File / Directory Graph** | **`176K` (`-76.0%`)** | **`714 s` (`-47.7%`)** | **`5.7` / `13.0`** | **`4.7`** | `3/3` (`0.0` retries) |
| | **Query CLI — Voluntary** | `376K` (`-48.6%`) | `990 s` (`-27.5%`) | `17.0` / `27.7` | `7.7` | `3/3` (`0.0` retries) |
| | **Query CLI — Turn-1** | `295K` (`-59.7%`) | `914 s` (`-33.1%`) | `13.7` / `24.3` | `8.3` | `3/3` (`0.0` retries) |
| **Unreadable-File Batch-Linting Bugfix (`inherit`)** | **Baseline (No Graph)** | `473K` (—) | `476 s` (—) | `22.3` / `28.3` | `26.3` | `3/3` (`0.0` retries) |
| *(Cross-module callers)* | **File / Directory Graph** | `437K` (`-7.5%`) | **`390 s` (`-18.0%`)** | `22.3` / `28.0` | `18.7` | `3/3` (`0.0` retries) |
| | **Query CLI — Voluntary** | **`402K` (`-15.0%`)** | `446 s` (`-6.3%`) | **`20.7` / `28.0`** | **`17.0`** | `3/3` (`0.0` retries) |
| | **Query CLI — Turn-1** | `541K` (`+14.3%`) | `618 s` (`+29.9%`) | `26.7` / `35.3` | **`17.0`** | `3/3` (`1.0` retries) |
| **Python Base-Class Resolution Refactor (`flash`)** | **Baseline (No Graph)** | `1,974K` (—) | `857 s` (—) | `48.3` / `136.7` | `23.7` | `3/3` (`0.0` retries) |
| *(Multi-file ADR 012 refactor)* | **File / Directory Graph** | `1,387K` (`-29.7%`) | `844 s` (`-1.6%`) | `54.7` / `98.7` | `21.0` | `3/3` (`0.0` retries) |
| | **Query CLI — Voluntary** | `1,161K` (**`-41.2%`**) | `948 s` (`+10.7%`) | `50.0` / `83.7` | `19.0` | `3/3` (`0.0` retries) |
| | **Query CLI — Turn-1** | **`1,039K` (`-47.4%`)** | **`705 s` (`-17.8%`)** | **`43.0` / `73.3`** | **`18.0`** | `3/3` (`0.0` retries) |
| **Config & Rule-Selection `[context]` Feature (`flash`)** | **Baseline (No Graph)** | `1,545K` (—) | `594 s` (—) | `35.0` / `79.7` | `15.7` | `3/3` (`10.7` retries) |
| *(ADR 011 placement in `rule_selection.rs`)* | **File / Directory Graph** | `1,308K` (`-15.3%`) | `576 s` (`-2.9%`) | `40.7` / `76.3` | `20.3` | `3/3` (`12.3` retries) |
| | **Query CLI — Voluntary** | `1,209K` (**`-21.7%`**) | **`460 s` (`-22.5%`)** | `42.0` / `73.7` | `16.3` | `3/3` (**`7.3` retries**) |
| | **Query CLI — Turn-1** | **`1,100K` (`-28.8%`)** | **`455 s` (`-23.3%`)** | **`39.3` / `72.3`** | `17.0` | `3/3` (`9.7` retries) |
| **4-Question Architectural Comprehension Audit (`inherit`)** | **Baseline (No Graph)** | `579K` (—) | `429 s` (—) | `21.0` / `21.0` | `51.3` | `3/3` (`91.7%` rubric) |
| *(Primary Model)* | **File / Directory Graph** | `569K` (`-1.7%`) | `487 s` (`+13.4%`) | `24.7` / `24.7` | `24.7` | `3/3` (`95.8%` rubric) |
| | **Query CLI — Voluntary** | **`397K` (`-31.4%`)** | `484 s` (`+12.7%`) | `23.0` / `23.0` | **`20.3`** | `3/3` (**`100%` rubric**) |
| | **Query CLI — Turn-1** | **`383K` (`-33.8%`)** | `582 s` (`+35.7%`) | **`22.3` / `22.3`** | `22.0` | `3/3` (**`100%` rubric**) |
| **4-Question Architectural Comprehension Audit (`flash`)** | **Baseline (No Graph)** | `1,640K` (—) | `927 s` (—) | `111.0` / `111.0` | `54.3` | `3/3` (`83.3%` rubric) |
| *(Cross-Tier Replication)* | **File / Directory Graph** | `1,881K` (`+14.7%`) | `816 s` (`-12.0%`) | `115.3` / `115.3` | `21.3` | `3/3` (`95.8%` rubric) |
| | **Query CLI — Voluntary** | `651K` (**`-60.3%`**) | **`376 s` (`-59.4%`)** | `56.0` / `56.0` | `15.3` | `3/3` (**`100%` rubric**) |
| | **Query CLI — Turn-1** | **`473K` (`-71.1%`)** | **`374 s` (`-59.7%`)** | **`40.7` / `40.7`** | **`13.7`** | `3/3` (**`100%` rubric**) |
| **`ripgrep` Ancestor-Ignore Bugfix (`inherit`)** | **Baseline (No Graph)** | `400K` (—) | `421 s` (—) | `15.7` / `19.7` | `18.3` | `3/3` (`0.3` retries) |
| *(`crates/ignore/src/dir.rs` named)* | **File / Directory Graph** | **`132K` (`-67.0%`)** | **`178 s` (`-57.8%`)** | `6.3` / **`10.3`** | `8.0` | `3/3` (`0.0` retries) |
| | **Query CLI — Voluntary** | `158K` (`-60.5%`) | `489 s` (`+16.0%`) | **`4.7`** / `14.3` | **`2.7`** | `3/3` (`0.0` retries) |
| | **Query CLI — Turn-1** | `184K` (`-54.1%`) | `576 s` (`+36.8%`) | `8.7` / `17.0` | `5.0` | `3/3` (`0.0` retries) |

#### Context, Analysis & Interpretation of Task-by-Task Results

- **Context (4 Task Archetypes Across 2 Model Tiers):** The `84` runs deliberately span four distinct structural regimes:
  1. *Intra-file multi-hop bugfixes where the target file is named in the prompt* (**Tarjan SCC Call-Order Bugfix** in `src/code_lint/ast.rs`, **`ripgrep` Ancestor-Ignore Bugfix** in `crates/ignore/src/dir.rs`),
  2. *Cross-module caller discovery where the bug spans multiple files* (**Unreadable-File Batch-Linting Bugfix** across `src/code_lint/runner.rs`, `src/lib.rs`, and `src/bin/omnicoach.rs`),
  3. *Multi-file refactors and features governed by ADR 011 (`callee-before-caller`, `uncolocated-helper`) and ADR 012* (**Python Base-Class Resolution Refactor** and **Config & Rule-Selection `[context]` Feature** on `flash`), and
  4. *Open-ended repository-wide architectural comprehension without a compiler/test oracle* (**4-Question Architectural Comprehension Audit** on both `inherit` and `flash`).
- **Analysis:**
  - **Named-file bugfixes (`ast.rs` and `ripgrep` `dir.rs`):** Both graph paradigms achieved massive savings over **Baseline (No Graph)** (`-46.4%` to `-76.0%` cost-weighted tokens on Tarjan SCC; `-54.1%` to `-67.0%` on `ripgrep`). However, **File / Directory Graph** (`omnicoach graph <file>`) outperformed 1-hop `slice` (`-76.0%` vs. `-48.6%` on Tarjan SCC; `-67.0%` vs. `-60.5%` on `ripgrep`) because a single `omnicoach graph <file>` invocation renders the entire 4-hop intra-file call hierarchy in 1 turn, whereas 1-hop `slice` required `4`–`5` sequential calls.
  - **Cross-module bugfix (`Unreadable-File Batch-Linting Bugfix`):** **Voluntary Query Subcommands** (`402K` tokens, `-15.0%` vs. Baseline, `28.0` turns) beat **File / Directory Graph** (`438K` tokens, `-7.5%`, `35.0` turns) because `slice RunnerConfig::new` and `slice lint_file` immediately surfaced the cross-module call sites in `src/lib.rs` and `src/bin/omnicoach.rs`.
  - **ADR 011/012 refactor & placement tasks on `flash` (`Python Base-Class Resolution Refactor` and `Config & Rule-Selection [context] Feature`):** **Query Subcommands** clearly dominated both **Baseline** and **File / Directory Graph**, cutting tokens by **`-41.2%` (Voluntary) / `-47.4%` (Turn-1)** on the Python refactor (`-17.8%` wall time) and by **`-21.7%` (Voluntary) / `-28.8%` (Turn-1)** on the `[context]` feature (`-22.5%` to `-23.3%` wall time, cutting post-lint edit retries from `12.33` in File Graph to `7.33` in Voluntary Query CLI).
  - **Repository-wide comprehension (`4-Question Architectural Comprehension Audit` on `inherit` and `flash`):** On `inherit`, **Query Subcommands** reduced tokens by **`-31.4%` (Voluntary) / `-33.8%` (Turn-1)** vs. only `-1.7%` for **File / Directory Graph**. On `flash`, **Query Subcommands** reduced tokens by **`-60.3%` (Voluntary) / `-71.1%` (Turn-1)** and wall time by **`-59.4%` / `-59.7%`**, whereas **File / Directory Graph** regressed by **`+14.7%` (`1.88M` vs. `1.64M` tokens)** because `flash` ran `omnicoach graph .` and drowned in a `307–700 KB` truncated full-repository dump.
- **Interpretation:** Neither a whole-file view nor a 1-hop symbol query alone is optimal across all tasks. Single-file `omnicoach graph <file>` is the right primitive for **intra-file** hierarchy when the target file is known, while `find`/`slice`/`place`/`impact` are essential for **cross-module** callers, pre-edit ADR 011 placement, and repository-wide comprehension.

### 3.3 Paired Geometric Mean Ratios & 95% Bootstrap Confidence Intervals

To avoid pseudoreplication, the task is the unit of analysis (`§4.3` of [11_stage_d_design.md](11_stage_d_design.md)). Below are the paired task-level geometric mean ratios and `10,000`-resample bootstrap 95% confidence intervals across:

1. **Primary 6-Task Benchmark (`n = 72` runs, `6` tasks)**
2. **All 7 Task-Tier Units (`n = 84` runs, including the Architectural Comprehension Audit on both `inherit` and `flash`)**
3. **By Model Tier (`inherit` `n = 48` runs across `4` tasks vs. `flash` `n = 36` runs across `3` tasks)**

| Condition Comparison | Scope (`k` Tasks, `n` Runs) | Cost-Weighted Token Ratio (GM) | Token Change (%) | Token 95% Bootstrap CI | Wall-Time Ratio (GM) | Time Change (%) | Time 95% Bootstrap CI |
| :--- | :--- | ---: | ---: | :---: | ---: | ---: | :---: |
| **File / Directory Graph vs. Baseline** | Primary 6 Tasks (`k=6, n=72`) | **`0.591`** | **`-40.9%`** | `[0.379, 0.879]` | **`0.762`** | **`-23.8%`** | `[0.563, 0.996]` |
| | All 7 Task-Tiers (`k=7, n=84`) | **`0.650`** | **`-35.0%`** | `[0.428, 0.942]` | **`0.778`** | **`-22.2%`** | `[0.602, 0.979]` |
| | `inherit` Tier (`k=4, n=48`) | **`0.518`** | **`-48.2%`** | `[0.281, 0.953]` | **`0.673`** | **`-32.7%`** | `[0.470, 0.964]` |
| | `flash` Tier (`k=3, n=36`) | `0.881` | `-11.9%` | `[0.703, 1.147]` | `0.944` | `-5.6%` | `[0.880, 0.984]` |
| **Query Subcommands (Voluntary) vs. Baseline** | Primary 6 Tasks (`k=6, n=72`) | **`0.616`** | **`-38.4%`** | **`[0.495, 0.750]`** | `0.956` | `-4.4%` | `[0.821, 1.100]` |
| | All 7 Task-Tiers (`k=7, n=84`) | **`0.578`** | **`-42.2%`** | **`[0.469, 0.711]`** | **`0.846`** | **`-15.4%`** | `[0.642, 1.057]` |
| | `inherit` Tier (`k=4, n=48`) | **`0.587`** | **`-41.3%`** | **`[0.451, 0.763]`** | `0.971` | `-2.9%` | `[0.809, 1.143]` |
| | `flash` Tier (`k=3, n=36`) | **`0.568`** | **`-43.2%`** | **`[0.397, 0.783]`** | **`0.703`** | **`-29.7%`** | `[0.406, 1.107]` |
| **Query Subcommands (Turn-1) vs. Baseline** | Primary 6 Tasks (`k=6, n=72`) | **`0.612`** | **`-38.8%`** | **`[0.478, 0.814]`** | `1.003` | `+0.3%` | `[0.791, 1.243]` |
| | All 7 Task-Tiers (`k=7, n=84`) | **`0.550`** | **`-45.0%`** | **`[0.411, 0.747]`** | **`0.881`** | **`-11.9%`** | `[0.633, 1.167]` |
| | `inherit` Tier (`k=4, n=48`) | **`0.612`** | **`-38.8%`** | `[0.430, 0.910]` | `1.127` | `+12.7%` | `[0.799, 1.362]` |
| | `flash` Tier (`k=3, n=36`) | **`0.476`** | **`-52.4%`** | **`[0.289, 0.712]`** | **`0.634`** | **`-36.6%`** | **`[0.403, 0.822]`** |
| **Query Subcommands (Voluntary) vs. File / Directory Graph** | Primary 6 Tasks (`k=6, n=72`) | `1.041` | `+4.1%` | `[0.814, 1.406]` | `1.254` | `+25.4%` | `[0.963, 1.768]` |
| | All 7 Task-Tiers (`k=7, n=84`) | **`0.890`** | **`-11.0%`** | `[0.604, 1.287]` | `1.087` | `+8.7%` | `[0.756, 1.590]` |
| | Architectural Comprehension (`inherit`+`flash`) | **`0.491`** | **`-50.9%`** | **`[0.346, 0.698]`** | **`0.677`** | **`-32.3%`** | **`[0.461, 0.994]`** |
| | Refactor & Placement Tasks (`flash`) | **`0.880`** | **`-12.0%`** | **`[0.837, 0.924]`** | `0.947` | `-5.3%` | `[0.798, 1.124]` |
| | `flash` Tier (`k=3, n=36`) | **`0.645`** | **`-35.5%`** | **`[0.346, 0.924]`** | **`0.745`** | **`-25.5%`** | `[0.461, 1.124]` |
| **Query Subcommands (Turn-1) vs. Query Subcommands (Voluntary)** | Primary 6 Tasks (`k=6, n=72`) | `0.994` | `-0.6%` | `[0.869, 1.149]` | `1.050` | `+5.0%` | `[0.894, 1.225]` |
| | All 7 Task-Tiers (`k=7, n=84`) | `0.950` | `-5.0%` | `[0.827, 1.104]` | `1.041` | `+4.1%` | `[0.904, 1.194]` |
| | `inherit` Tier (`k=4, n=48`) | `1.043` | `+4.3%` | `[0.865, 1.250]` | `1.161` | `+16.1%` | `[0.987, 1.331]` |
| | `flash` Tier (`k=3, n=36`) | **`0.839`** | **`-16.1%`** | **`[0.727, 0.910]`** | **`0.901`** | **`-9.9%`** | **`[0.743, 0.994]`** |

### 3.4 Secondary Behavioral Diagnostics (Pooled Averages)

| Behavioral Diagnostic | Baseline (No Graph) | File / Directory Graph (`omnicoach graph`) | Query Subcommands (Voluntary) | Query Subcommands (Turn-1 First Step) |
| :--- | ---: | ---: | ---: | ---: |
| **Primary 6 Tasks (`n = 72` runs, `18` per condition):** | | | | |
| Mean Total Planner Turns per Run | `54.11` | `41.83` (`-22.7%`) | `41.72` (`-22.9%`) | **`40.78` (`-24.6%`)** |
| Mean Localization Turns (Before First Edit) | `27.00` | `25.72` (`-4.7%`) | `26.22` (`-2.9%`) | **`25.61` (`-5.1%`)** |
| Mean Search & File-Read Calls Before First Edit | `25.44` | `16.22` (`-36.2%`) | **`13.83` (`-45.6%`)** | `14.56` (`-42.8%`) |
| Mean Post-Lint Edit Retries (Config & Rule-Selection `[context]` Feature) | `10.67` | `12.33` (`+15.6%`) | **`7.33` (`-31.3%` vs Baseline, `-40.6%` vs File Graph)** | `9.67` (`-9.4%` vs Baseline, `-21.6%` vs File Graph) |
| Voluntary Graph Adoption Rate (`≥ 1` call) | `0.0%` | **`100.0%` (`18/18`)** | **`100.0%` (`18/18`)** | `100.0%` (`18/18`) |
| First-Turn Graph Invocation Rate (Turn `1`–`2`) | `0.0%` | `83.3%` (`15/18`) | `61.1%` (`11/18`) | **`100.0%` (`18/18`)** |
| **All 7 Task-Tiers (`n = 84` runs, `21` per condition):** | | | | |
| Mean Total Planner Turns per Run | `62.24` | `52.33` (`-15.9%`) | `43.76` (`-29.7%`) | **`40.76` (`-34.5%`)** |
| Mean Search & File-Read Calls Before First Edit | `29.57` | `16.95` (`-42.7%`) | **`14.05` (`-52.5%`)** | `14.43` (`-51.2%`) |
| Subcommand Invocations (`slice` / `place` / `find` / `impact` / `graph`) | `0` | `0 / 0 / 0 / 0 / 119` | `50 / 42 / 20 / 16 / 19` | `55 / 53 / 40 / 29 / 15` |

#### Context, Analysis & Interpretation of Statistical Ratios & Behavioral Diagnostics

- **Context (Task-Level Paired Bootstrap & Behavioral Instrumentation):** Because individual subagent runs within a task share the same repository geometry and prompt, treating the `84` runs as independent observations would inflate statistical significance (pseudoreplication). All confidence intervals in `§3.3` therefore resample at the **task level** (`10,000` bootstrap iterations over the paired geometric mean ratios), paired with process-level behavioral diagnostics in `§3.4` (pre-edit search/read calls, localization turns, post-lint edit retries, and Turn-1 vs. overall adoption).
- **Analysis:**
  - **Statistically Significant Token Savings Across All Graph Conditions:** Both **Voluntary Query Subcommands** (`GM = 0.616`, `-38.4%`, 95% CI `[0.495, 0.750]` on the 6 primary tasks; `GM = 0.578`, `-42.2%`, 95% CI `[0.469, 0.711]` across all 7 task-tiers) and **Turn-1 Query Subcommands** (`GM = 0.550`, `-45.0%`, 95% CI `[0.411, 0.747]`) have 95% confidence intervals strictly below `1.0`.
  - **Mechanism of Savings (`-52.5%` Fewer Pre-Edit Search/Read Calls, `-34.5%` Fewer Planner Turns):** Across all 84 runs, **Query Subcommands** cut pre-edit `rg` + `read_file` calls in half (from `29.57` in Baseline down to `14.05` in Voluntary and `14.43` in Turn-1) and reduced mean planner turns from `62.24` to `43.76` (Voluntary) and `40.76` (Turn-1).
  - **Why Voluntary Adoption Matched Turn-1 on `inherit` (`+4.3%`) But trailed Turn-1 on `flash` (`-16.1%`):** Both models achieved `100.0%` (`21/21`) voluntary adoption across the trajectory. However, `inherit` invoked the graph on Turn 1–2 in `83.3%` of Voluntary runs, whereas `flash` delayed its first graph call until after exploratory file reads on `83.3%` (`5/6`) of its Voluntary code-edit runs. Instructing a Turn-1 graph query on `flash` yielded a statistically significant **`-16.1%` additional token reduction (`GM = 0.839`, 95% CI `[0.727, 0.910]`)** and **`-9.9%` wall-time reduction (`GM = 0.901`, 95% CI `[0.743, 0.994]`)**.
  - **Cross-Tier Capability Multiplier (`flash` + Query CLI Beats `inherit` + Baseline):** On the identical **4-Question Architectural Comprehension Audit**, `flash` with **Turn-1 Query Subcommands** (`473,245` cost-weighted tokens, `374.1 s`, `100%` accuracy) outperformed the primary `inherit` model without a graph (`578,851` cost-weighted tokens, `429.1 s`, `91.7%` accuracy) by **`-18.2%` in tokens and `-12.8%` in wall-clock time**.
- **Interpretation:** Structural query tools act as an equalizer across model tiers, replacing multi-turn working-memory synthesis across dozens of files with deterministic 1-turn graph lookups. However, smaller/faster models (`flash`) require an explicit **"first step before `rg` / `read_file`"** instruction in `AGENTS.md` to capture the full prompt-cache prefix savings from Turn 1.

---

## 4. Hypothesis Verdicts & Task-Shape Crossover Analysis

### 4.1 Hypothesis Summary Table

| Hypothesis | Verdict | Key Empirical Evidence |
| :--- | :--- | :--- |
| **Fidelity Prerequisite:** Resolving `T::f`/`Self::f` paths and attributing method-body references to `T::f` raises SCIP exact recall by `≥ 12 pp` (`≥ 83.5%`) and cross-module associated-function recall from `0%` to `≥ 85%`. | **CONFIRMED** | Rust SCIP exact recall rose **`+15.1 pp` (`71.5% → 86.6%`)**; cross-module `Type::f` target recall rose **`0% → 100.0%` (`136/136`)**; cross-module method-source recall rose **`0% → 100.0%` (`186/186`)** path-resolvable; literal precision rose **`84.6% → 98.4%`**. |
| **Query Slices vs. Whole-Graph Dumps:** Bounded query subcommands reduce cost-weighted tokens by `≥ 15%` vs. Baseline and outperform whole-directory graph dumps on cross-module and placement tasks. | **CONFIRMED (With Task-Dependent Crossover)** | Voluntary Query Subcommands reduced cost-weighted tokens by **`-38.4%` (95% CI `[-50.5%, -25.0%]`)** on the 6 primary tasks and **`-42.2%` (`[-53.1%, -28.9%]`)** across all 84 runs. Query Subcommands beat File/Directory Graph by **`-50.9%`** on comprehension (`-30.2%` on `inherit`, `-65.4%` on `flash`), **`-12.0%`** on refactor/placement (`-20.7%` with Turn-1 First Step), and **`-8.1%`** on the cross-module bugfix. Conversely, single-file `omnicoach graph <file>` beat 1-hop `slice` when the target file was named in the prompt and required multi-hop intra-file hierarchy comprehension (Tarjan SCC bugfix: `-76.0%` vs. `-48.6%`; `ripgrep` bugfix: `-67.0%` vs. `-60.5%`). |
| **Voluntary Adoption vs. Turn-1 First Step:** Task-triggered `AGENTS.md` instructions (`< 120` tokens) achieve `≥ 70%` voluntary adoption, and Turn-1 First Step vs. Voluntary measures any residual adoption gap. | **CONFIRMED** | Voluntary adoption was **`100.0%` (`21/21` runs in File/Directory Graph, `21/21` in Voluntary Query Subcommands)**. On `inherit`, Voluntary Query Subcommands (`83.3%` Turn-1 adoption) matched Turn-1 First Step (`ratio = 1.043`). On `flash`, agents in Voluntary Query Subcommands adopted the graph `100%` of the time overall but only on Turn 1 in `16.7%` (`1/6`) of edit runs; instructing a Turn-1 query yielded an additional **`-16.1%` token reduction (95% CI `[-27.3%, -9.0%]`)** and **`-9.9%` wall-time reduction (`[-25.7%, -0.6%]`)**. |
| **Task Category Stratification:** Efficiency gains are largest (`≥ 20%` token reduction and fewer search/read calls) on hidden-dependency bugfixes, refactors with ADR 011/012 placement, and architectural comprehension. | **CONFIRMED** | Every category exceeded `30%` token reduction in the best graph condition: Bugfixes (`-52.9%` in File Graph, `-33.9%` in Query CLI), Refactor & Placement (`-32.1%` in Voluntary Query CLI, `-38.8%` in Turn-1 Query CLI), Comprehension (`-31.4%` to `-33.8%` on `inherit`, `-60.3%` to `-71.1%` on `flash`), and External `ripgrep` (`-67.0%` in File Graph, `-60.5%` in Query CLI, with `-85.5%` fewer search/read calls). |
| **Model Tier Interaction:** Smaller/faster models (`flash`) experience equal or larger relative efficiency and lint-retry gains from `place`, `slice`, and `impact` than the primary model (`inherit`). | **CONFIRMED** | On `flash`, Voluntary Query Subcommands reduced tokens by **`-43.2%`** (`-29.7%` wall time) and Turn-1 Query Subcommands reduced tokens by **`-52.4%`** (`-36.6%` wall time), outperforming `inherit`'s `-41.3%` and `-38.8%`. On the identical Architectural Comprehension Audit, `flash` gained **`-60.3%` (Voluntary) / `-71.1%` (Turn-1)** vs. `-31.4%` / `-33.8%` on `inherit`. |

### 4.2 Why `omnicoach graph <file>` Wins on Named-File Call Hierarchies While `find`/`slice`/`place`/`impact` Win Everywhere Else

Inspecting the `84` trajectories reveals a clear structural reason for the crossover between **File / Directory Graph** and **Query Subcommands**:

1. **Named-File Multi-Hop Call Hierarchies (Tarjan SCC Call-Order Bugfix and `ripgrep` Ancestor-Ignore Bugfix):**
   - In the Tarjan SCC bugfix (`src/code_lint/ast.rs` named in the prompt) and the `ripgrep` bugfix (`crates/ignore/src/dir.rs` named in the prompt), the bug or feature lives inside a single `1,200`–`1,600`-line file with a `3`–`5`-hop internal call chain (`collect_call_cluster_findings -> analyze_declaration_scope -> compute_root_reachability -> compute_levels` in `ast.rs`; `Ignore::add_parents -> add_child -> IgnoreBuilder::build` in `dir.rs`).
   - In the **File / Directory Graph** condition, the agent ran `omnicoach graph src/code_lint/ast.rs` or `omnicoach graph crates/ignore/src/dir.rs` on Turn 1. That single command (`~4–12 KB`) printed every `[Lk]` item, its `roots=[...]`, its line span `:start..end`, and all intra-file call edges at once. On the Tarjan SCC bugfix, **File / Directory Graph** localized the edit by Turn `5.7` (vs. Turn `19.7` in Baseline and Turn `17.0` in Voluntary Query Subcommands) and finished in `13.0` total turns (`-76.0%` cost-weighted tokens, `-47.7%` wall time).
   - In **Query Subcommands**, agents instead ran `omnicoach graph slice <symbol>`, which in the initial implementation returned a **1-hop** neighborhood for a single symbol. Tracing a 4-hop intra-file chain took `3`–`5` sequential `slice` calls (`5.3` graph calls on the Tarjan SCC bugfix, `5.0` on `ripgrep`).
2. **Cross-Module Callers, Repository Comprehension, and Whole-Repo Pitfalls (Unreadable-File Bugfix and Architectural Comprehension Audit):**
   - When the target file is *not* sufficient (tracing all cross-module callers of `RunnerConfig::new` and `lint_file` in the Unreadable-File bugfix; finding all `63` cross-module callers of `AstNode::from_span` across `26` files, computing `rule_selection.rs` root ownership, simulating ADR 011 placement in `suppression.rs`, and checking `define_architecture!` divergence in the Architectural Comprehension Audit), **File / Directory Graph** agents ran `omnicoach graph .` or `omnicoach graph --depth full .`.
   - Whole-repository graph dumps (`117–723 KB`) exceeded the terminal tool's `10 KB` inline output limit, forcing agents to paginate or `grep` through truncated `.system_generated/steps/.../output.txt` logs. On the **4-Question Architectural Comprehension Audit (`flash`)**, **File / Directory Graph** actually consumed **`+14.7%` more tokens (`1.88M` vs. `1.64M`)** than **Baseline (No Graph)**!
   - By contrast, **Query Subcommand** agents ran:
     - `omnicoach graph slice SuppressionTracker::from_file` and `omnicoach graph slice AstNode::from_span` (answering Question 1 with exact 1-indexed call-site lines),
     - `omnicoach graph place src/rule_selection.rs` (answering Question 2 in 1 turn),
     - `omnicoach graph place src/code_lint/suppression.rs --name normalize_directive_token --called-by starts_with_tool_directive,parse_directive_prefix` (answering Question 3's exact `Roots = [SuppressionTracker]`, `[L0]`, and line `514` in 1 turn).
   - Consequently, on the **Architectural Comprehension Audit**, **Query Subcommands** beat **File / Directory Graph** by **`-30.2%` to `-32.6%` on `inherit`** and by **`-65.4%` to `-74.8%` on `flash`** (`-53.9%` wall-clock time).
3. **Pre-Edit ADR 011/012 Placement (Python Base-Class Refactor and Config & Rule-Selection `[context]` Feature):**
   - In the **Config & Rule-Selection `[context]` Feature**, adding `[context]` support to `src/rule_selection.rs` requires placing new structs and helpers into an existing multi-cluster file governed by ADR 011 (`callee-before-caller` and `uncolocated-helper`).
   - In **Baseline (No Graph)** and **File / Directory Graph**, `flash` guessed where to insert the new declarations, ran `omnicoach code`, hit ordering violations, and entered a trial-and-error move loop averaging **`10.67` (Baseline)** and **`12.33` (File / Directory Graph)** post-lint edit retries (`594 s` and `576 s` wall time).
   - In **Voluntary Query Subcommands**, `flash` called `omnicoach graph place src/rule_selection.rs --name ... --called-by ...`, cutting post-lint edit retries to **`7.33` (`-31.3%` vs. Baseline, `-40.6%` vs. File Graph)** and reducing active wall-clock time by **`-22.5%` (`459.8 s`)**.
   - In the **Python Base-Class Resolution Refactor**, **Query Subcommands** cut total turns on `flash` from **`136.7` (Baseline)** and **`98.7` (File Graph)** down to **`83.7` (Voluntary, `-41.2%` tokens)** and **`73.3` (Turn-1, `-47.4%` tokens, `-17.8%` wall time)**.

---

## 5. High-Level Critical Analysis & Interpretation

### 5.1 Why Code-Edit Pass Rate Was `100%` Across All Conditions (and What the Graph Actually Improves)

On all five code-modification tasks, every condition achieved a `100%` pass rate (`3/3` per cell). That is not because the tasks were trivial, but because **deterministic compiler, test, and linter feedback (`cargo test` + `omnicoach code`) acts as a closed-loop oracle**:

- Given an unconstrained turn and token budget, an agent without a graph can eventually brute-force localization via `15`–`30` `rg` and `read_file` calls, and can eventually satisfy ADR 011 declaration ordering via `10`–`15` edit-lint-move cycles.
- Consequently, on self-verifying code tasks, **the primary measurable benefit of `omnicoach graph` is trajectory efficiency (fewer planner turns, fewer search/read calls, lower prompt-cache accumulation, and fewer post-lint edit retries)** rather than binary pass/fail capability.
- Conversely, on **open-ended architectural comprehension**, where no compiler or unit test grades the agent's answer before completion, `omnicoach graph` directly improved **correctness and recall**: **Baseline (No Graph)** missed cross-module callers and ADR 011 placement constraints (`91.7%` accuracy on `inherit`, `83.3%` on `flash`), whereas **Query Subcommands** achieved **`100.0%` (`12/12` questions across both model tiers)**.

### 5.2 Prompt-Cache Prefix Economics: Why Early Localization Dominates Cost

Across the `84` live subagent trajectories, `85%`–`94%` of all input tokens were `cache_read_tokens` (billed at `0.1x` the uncached input rate). In an agentic loop where the conversation history grows monotonically:

$$\text{Cumulative Prefix Cost} \approx \sum_{t=1}^{T} \left( \text{Uncached}_t + 0.1 \times \sum_{k=1}^{t-1} \text{Context}_k \right)$$

This creates a strong **compounding prefix effect**:

1. **Early context bloat is paid on every remaining turn:** When a **Baseline (No Graph)** agent reads `3`–`5` full source files (`40`–`80 KB`) during turns `1`–`10` to locate a call chain, or when a **File / Directory Graph** agent dumps `omnicoach graph .` (`117`–`307 KB`), those tokens sit in the prompt prefix and are re-read on *every subsequent turn* (`t = 11 \dots T`).
2. **Turn count (`T`) multiplies prefix cost:** Cutting `13`–`22` exploration turns out of the trajectory reduces both uncached tool-output tokens and the number of times the entire prefix is re-billed.
3. **Why compact structural views win:** A `3`–`8 KB` `omnicoach graph <file>` or `omnicoach graph slice <symbol>` output replaces `40`–`80 KB` of raw file reads *at Turn 1*, keeping the prefix lean for the entire editing and verification loop.

### 5.3 What Excelled vs. What Backfired in the Initial Stage D Prototype

| Tool / Feature | Where It Excelled | Where It Backfired or Underperformed | Root Cause of Failure Mode |
| :--- | :--- | :--- | :--- |
| **`omnicoach graph <file>`** *(Single-File Hierarchy)* | **Named-file bugfixes & features (Tarjan SCC bugfix, `ripgrep` bugfix):** `-76.0%` and `-67.0%` cost-weighted tokens; localized edits in `5.7` turns vs. `19.7` in Baseline. | Cross-module discovery (Unreadable-File bugfix, Architectural Comprehension Audit), where single-file views miss callers in other modules. | Single-file scope only builds intra-file edges (`ScopeFacts` + `IntraFileReference`). |
| **`omnicoach graph .`** *(Multi-File Directory Dump)* | Small directories (`2`–`3` files) or `--depth component` (`~2 KB`). | **Whole-repository invocation (Architectural Comprehension on `flash`):** `+14.7%` *more* tokens (`1.88M` vs. `1.64M`) than having no graph at all! | `omnicoach graph .` defaulted to `ProjectDepth::Full` across all `55+` files (`117`–`307 KB`), exceeding the `10 KB` inline tool output limit and forcing agents to grep truncated log files. |
| **`omnicoach graph slice <sym>`** *(Bounded Neighborhood)* | **Cross-module caller/callee tracing (Unreadable-File bugfix, Architectural Comprehension, Python Refactor):** `-31%` to `-71%` tokens on comprehension; `100%` caller recall with exact 1-indexed call-site lines. | **Multi-hop intra-file chains (Tarjan SCC bugfix, `ripgrep` bugfix) and multi-symbol queries (Comprehension Q1):** Query CLI took `4`–`6` sequential shell calls where `omnicoach graph <file>` took `1`. | `slice` accepted only a **single symbol** per CLI invocation (rebuilding the project graph each time) and was hardcoded to **`1` hop**. |
| **`omnicoach graph place <file>`** *(ADR 011 Simulator)* | **Pre-edit declaration placement (Config & Rule-Selection `[context]` Feature, Comprehension Q3):** Cut post-lint edit retries by `-31.3%` vs. Baseline (`-40.6%` vs. File Graph) and wall time by `-22.5%`. | Agents that skipped `place` before editing still needed `2`–`3` moves to satisfy `omnicoach code`. | `best_anchor` in [`call_clusters.rs`](../../../src/code_lint/ast/call_clusters.rs) scored candidate slots only by whether they fixed the *single* triggering rule (`private-before-public`) without checking whether the target slot violated another rule (`callee-before-caller`) on the same item (`2`-move cascade). |
| **`omnicoach graph impact --diff`** *(Post-Edit Blast Radius)* | **Post-edit verification (Unreadable-File bugfix, Python Refactor, `[context]` Feature):** Surfaced affected callers, `Divergent` component edges, and test files in `1` command. | On files with pre-existing ADR 011 lint debt (`src/config.rs` and `src/rule_selection.rs`), `impact --diff` listed ordering findings for *every* declaration in any touched module. | `render_divergent_edges` in [`query.rs`](../../../src/code_lint/graph/query.rs) filtered findings by `changed_modules` (whole file) rather than scoping to the modified `seeds` declarations. |

---

## 6. Part 3: Post-Benchmark Follow-Up Refinements & Empirical Evaluation

To eliminate the failure modes identified in `§5.3` and enforce the `< 10 KB` agent-harness inline stdout ceiling, we implemented three architectural and CLI improvements and evaluated them against deterministic tests and follow-up live subagent runs.

### 6.1 Follow-Up 1: Conflict-Free Single-Move Anchor Selection (`best_anchor`) & Seed-Scoped `impact --diff`

1. **Problem (`2`-Move Lint Cascade in `call_clusters.rs`):**
   - In [`call_clusters.rs`](../../../src/code_lint/ast/call_clusters.rs), `best_anchor` previously evaluated candidate insertion slots `new_pos` for a declaration `from` against a single violated rule `rule_to_fix` by minimizing `(broken_held, distance, new_pos)`, where `broken_held` counted only constraints that *already held before the move* and were broken by the move.
   - When moving a `pub` item above a `priv` item to fix `private-before-public` landed above a `priv` caller or type and triggered `callee-before-caller`, `best_anchor` recommended an anchor that left an existing or newly created constraint on `from` unsatisfied, forcing the agent to move the declaration twice.
2. **Fix ([`call_clusters.rs`](../../../src/code_lint/ast/call_clusters.rs) & [`query.rs`](../../../src/code_lint/graph/query.rs)):**
   - Upgraded `best_anchor` to score every candidate position where `rule_to_fix` holds by the 4-tuple:
     $$\text{score}(\text{new\_pos}) = \bigl(\text{broken\_held},\; \text{unfixed\_own},\; |\text{new\_pos} - \text{from}|,\; \text{new\_pos}\bigr)$$
     where `unfixed_own` counts how many of the moved item's own ADR 011 constraints (`private-before-public`, `callee-before-caller`, `constructor-after-method`, `uncolocated-helper`) remain violated at `new_pos`. Whenever a conflict-free slot (`broken_held == 0 && unfixed_own == 0`) exists anywhere in the scope, `omnicoach code` and `omnicoach graph` now point directly to that slot on the **first** diagnostic.
   - Updated `render_divergent_edges` in [`query.rs`](../../../src/code_lint/graph/query.rs) so `omnicoach graph impact --diff` reports declaration-ordering findings only when the declaration is in `seeds` (modified by the diff).
3. **Verification:**
   - Verified by unit test `test_collect_call_cluster_findings` (`case_6_private_before_public_and_callee_before_caller_selects_conflict_free_anchor`) in [`call_clusters.rs`](../../../src/code_lint/ast/call_clusters.rs) and `impact_reports_changed_declarations_callers_and_divergent_architecture` in [`query.rs`](../../../src/code_lint/graph/query.rs).
4. **Landing note:** The `best_anchor` 4-tuple ships (covered by case `private_before_public_and_callee_before_caller_selects_conflict_free_anchor` of `test_call_cluster_placement_anchors`); the seed-scoped `impact --diff` fix was removed with `impact` itself ([05_decisions.md](05_decisions.md) `D63`).

### 6.2 Follow-Up 2: Multi-Symbol Batching, `--hops <1..4>`, Shared Per-Invocation Budget, and Identifier-Token Overlap Fallback in `omnicoach graph slice` / `find`

1. **Problem (Sequential `1`-Hop / `1`-Symbol Shell Chatter, `10 KB` Inline Truncation, and Symbol-Name Drift):**
   - In the initial Stage D implementation, `omnicoach graph slice` accepted a single `target: String` and only rendered `1`-hop callers and callees. Tracing a `2`-hop chain (`compute_tarjan_scc -> analyze_declaration_scope -> collect_call_cluster_findings`) or querying two symbols (`SuppressionTracker::from_file` and `AstNode::from_span`) required multiple CLI invocations, rebuilding `ProjectFacts` from disk each time.
   - Conversely, naively multiplying the `8 KB` (`2,000`-token) default budget by symbol count or `hops` allowed multi-symbol or 2-hop slices to emit `14–16 KB`, crossing the `10 KB` agent-harness inline stdout truncation boundary.
   - Additionally, when an agent or prompt referred to a symbol by a slightly drifted camelCase/snake_case name (e.g., querying `omnicoach graph slice RuleSelectionConfig` when the struct in `src/rule_selection.rs` was named `RawSelection`), contiguous substring matching returned `(0 matches)`, triggering `20+` fallback file-read turns.
2. **Fix ([`query.rs`](../../../src/code_lint/graph/query.rs), [`omnicoach.rs`](../../../src/bin/omnicoach.rs), [`AGENTS.md`](../../../AGENTS.md)):**
   - Extended `SliceQueryOptions` and `SliceArgs` to accept multiple symbol patterns (`omnicoach graph slice <sym1> <sym2>... [paths...]`, building `ProjectFacts` once per language across all requested symbols) and `--hops <1..4>` (default `1`, clamped to `1..=4`).
   - Added BFS frontier traversal (`render_multi_hop_callers` and `render_multi_hop_callees`) that annotates intermediate hops (`[call via analyze_declaration_scope]`) with cycle-safe `visited` sets, reserving `25%` of the symbol's character budget (`MULTI_HOP_RESERVE_SHIFT = 2`) for hop-2+ entries so a large hop-1 caller list never starves hop 2.
   - Enforced `options.budget` (`default = 2,000` tokens = `8,000` bytes) as a **shared per-invocation budget** across all requested symbols in `slice_in_sources`, dynamically tracking `remaining_chars` while reserving `MIN_RESERVED_CHARS_PER_SYMBOL = 1,600` bytes for each subsequent symbol.
   - Added an **identifier-token overlap fallback (`MatchRank::TokenOverlap`)** in `score_declaration_match` / `find_in_sources`: when no exact, prefix, or contiguous substring match exists, `omnicoach graph find` (and `slice`'s fallback) splits the query into camelCase/snake_case identifier tokens (`len > 2`) and ranks declarations matching `>= 2` tokens across their qualified `module::symbol` path and `>= 1` token in their item name (so `RuleSelectionConfig` automatically surfaces `rule_selection::RawSelection`, `rule_selection::parse_config`, and `rule_selection::ConfigError`).
3. **Verification:**
   - Verified by unit tests `slice_supports_multiple_symbols_and_two_hop_neighborhood` and `find_falls_back_to_identifier_token_overlap_when_substring_misses` in [`query.rs`](../../../src/code_lint/graph/query.rs).
   - On Question 1 of the **Architectural Comprehension Audit** (`omnicoach graph slice SuppressionTracker::from_file AstNode::from_span`), a single command returns both complete caller/callee slices in `7,009` bytes (`0.46 s`), fitting `100%` inline without terminal truncation regardless of symbol argument order.
4. **Landing note:** Multi-symbol batching, `--hops`, and the shared budget ship. `MatchRank::TokenOverlap` and its test were removed before landing: the only evidence was this one task, and the token split returns noisy hits ([05_decisions.md](05_decisions.md) `D64`). `slice` now takes required positional symbols and a repeatable `--path <PATH>` to restrict the search.

### 6.3 Follow-Up 3: `8 KB` Multi-File Full-Graph Guard and Module-Edge Peer Capping

1. **Problem (`307–701 KB` Whole-Repo Dumps Exceeding `10 KB` Agent Terminal Limits):**
   - When an agent invoked `omnicoach graph .` without `--depth` on a multi-file repository, `generate_project_graphs` rendered `ProjectDepth::Full` (`307–701 KB` on `omnicoach`), spilling into truncated step output files. Furthermore, raw `ProjectDepth::ModuleOnly` on a `55`–`166`-module workspace was `35–42 KB` because foundational modules (`diagnostic`, `rule_declaration::*`) listed all `40+` incoming rule modules on a single line and large components emitted dozens of separate empty module headers with redundant module-level edges.
2. **Fix ([`graph.rs`](../../../src/code_lint/graph.rs) & [`render.rs`](../../../src/code_lint/graph/render.rs)):**
   - Added `MAX_MULTI_FILE_FULL_GRAPH_BYTES = 8_192` (`8 KB`) in `generate_project_graphs`: whenever `depth == ProjectDepth::Full` across multiple files (`files.len() > 1`) produces output exceeding `8 KB`, `omnicoach graph` automatically renders the `ProjectDepth::ModuleOnly` architectural summary followed by an actionable navigation footer pointing to `omnicoach graph <path/to/file>`, `omnicoach graph find <pattern>`, `omnicoach graph slice <symbol>... [--hops 2]`, and `omnicoach graph --depth public <dir>`.
   - In [`render.rs`](../../../src/code_lint/graph/render.rs), capped group-level edge peer lists (`MAX_MODULE_EDGE_PEERS = 2`, `MAX_COMPONENT_EDGE_PEERS = 6`, appending `, ... (+N more)`), capped empty module headers per component in `ModuleOnly` views (`MAX_EMPTY_MODULES_PER_COMPONENT = 2`, appending `// ... (+N more modules in <Component>)`), and suppressed redundant per-module edges on empty modules when `groups.len() > 20`, reducing `omnicoach graph .` across all `166` files to **`7,085` bytes (`100%` inline)** while preserving all `20` architectural components.
3. **Verification:**
   - Verified by unit test `full_depth_multi_file_graph_summarizes_when_exceeding_inline_budget` in [`graph.rs`](../../../src/code_lint/graph.rs).
4. **Landing note:** The size-triggered guard was replaced by a target-dependent default depth ([05_decisions.md](05_decisions.md) `D65`): without `--depth`, a single file renders at full depth and a directory or several files at module depth, so the module overview the guard fell back to is now the predictable default and an explicit `--depth` is never changed silently. The peer and empty-module caps above ship; the per-module edge omission above `20` modules is now announced by a notice line under the title. On the current repository, `omnicoach graph src` renders `8.9 KB` (`133` lines) inline. The tests are `multi_file_graph_defaults_to_module_depth`, `module_depth_graph_of_many_modules_omits_module_edges_with_notice`, and `multi_file_graph_honors_explicit_full_depth` in [`graph.rs`](../../../src/code_lint/graph.rs).

### 6.4 Empirical Evaluation of Follow-Ups on Live Subagent Trials

We evaluated the follow-up binary across five live subagent trials spanning the **Tarjan SCC Call-Order Bugfix (`inherit`, `n = 3` runs)**, the **4-Question Architectural Comprehension Audit (`flash`, `2` follow-up conditions: Turn-1 Batched Slice and Turn-1 `omnicoach graph .` Directory Guard)**, and the **Config & Rule-Selection `[context]` Feature (`flash`)**:

| Task & Evaluation Cohort | Model Tier | Cost-Weighted Tokens (Δ) | Planner Turns (Δ) | Pass / Accuracy | Observed Agent Behavior |
| :--- | :--- | ---: | ---: | :---: | :--- |
| **Tarjan SCC Call-Order Bugfix — Baseline (No Graph)** *(mean, `n=3`)* | `inherit` | `368K` (—) | `46.0` (—) | `100%` (`3/3`) | `20.3` search/read calls before first edit (`639 s`). |
| **Tarjan SCC Call-Order Bugfix — Initial Turn-1 Query CLI** *(mean, `n=3`)* | `inherit` | `197K` (`-46.4%` vs. Baseline) | `31.0` (`-32.6%`) | `100%` (`3/3`) | Used `4`–`5` sequential 1-hop `slice` calls to trace `ast.rs` call chain. |
| **Tarjan SCC Bugfix — Follow-Up Run 1 (Multi-Symbol Batched Slice)** | `inherit` | `248K` (`-32.7%` vs. Baseline) | `22.0` (`-52.2%`) | **`PASS`** | Combined `omnicoach graph src/code_lint/ast.rs` with **multi-symbol** `omnicoach graph slice compute_tarjan_scc analyze_declaration_scope` and `impact --diff`. |
| **Tarjan SCC Bugfix — Follow-Up Run 2 (2-Hop Slice)** | `inherit` | `245K` (`-33.6%` vs. Baseline) | `19.0` (`-58.7%`) | **`PASS`** | Immediately ran **`omnicoach graph slice compute_tarjan_scc --hops 2`** and **`omnicoach graph slice collect_call_cluster_findings --hops 2`**, tracing the 2-hop call chain in 1 query. |
| **Tarjan SCC Bugfix — Follow-Up Run 3 (File Graph + 1-Hop Slice)** | `inherit` | **`144K` (`-60.8%` vs. Baseline, `-26.8%` vs. Initial Query CLI)** | **`15.0` (`-67.4%` vs. Baseline, `-51.6%` vs. Initial Query CLI)** | **`PASS`** | Used `omnicoach graph src/code_lint/ast.rs` for hierarchy, one `1`-hop `slice`, and `impact --diff` without redundant overlapping `slice` calls (`-60.0%` cached prefix tokens). |
| **Tarjan SCC Bugfix — Follow-Up Mean (`3` Runs)** | `inherit` | **`212K` (`-42.4%` vs. Baseline)** | **`18.7` (`-39.8%` vs. Initial Query CLI)** | **`100%` (`3/3`)** | Organic adoption of both `--hops 2` and multi-symbol `slice`, cutting mean planner turns from `31.0` to `18.7` and cached prefix volume by `-37.4%`. |
| **Architectural Comprehension Audit (`flash`) — Baseline (No Graph)** *(mean, `n=3`)* | `flash` | `1,640K` (—) | `111.0` (—) | `83.3%` (`13.3/16` pts) | Manual `rg` and `read_file` across `166` files (`927 s`). |
| **Architectural Comprehension Audit (`flash`) — Initial File / Directory Graph** *(mean, `n=3`)* | `flash` | `1,881K` (`+14.7%` vs. Baseline) | `115.3` (`+3.9%`) | `95.8%` (`15.3/16` pts) | Ran `omnicoach graph .` (`700 KB` truncated dump), causing `+14.7%` token regression over Baseline (`816 s`). |
| **Architectural Comprehension Audit (`flash`) — Initial Turn-1 Query CLI** *(mean, `n=3`)* | `flash` | `475K` (`-71.0%` vs. Baseline) | `30.3` (`-72.7%`) | `100%` (`16/16` pts) | Separate CLI invocations for each symbol in Question 1. |
| **Architectural Comprehension Audit (`flash`) — Follow-Up (Turn-1 Batched Slice)** | `flash` | **`373K` (`-77.3%` vs. Baseline, `-21.4%` vs. Initial Query CLI)** | **`23.0` (`-79.3%` vs. Baseline, `-24.1%` vs. Initial Query CLI)** | **`100%` (`16/16` pts)** | Immediately batched **`omnicoach graph slice SuppressionTracker::from_file AstNode::from_span`** in Turn 1 (`7.0 KB`), answering both halves of Question 1 in one turn. |
| **Architectural Comprehension Audit (`flash`) — Follow-Up (`omnicoach graph .` Directory Guard)** | `flash` | **`523K` (`-72.2%` vs. Initial File/Dir Graph, `-68.1%` vs. Baseline)** | **`44.0` (`-61.8%` vs. Initial File/Dir Graph)** | **`100%` (`16/16` pts)** | Ran `omnicoach graph .` on Turn 1, received the **`7.1 KB` inline `ModuleOnly` summary + navigation footer**, and immediately pivoted to `slice` and `place` (`404 s`, **`-50.5%` wall time**). |
| **Config & Rule-Selection `[context]` Feature (`flash`) — Follow-Up (`place` & Symbol Drift Case Study)** | `flash` | `1,483K` (`-4.0%` vs. Baseline) | `87.0` | **`PASS` (`0` ordering retries)** | Used `omnicoach graph place src/rule_selection.rs` to insert `RuleSelectionConfig` after `RuleStatus:420` with **zero ADR 011 ordering retries**, while uncovering the `RuleSelectionConfig` vs. `RawSelection` symbol-drift lookup gap fixed by `MatchRank::TokenOverlap`. |

### 6.5 Critical Interpretation of the Follow-Up Evaluation

1. **Follow-Up 3 (`8 KB` Multi-File Full-Graph Guard) Eliminated the `+14.7%` Whole-Repo Regression on `flash` (`-72.2%` Cost-Weighted Tokens, `-61.8%` Turns, `-50.5%` Wall Time):**
   - The single most important safety result of the follow-up evaluation is the **Architectural Comprehension Audit (`omnicoach graph .` Directory Guard)** trial on `flash`. In the initial Stage D benchmark, running `omnicoach graph .` emitted a `307–700 KB` `ProjectDepth::Full` dump that exceeded the `10 KB` inline tool limit, causing `flash` to regress by `+14.7%` over Baseline (`1,881,474` vs. `1,640,053` cost-weighted tokens across `115.3` turns and `816.1 s`).
   - With Follow-Up 3's `8 KB` auto-summary guard and navigation footer (`7,085` bytes inline), when the `flash` subagent ran `omnicoach graph .` on Turn 1, it received all `20` architectural components inline and followed the footer's suggestion on Turn 2 (`omnicoach graph slice SuppressionTracker::from_file AstNode::from_span`, `omnicoach graph place src/rule_selection.rs`, and `omnicoach graph place src/code_lint/suppression.rs`), finishing with a **`16/16` (`100%`) rubric score in `44` turns, `523,258` cost-weighted tokens (`-72.2%` vs. Initial File / Directory Graph, `-68.1%` vs. Baseline), and `404.0 s` (`-50.5%` wall time)**.
2. **Multi-Symbol Batching and `--hops 2` Eliminated Sequential Query Round-Trips (`-39.8%` Turns on the Tarjan SCC Bugfix, `-24.1%` Turns on Architectural Comprehension):**
   - Both `inherit` and `flash` organically used the new CLI affordances on Turn `1`–`2` from the updated `AGENTS.md` instructions alone, cutting planner turns from `31.0 -> 18.7` on the Tarjan SCC bugfix (`-37.4%` cached prefix tokens) and from `30.3 -> 23` on the Architectural Comprehension Audit (`-28.5%` cached prefix tokens, `-21.4%` cost-weighted tokens).
3. **The Uncached vs. Cached Token Trade-Off on Small/Known Files (Overlapping `graph <file>` + `slice` Queries):**
   - Even though planner turns on the Tarjan SCC bugfix dropped by `-39.8%` (`31.0 -> 18.7`) and `cache_read_tokens` dropped by `-37.4%` (`1.07M -> 672K`), mean cost-weighted tokens across the 3 runs was `212,292` vs. `197,361` in the initial Turn-1 Query CLI (`+7.6%`), because uncached `input_tokens` rose in Follow-Up Run 1 (`84.9K`) and Follow-Up Run 2 (`81.7K`) while Follow-Up Run 3 dropped to `55.7K` (`144,468` cost-weighted tokens, **`-26.8%` below the initial Turn-1 Query CLI**).
   - **Root cause:** In Follow-Up Runs 1 and 2, the prompt recommended *both* `omnicoach graph <file>` (for named files) and `omnicoach graph slice ... [--hops 2]`. Those two runs executed `omnicoach graph src/code_lint/ast.rs` (which already prints every `[Lk]` declaration and intra-file edge in `ast.rs`) **and then also** ran multi-symbol or `--hops 2` `slice` queries on internal functions of `ast.rs`, paying uncached input tokens twice for the same file's call graph.
   - In Follow-Up Run 3, the agent used `omnicoach graph src/code_lint/ast.rs` for the hierarchy, ran one compact `1`-hop `slice compute_tarjan_scc`, and went straight to editing—finishing in `15` turns and `144,468` cost-weighted tokens.
   - **Action taken in [`AGENTS.md`](../../../AGENTS.md):** We updated `AGENTS.md` to frame `omnicoach graph <file>` and `omnicoach graph slice` as **mutually exclusive by scope**: use `omnicoach graph <file>` for **intra-file** hierarchy when the target file is known (and do not also run `slice` on internal functions of that same file unless cross-module callers are needed); use `omnicoach graph slice <sym>... [--hops 2]` for **cross-module** callers/callees or when the defining file is unknown.
4. **Zero ADR 011 Ordering Retries via `omnicoach graph place` vs. Symbol-Name Drift (`Config & Rule-Selection [context] Feature` Case Study):**
   - In the **Config & Rule-Selection `[context]` Feature** follow-up trial on `flash`, the subagent called `omnicoach graph place src/rule_selection.rs --name RuleSelectionConfig --calls "" --called-by "parse_config,rule_status"` before editing `src/rule_selection.rs`. The placement oracle computed `Ownership: Shared(rule_status, parse_config) | Roots: [rule_status, parse_config]` and recommended inserting after `RuleStatus` at line `420`. The subagent placed `RuleSelectionConfig` at that exact slot on its first edit and passed both `omnicoach graph impact --diff` (seed-scoped) and `omnicoach code --diff` with **zero ADR 011 ordering retries** (compared to `1.0` ordering retries in Baseline and `0.5` in Initial Full Graph Suite).
   - However, the trial also exposed a subtle usability gap: the task prompt referred to `RuleSelectionConfig` in `src/rule_selection.rs`, whereas at the base commit that private struct was still named `RawSelection`. When the subagent ran `omnicoach graph slice RuleSelectionConfig`, contiguous substring matching returned `(0 matches)`, causing the subagent to spend `28` turns searching the repository to verify whether `RuleSelectionConfig` existed elsewhere before realizing `RawSelection` was the struct to rename.
   - **Action taken in [`query.rs`](../../../src/code_lint/graph/query.rs):** We added `MatchRank::TokenOverlap` to `find_in_sources` so that when a multi-word camelCase or snake_case symbol query (`RuleSelectionConfig`) has no contiguous substring match, `omnicoach graph find` / `slice` automatically splits the query into identifier tokens (`["config", "rule", "selection"]`) and surfaces matching declarations in the target module (`rule_selection::RawSelection`, `rule_selection::parse_config`, `rule_selection::ConfigError`) on the first call.
5. **The `10 KB` Agent Harness Inline Truncation Cliff Is the True Budget Ceiling:**
   - In coding agent harnesses (`run_command`), stdout above **`~10 KB` (`10,240` bytes)** is truncated inline and spilled to `.system_generated/steps/<N>/output.txt`. Whenever a command crosses `10 KB`—either by chaining multiple `omnicoach graph` commands with `&&` in one shell call or by multiplying `--budget` across batched symbols/hops—the agent loses the middle of the output inline and must spend extra turns reading or scripting against the log file.
   - **Action taken in [`query.rs`](../../../src/code_lint/graph/query.rs), [`graph.rs`](../../../src/code_lint/graph.rs), and [`AGENTS.md`](../../../AGENTS.md):** We bounded `slice_in_sources` by a shared `8 KB` (`2,000`-token) per-invocation budget across all batched symbols/hops, lowered `MAX_MULTI_FILE_FULL_GRAPH_BYTES` to `8 KB` (`8,192` bytes) with `2`-peer module edge capping, and instructed agents in `AGENTS.md` to run one `omnicoach graph` command per tool call rather than chaining multiple `omnicoach graph` calls with `&&`.

> **Landing note:** The actions in items 1, 4, and 5 that rely on `place`, `impact --diff`, `MatchRank::TokenOverlap`, or `MAX_MULTI_FILE_FULL_GRAPH_BYTES` were revised before landing; see the landing notes in §6.1–§6.3. The one-command-per-tool-call instruction and the scope-disjoint `graph <file>` / `slice` routing ship in [`AGENTS.md`](../../../AGENTS.md).

---

## 7. Production Recommendations & Adopted `AGENTS.md` Guidelines

> **Landing note:** This section records the recommendation made from the benchmark. What landed differs in three places: the `place` and `impact` bullets of §7.1 item 2 and the matching `AGENTS.md` lines in §7.3 were removed in favor of `omnicoach code` diagnostics ([05_decisions.md](05_decisions.md) `D63`); the `8 KB` directory guard in §7.1 item 3 and §7.2 item 1 was replaced by a target-dependent default depth with visible caps (`D65`); and the unconditional "first step" trigger in §7.3 became conditional on the task needing the structure of unread code (`D59`). The current instruction block is [`AGENTS.md`](../../../AGENTS.md).

### 7.1 What Is Promoted to Production

1. **All Five Graph Resolution Fidelity Fixes:** Method-first `collect_intra_file_references` and `enclosing_scope_symbol` attribution (`Type::method`), two-segment `Type::method` and `Self::method` path resolution across modules and re-exports, 1-indexed call-site line provenance, usage-aligned `SignatureContract` (`[sig]`) rendering, and keeping `src/` rule files ending in `_test.rs` (`71.5% → 86.6%` exact SCIP recall, `0% → 100%` cross-module `Type::method` recall, `84.6% → 98.4%` precision, `0.46 s` full-repo build time).
2. **All Four Query Subcommands (`find`, `slice`, `place`, `impact`) Plus Single-File `omnicoach graph <file>`:**
   - `omnicoach graph <file>`: intra-file `[Lk]` call-cluster hierarchy when the target file is known.
   - `omnicoach graph find <pattern>`: fast symbol/span/root lookup when the defining file is unknown.
   - `omnicoach graph slice <symbol>... [--hops 1..4]`: batched cross-module callers (with 1-indexed call-site lines and snippets), callees (with signatures), cluster siblings, and explicit `rg` fallback command for receiver-typed method calls, bounded by a shared `8 KB` inline budget.
   - `omnicoach graph place <file> [--name <sym> --calls ... --called-by ...]`: authoritative ADR 011 `Roots`, `[Lk]` level, and conflict-free insertion line, backed by conflict-free `best_anchor` scoring (`(broken_held, unfixed_own, distance, pos)`).
   - `omnicoach graph impact --diff` / `--symbol <sym>`: post-edit blast radius, seed-scoped ADR 011 ordering findings, `Divergent` component reflexion check against `define_architecture!`, and affected test targets.
3. **Automatic `8 KB` Multi-File Directory Guard:** Invoking `omnicoach graph <dir>` when the full graph exceeds `8 KB` automatically summarizes to module-level architecture with capped peer lists and an explicit navigation footer.

### 7.2 What Is Kept Out of the Default Agent Loop

1. **Unbounded Whole-Repository Text Dumps:** Never recommend bare `omnicoach graph .` in agent instructions, and guard multi-file full graphs at `8 KB`.
2. **Always-On Background Daemons or External Indexers:** Because `omnicoach` recomputes the fresh graph from source in `0.28–0.46 s`, an on-demand CLI requires zero background daemons, zero stale-cache bugs, and zero external toolchain dependencies (`rust-analyzer scip` takes `44 s` warm).
3. **Heuristic Unqualified Method-Name Matching:** Keep receiver-typed method calls (`x.f()`) handled via the `slice` completeness footer's exact `rg '\.<method>\s*\(' src/` command rather than heuristic bare-name matching, preserving `98.4%` edge precision.

### 7.3 Production `AGENTS.md` Instruction Block (Adopted in This Repository)

```markdown
## Structural Navigation & Queries with `omnicoach graph`

Run `omnicoach graph` as your **first step** before multi-step `rg` or `read_file` exploration (run one `omnicoach graph` command per tool call rather than chaining multiple `omnicoach graph` commands with `&&`, so stdout stays within inline tool limits):

- **Intra-file call hierarchy & `[Lk]` clusters (when the target file is known):** run `omnicoach graph <file>` to see all `[Lk]` levels, `roots=[...]`, line spans (`:start..end`), and intra-file call edges in one view. Do not also run `slice` on internal functions of that same file unless you need their cross-module callers, and never run bare `omnicoach graph .` on the whole repository.
- **Locate a symbol or module across the repo (when the defining file is unknown):** run `omnicoach graph find <pattern>`.
- **Cross-module callers (with call-site lines), callees, and signature contracts:** run `omnicoach graph slice <symbol>...` (pass multiple symbols in one call to batch within the shared inline budget; add `--hops 2` for 2-hop cross-module or large-file call chains).
- **Compute valid ADR 011 declaration placement (`Roots`, `[Lk]`, insertion line) before adding or moving an item:** run `omnicoach graph place <file> --name <item> --calls <callees> --called-by <callers>`.
- **Verify blast radius, ADR 006 architecture conformance (`Divergent` edges), and affected tests after editing:** run `omnicoach graph impact --diff`.
```

---

## 8. TL;DR & Suggestions for Next Steps

### 8.1 TL;DR

1. **Yes — `omnicoach graph` provides a large, statistically significant efficiency and accuracy advantage when matched to the task's scope and bounded within inline terminal limits (`< 10 KB`):**
   - **Graph Fidelity (`rust-analyzer scip` & `jedi` ground truth):** Fixing method-first source attribution (`collect_intra_file_references` and `enclosing_scope_symbol` returning `Type::method`) and two-segment cross-module `Type::method` / `Self::method` path resolution raised Rust exact symbol-to-symbol recall from **`71.5%` to `86.6%` (`+15.1 pp`)**, cross-module `Type::method` recall from **`0.0%` to `100.0%` (`136/136`)**, literal precision from **`84.6%` to `98.4%`**, and builds the full `omnicoach` repository in **`0.46 s`** (vs. `44 s` warm for `rust-analyzer scip`).
   - **84-Run Benchmark (`6` Tasks, `7` Task-Tier Units):** Voluntary Query Subcommands reduced cost-weighted tokens by **`-38.4%` (`95% CI [-50.5%, -25.0%]`)** on the 6 primary tasks (`-42.2%` across all 84 runs) and cut pre-edit search/read calls by **`-52.5%` (`29.6 → 14.1`)**. Turn-1 Query Subcommands reduced tokens by **`-45.0%` (`95% CI [-58.9%, -25.3%]`)** across all 84 runs and by **`-52.4%` (`-36.6%` wall-clock time)** on `flash`.
   - **Where It Helps Quality vs. Efficiency:** On code-modification tasks where `cargo test` + `omnicoach code` act as a closed-loop oracle, all conditions achieved a `100%` pass rate (`84/84`), and the graph's primary benefit is **trajectory efficiency** (fewer turns, fewer file reads, lower cached-prefix compounding, and `-31%` to `-100%` fewer ADR 011 ordering retries). On open-ended **Architectural Comprehension** without a test oracle, `omnicoach graph` directly improved **accuracy** from `91.7%` (`inherit`) / `83.3%` (`flash`) in Baseline to **`100.0%`**—and enabled **`flash` + Query CLI (`473K` tokens, `374 s`) to beat `inherit` + Baseline (`579K` tokens, `429 s`) by `-18.2%` in tokens and `-12.8%` in wall time**.
   - **Post-Benchmark Follow-Up Refinements:**
     - **Conflict-Free Anchor Scoring (`best_anchor` 4-tuple) + Seed-Scoped `impact --diff`:** Eliminates `2`-move ADR 011 lint cascades (`private-before-public` $\to$ `callee-before-caller`) and scopes `impact --diff` ordering findings strictly to modified declarations.
     - **Multi-Symbol Batching, `--hops 1..4`, Shared `8 KB` Budget, and `MatchRank::TokenOverlap` Fallback:** Cut planner turns on the **Tarjan SCC Call-Order Bugfix (`inherit`)** by an additional **`-39.8%` (`31.0 → 18.7` turns, best run `15` turns / `144K` tokens = `-60.8%` vs. Baseline)** and on the **Architectural Comprehension Audit (`flash`)** by **`-24.1%` (`30.3 → 23` turns, `373K` tokens = `-77.3%` vs. Baseline)**.
     - **`8 KB` Multi-File Full-Graph Guard (`omnicoach graph .`):** Eliminated the `+14.7%` whole-repo dump regression on `flash`, cutting cost-weighted tokens from **`1.88M` to `523K` (`-72.2%` vs. Initial File/Dir Graph, `-68.1%` vs. Baseline)** and wall time by **`-50.5%` (`816 s → 404 s`)**.

### 8.2 Suggestions for Next Steps

> **Landing note:** P0 shipped without `place`, `impact`, `MatchRank::TokenOverlap`, and the `8 KB` guard ([05_decisions.md](05_decisions.md) `D63`–`D65`). P1 landed as ownership-aware `omnicoach code` diagnostics with the conflict-free `best_anchor` ([13_stage_e_diagnostic_placement_design.md](13_stage_e_diagnostic_placement_design.md)), which replaced `place` instead of complementing it.

| Priority | Suggestion | Why It Matters & Expected Impact |
| :---: | :--- | :--- |
| **P0 (Done & Ready to Merge)** | **Ship the Stage D + Follow-Up `omnicoach graph` stack and updated `AGENTS.md` as the default developer/agent workflow.** | All `34` `graph` unit tests, `call_clusters` anchor tests, and repository self-lint checks pass cleanly, with `0.46 s` full-repo latency and `< 8 KB` inline output guarantees across every subcommand. |
| **P1 (High-Leverage Linter Integration)** | **Embed `omnicoach graph place` anchor hints directly into `omnicoach code` ADR 011 diagnostic remediation messages.** | Even when an agent forgets to run `omnicoach graph place` *before* editing, `omnicoach code` already runs after editing. Using the upgraded conflict-free `best_anchor` 4-tuple (`(broken_held, unfixed_own, distance, pos)`) and ownership summary (`Roots=[...] [Lk]`) directly inside `omnicoach code`'s fix hint gives agents 1-step placement remediation automatically without an extra CLI call. |
| **P2 (Cross-File Architecture & Dead-Code Rules)** | **Promote `ProjectFacts` into a shared `CodeLintProject` component (`D37`) to power cross-file lint rules in `omnicoach code`.** | Now that `ProjectFacts` achieves `86.6%` exact SCIP recall (`100%` on cross-module `Type::method` paths) and `98.4%` precision in `0.46 s`, `omnicoach code` can reuse it directly for: (a) native ADR 006 `define_architecture!` conformance linting (replacing bespoke test-only checking), (b) intra-component module cycle detection (`L6`), and (c) cross-module unused-public-item / dead-abstraction detection. |
| **P3 (Optional Receiver-Type Heuristics)** | **Add local `let x: Type = ...` / parameter-type binding resolution for receiver-typed method calls (`x.method()`).** | The remaining `13.4%` SCIP recall gap (`376` receiver-typed method edges + `150` trait dynamic-dispatch edges) comes from method calls on local variables (`tracker.is_suppressed()`, `node.children()`). Resolving explicit parameter type annotations (`fn foo(tracker: &SuppressionTracker)`) and simple `let x = Type::new(...)` bindings in `ScopeFacts` would capture over half of the remaining receiver-typed calls without requiring a full compiler type-checker or sacrificing the `98.4%` precision floor. |
