# Performance & Benchmarking: 01 Understand

> [!NOTE]
> **Status: COMPLETED (2026-10-08).** Establishing a permanent, interpretable benchmark and
> performance evaluation suite for Omni (`code_lint` and `command_lint`), plus a 3-way historical
> evaluation across the `ast-grep` baseline (`ytmwoktu`), the P3 dedicated AST migration
> (`yvmzyprn`), and the SOTA semantic index (`kwkyuqvs`).

---

## 1. Core Problem & Motivation

A benchmark suite is only useful over the lifetime of a project if its output **explains *why* a
number moved**, not just *that* a number moved. Specifically, we need to answer two core questions
before writing any benchmark code:

1. **What to benchmark**: Which pipeline layers, workloads (languages, file sizes, syntactic shapes),
   and extractor groups give a complete, non-redundant decomposition of Omni's runtime and memory
   profile?
2. **How to benchmark and present it**: How do we report results so a developer running the suite
   immediately sees:
   - Throughput (`MB/s` or `kloc/s`) normalized by input size,
   - Stage attribution (`Parse` vs. `Suppression scan` vs. `Shared semantic index` vs. `Rule passes`),
   - Cold-cache vs. warm-cache cost (isolating `OnceLock` shared projections from the rules that
     consume them), and
   - Memory / allocation pressure (allocations and bytes per file), not just noisy wall-clock time?

---

## 2. Goals & Non-Goals

### Goals
- **G1 — Informative, Interpretable Pipeline Decomposition**:
  Decompose per-file and multi-file execution into orthogonal stages (`Parse`, `Suppression`,
  `Shared AST/Semantic Index`, `Per-Rule / Per-Family Evaluation`, and `End-to-End File/Corpus`) so
  regressions immediately point to the responsible layer.
- **G2 — Meaningful Units & Metrics**:
  Report throughput (`MB/s` / `lines/s`), wall-clock time (`median` / `min` / `max`), and heap
  allocation counts/bytes where supported by the harness, across representative Python and Rust
  workloads.
- **G3 — Profiling Ergonomics (`[profile.profiling]`)**:
  Provide a dedicated Cargo profile (`inherits = "release"`, `debug = "line-tables-only"`,
  `strip = "none"`) and documented recipes for `perf` / `samply` flamegraphs.
- **G4 — 3-Way Historical Evaluation (`ast-grep` → P3 → Semantic Index)**:
  Measure the exact performance, CPU, memory, compile-time, and binary-size evolution across:
  1. `ytmwoktu` (`ast-grep` / `tree-sitter` baseline),
  2. `yvmzyprn` (P3 `ruff_python_ast` + `ra_ap_syntax` migration), and
  3. `kwkyuqvs` (SOTA semantic index & single-pass parameter visitor).
- **G5 — Roadmap Synchronization**:
  Update [ROADMAP.md](../../../ROADMAP.md) to reflect completed AST/semantic-index items and
  record empirically measured bottlenecks from the new benchmark suite.

### Explicit Non-Goals
- **NG1 — Production CLI `--timings` Flag**:
  Deferred. Omni has no third-party plugin ecosystem, and lazy `OnceLock` projections on
  `ParsedFile` (`collect_bindings`, `collect_call_candidates`, `ImportMap`, etc.) would cause naive
  per-rule wall-clock timers in `runner.rs` to blame whichever rule executes first in `CODE_RULES`.
  A dedicated benchmark harness measures cold vs. warm shared-index costs accurately without
  polluting production code.
- **NG2 — Speculative Engine Rewrites in This Track**:
  This track builds the measurement and benchmarking system and records the baseline numbers. Any
  new optimization opportunities uncovered by the benchmarks (e.g., unmemoized multi-consumer
  extractors) will be triaged with hard data first.
- **NG3 — Flaky Wall-Clock Assertions in `cargo test`**:
  Wall-clock thresholds do not belong in unit/integration tests due to runner variance (15–30%).

---

## 3. Decisions & Open Questions

### Decisions
- **D1 (No CLI `--timings` bloat)**: Keep timing and stage attribution in `benches/` and developer
  benchmarking tooling rather than adding atomic timing accumulators to `src/code_lint/runner.rs`.
- **D2 (Local SOTA statistical harness + profiling)**: Use a Rust statistical benchmark harness in
  `benches/` (selecting between `divan`, `criterion`, and a custom diagnostic table after Phase 2
  research & Phase 3 prototyping) paired with `hyperfine` for end-to-end CLI runs and
  `[profile.profiling]` for `samply` / `perf`.
- **D3 (Deterministic in-repo workloads)**: Drive `cargo bench` from deterministic, committed
  workloads covering realistic Python and Rust code shapes so benchmark numbers are comparable
  across commits and machines over the project's lifetime.
- **D4 (Measure first, optimize second)**: Separate benchmark construction and historical
  measurement from any subsequent rule/AST optimizations.
- **D5 (Fixture Corpus — 3 Pinned Real-World + 1 `kitchen_sink` per Language, Zero Repetition)**:
  Remove synthetic string multiplication (`repeat_factor`). Vendor 3 canonical real-world Python
  files from Ruff's `ruff_benchmark/resources/` (`numpy/globals.py`, `pydantic/types.py`,
  `large/dataset.py`) + 3 pinned real-world Rust files (`config.rs`, `annotations.rs`, `cli.rs`) +
  1 Oxc-style non-repeated `kitchen_sink` stress fixture per language under `benches/fixtures/`.
- **D6 (4 Top-Level Benchmark Groups Mirroring Ruff & Oxc)**: Structure
  [benches/omni_bench.rs](../../../benches/omni_bench.rs) into `parser`, `semantic`
  (`memoized_oncelock` + `unmemoized_extractors`), `linter` (`end_to_end_*`, `family_*`,
  `rule_*`), and `command_lint`.
- **D7 (Warm-Preparsed Rule Isolation in `linter::family_*` and `linter::rule_*`)**: Match Oxc's
  `benches/linter.rs` by pre-parsing `ParsedFile` and pre-warming all `OnceLock` indexes outside
  the timed loop in `family_*` and `rule_*`, and filtering `family_rust` to only the 3 families
  that have Rust rules (`A_call_candidates_oncelock`, `B_bindings_oncelock`,
  `E_dedicated_extractors`).

### Core Design Questions for Phase 2 & Phase 3 (Resolved)
- **Q1 (What to benchmark — Workload & Stage Taxonomy)**: Resolved via `D5` and `D6`.
- **Q2 (How to benchmark — Harness & Interpretability)**: Resolved via `D2`, `D6`, and `D7`.
