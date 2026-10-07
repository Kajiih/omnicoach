# Performance & Benchmarking: 03 Design & Plan

> [!NOTE]
> **Status: IMPLEMENTED (2026-10-08).** Defines Critical User Journeys (CUJs), acceptance criteria,
> fixture corpus (`D5`), 4 top-level benchmark groups (`D6`), warm-preparsed rule isolation (`D7`),
> and the ordered execution plan for Omni's SOTA benchmark and performance evaluation suite.

---

## 1. Critical User Journeys (CUJs) & Definition of Done

We define "done" independently of implementation details via four developer journeys:

| ID | Critical User Journey | Command | Acceptance Criteria & Metrics |
| :--- | :--- | :--- | :--- |
| **CUJ-1** | **Parser & Semantic Layer Decomposition**: An engineer wants to see how time and heap allocations divide across stage-1 parsing (`parser::parse_file`), suppression scanning (`suppression_scan`), memoized `OnceLock` indexes (`memoized_oncelock`), and unmemoized AST extractors (`unmemoized_extractors`) for Python and Rust. | `cargo bench --bench omni_bench -- parser`<br>`cargo bench --bench omni_bench -- semantic` | Outputs a hierarchical tree table with **wall-clock (`fastest`/`median`/`mean`)**, **throughput (`MB/s` / `GB/s`)**, and **exact heap allocations (`alloc count` + `allocated bytes`)** across all 8 pinned fixtures (4 Python, 4 Rust). |
| **CUJ-2** | **End-to-End Linter & Warm-Preparsed Rule Attribution**: An engineer adds or modifies a lint rule and wants to measure (a) full end-to-end linting vs. runner floor (`end_to_end_all_rules` vs. `end_to_end_no_rules`), and (b) pure marginal rule-family (`family_python`, `family_rust`) and per-rule (`rule_python`, `rule_rust`) evaluation on pre-parsed, pre-warmed `ParsedFile` instances (`D7`). | `cargo bench --bench omni_bench -- linter` | Pre-parses `ParsedFile` and pre-warms all shared `OnceLock` indexes outside the timed loop (`D7`) for `family_*` and `rule_*`, reporting both rule-family and per-rule time, throughput, and heap allocations. |
| **CUJ-3** | **Command Linter & Shell Pipeline Benchmarking**: An engineer modifies `command_lint` and wants to measure shell parsing (`InterceptedCommand::parse`) and rule dispatch (`run_command_lint`). | `cargo bench --bench omni_bench -- command_lint` | Benchmarks simple commands, compound `&&` / `||` / `;` pipelines, and wrapper commands (`env`, `sudo`, `nohup`) with throughput and allocation counts. |
| **CUJ-4** | **Symbolicated CPU Flamegraphs & 3-Way Historical Evaluation**: An engineer wants to profile hot spots with `perf` / `samply` or review the one-time historical comparison across commits (`ast-grep` baseline `ytmwoktu` vs. P3 migration `yvmzyprn` vs. Semantic Index `kwkyuqvs`). | `cargo build --profile profiling` & historical evaluation | `[profile.profiling]` builds with `lto = "fat"`, `debug = "line-tables-only"`, `strip = "none"`; one-time 3-way evaluation table recorded in [04_execution_log.md](04_execution_log.md) (`D4`). |

---

## 2. Fixture Corpus (`D5`) & Self-Dogfooding Isolation (`D3`)

Following Ruff (`crates/ruff_benchmark/src/lib.rs`) and Oxc (`tasks/common/src/test_file.rs`), we use **zero artificial string repetition (`repeat(N)`)** and pin **3 real-world files + 1 `kitchen_sink` stress file per language** (8 fixtures total) under `benches/fixtures/*.fixture`:

| Language | Fixture ID | Pinned Source | Size (Bytes / LOC) | Target Path Role | Syntactic & Semantic Shape Stressed |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Python** | `py_real_small` | Ruff benchmark corpus: `numpy/globals.py` | 3,094 B / 94 LOC | `src/globals.py` | Small real-world module baseline: copyright header, module docstrings, small classes, module constants. |
| **Python** | `py_real_typed_lib` | Ruff benchmark corpus: `pydantic/types.py` | 26,742 B / 815 LOC | `src/types.py` | Dense real-world typing & class hierarchy (`Optional`, `Union`, generics, `@dataclass`, validators, type annotations). |
| **Python** | `py_real_test_suite` | Ruff benchmark corpus: `large/dataset.py` | 42,660 B / 1,105 LOC | `tests/test_dataset.py` | Large real-world call-, literal-, indexing-, and statement-heavy workload evaluated under `TestsOnly` + production rules. |
| **Python** | `py_kitchen_sink` | Hand-crafted non-repeated Omni stress module | 15,947 B / 521 LOC | `src/kitchen_sink.py` | Exercises every Python rule family (`Protocol`, `@dataclass`, `MutableMapping`, `os.environ`, `logging`, `try/except`, `asyncio.create_task`, `unittest.mock`, `pytest.raises`, `omni:ignore`). |
| **Rust** | `rs_real_small` | Pinned snapshot of `src/code_lint/config.rs` | 6,241 B / 172 LOC | `src/config.rs` | Small real-world Rust module with `serde` structs, `HashMap` collections, methods, and doc comments. |
| **Rust** | `rs_real_ast_module` | Pinned snapshot of `src/code_lint/ast/python/annotations.rs` | 24,148 B / 633 LOC | `src/annotations.rs` | Large real-world Rust AST analysis module with deep `match` trees, slices, generics, and helper functions. |
| **Rust** | `rs_real_test_suite` | Pinned snapshot of `tests/cli.rs` | 11,713 B / 362 LOC | `tests/cli_test.rs` | Real-world Rust integration test suite with `#[test]`, assertions, raw string literals, and command builders. |
| **Rust** | `rs_kitchen_sink` | Hand-crafted non-repeated Omni stress module | 8,378 B / 256 LOC | `src/kitchen_sink.rs` | Exercises every Rust rule family (`std::env::var`, `thread::sleep`, `Duration::from_millis`, tuple indexing `.0`/`.1`, `#[cfg(test)]`, `assert!(_ && _)`, `omni:ignore`). |

### Self-Dogfooding Isolation (`D3`)
1. Benchmark fixture files are stored under `benches/fixtures/*.fixture` and embedded at compile time via `include_str!`. Since their extension is `.fixture`, `Language::from_path` skips them during `omni-code-lint .` (`test_self_dogfooding_code_lint`).
2. The benchmark harness [omni_bench.rs](../../../benches/omni_bench.rs) passes 100% of Omni's own production Rust lints (`single-letter-name`, `abbreviated-name`, `type-suffixed-name`, `bare-multiline-string`, `repeated-literal`, etc.).

---

## 3. Benchmark Suite Architecture (`D6`, `D7`)

We use **`divan = "0.1.21"`** in `[dev-dependencies]` with `#[global_allocator] static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();` (`D2`).

```text
benches/omni_bench.rs
├── TestFile / ALL_TEST_FILES        // 8 pinned real-world + kitchen-sink fixtures + warm_shared_indexes
├── mod parser                       // Group 1 (Ruff parser.rs / Oxc parser.rs)
│   └── fn parse_file                // ParsedFile::new across all 8 fixtures
├── mod semantic                     // Group 2 (Oxc semantic.rs)
│   ├── mod memoized_oncelock        // Cold-cache cost of each OnceLock index on a fresh ParsedFile
│   │   ├── fn call_candidates
│   │   ├── fn bindings
│   │   ├── fn comment_nodes
│   │   ├── fn suppression_scan
│   │   ├── fn rust_inline_test_ranges
│   │   └── fn py_symbol_and_mutation_index
│   └── mod unmemoized_extractors    // Per-call cost of unmemoized AST extractors on a pre-parsed ParsedFile
│       ├── fn py_function_signatures
│       ├── fn py_parameter_usages
│       ├── fn py_classes
│       ├── fn py_class_attributes
│       ├── fn literal_occurrences
│       └── fn positional_reads
├── mod linter                       // Group 3 (Ruff linter.rs + Oxc linter.rs)
│   ├── fn end_to_end_all_rules      // Full lint_file (parse + suppressions + all rules)
│   ├── fn end_to_end_no_rules       // Full lint_file floor (parse + suppressions + 0 rules selected)
│   ├── fn family_python             // Warm-preparsed rule families A..E over the 4 Python fixtures (D7)
│   ├── fn family_rust               // Warm-preparsed rule families A, B, E over the 4 Rust fixtures (D7)
│   ├── fn rule_python               // Warm-preparsed individual Python rules (36 rules) (D7)
│   └── fn rule_rust                 // Warm-preparsed individual Rust rules (13 rules) (D7)
└── mod command_lint                 // Group 4 (Shell command parser & command_lint pipeline)
    ├── fn parse_shell_commands
    └── fn run_command_lint_pipeline
```

### Cargo Configuration ([Cargo.toml](../../../Cargo.toml))
1. **`[profile.profiling]`**:
   ```toml
   [profile.profiling]
   inherits = "release"
   debug = "line-tables-only"
   strip = "none"
   ```
2. **`[[bench]]` & `[dev-dependencies]`**:
   ```toml
   [[bench]]
   name = "omni_bench"
   harness = false
   ```
   With `divan = "0.1.21"` in `[dev-dependencies]`.

---

## 4. Ordered Execution Plan (Phase 4 → Phase 7)

1. **Step 1 — Cargo Configuration & Fixtures ([Cargo.toml](../../../Cargo.toml), `benches/fixtures/`)**:
   - Add `[profile.profiling]`, `[[bench]] name = "omni_bench"`, and `divan = "0.1.21"` to [Cargo.toml](../../../Cargo.toml).
   - Vendor the 8 pinned real-world and kitchen-sink fixtures under `benches/fixtures/`.
   - *Verify*: `cargo check --benches` compiles cleanly.
2. **Step 2 — Implement [omni_bench.rs](../../../benches/omni_bench.rs) (`parser`, `semantic`, `linter`, `command_lint`)**:
   - Implement the 4 benchmark modules with `divan::AllocProfiler` and `BytesCount` throughput counters, adhering strictly to Omni's self-dogfooding rules.
   - *Verify*: Run `cargo bench --bench omni_bench` and full quality gate (`fmt`, `clippy --all-targets`, `test`, `doc`).
3. **Step 3 — Execute Benchmarks & Record Empirical Findings ([04_execution_log.md](04_execution_log.md))**:
   - Run `cargo bench --bench omni_bench` on current `@` and record the full parser, semantic, linter, and command_lint tables (time, `MB/s`, allocations, allocated bytes), plus the one-time historical comparison (`D4`).
4. **Step 4 — Cleanup ([05_cleanup.md](05_cleanup.md)) & Roadmap Sync ([ROADMAP.md](../../../ROADMAP.md))**:
   - Verify no temporary files pollute the working tree.
   - Update [ROADMAP.md](../../../ROADMAP.md) with the benchmark suite status and the two high-leverage optimization opportunities uncovered by the benchmark suite.
5. **Step 5 — Independent Review & Audit ([06_review_and_audit.md](06_review_and_audit.md)) and Learnings ([07_learn.md](07_learn.md))**:
   - Run an independent review subagent to audit the benchmark suite, [Cargo.toml](../../../Cargo.toml), and docs against RICR and project rules.
   - Draft [07_learn.md](07_learn.md) and present for final review.
