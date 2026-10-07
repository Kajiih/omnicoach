# Performance & Benchmarking: 07 Learnings & Retrospective

> [!NOTE]
> **Status: COMPLETED (2026-10-08).** Distills the key engineering lessons from designing,
> building, and running Omni's SOTA benchmark suite (`benches/omni_bench.rs`).

---

## 1. Why Real-World Pinned Fixtures (`D5`) Beat Repeated Synthetic Snippets (`repeat(N)`)

- **What happened**: Our initial prototype multiplied small 50-line snippets via `source.repeat(10)` to reach ~20 KB. When we inspected Ruff (`crates/ruff_benchmark/src/lib.rs`) and Oxc (`tasks/common/src/test_file.rs`) and replaced those repeated snippets with 3 real-world pinned files (`numpy/globals.py`, `pydantic/types.py`, `large/dataset.py`; `config.rs`, `annotations.rs`, `tests/cli.rs`) + 1 non-repeated `kitchen_sink` stress file per language (`D5`), the dominant bottlenecks shifted dramatically:
  - On repeated synthetic snippets, every function had a tiny 2-line body and shallow module tree, hiding the $O(F \times N)$ cost of `find_expr_at_span` in [annotations.rs](../../../src/code_lint/ast/python/annotations.rs).
  - On `pydantic/types.py` (`py_real_typed_lib`, 815 LOC of real-world classes, validators, and type annotations), `find_expr_at_span` re-walking the full 815-line module AST for every parameter and return annotation immediately surfaced as **>53% of total warm Python rule runtime** (`8.30 ms` in `C_py_signatures_unmemoized` + `1.19 ms` in `nullable-collection-return`).
- **Takeaway**: Never use `source.repeat(N)` for AST/linter benchmarks. It creates unrealistically flat scope tables, artificially warms the CPU branch predictor on identical 50-line loops, and distorts $O(\text{depth})$ vs. $O(N^2)$ tree walks.

---

## 2. Why `divan::AllocProfiler` Is Essential Alongside Wall-Clock Timing (`D2`)

- **What happened**: Wall-clock timing alone showed that Rust rules took `~2.7 ms` on `annotations.rs` (`rs_real_ast_module`). Adding `divan::AllocProfiler` (`#[global_allocator]`) immediately explained *why*: `end_to_end_all_rules` on a single 23.6 KiB Rust file performed **46,843 heap allocations (2.22 MB)** — almost `20×` more allocations than Stage-1 parsing (`2,448` allocations).
- **Root cause**: `ra_ap_syntax` uses Rowan's lossless green/red tree, which dynamically allocates heap-backed red-node cursors during every `syntax.descendants()` traversal (`~4,400–8,800` allocations per pass on `annotations.rs`), whereas `ruff_python_ast` stores AST nodes in contiguous `Vec`/`Box` slices (`0` allocations per visitor pass).
- **Takeaway**: Always pair wall-clock + throughput (`BytesCount`) with an in-process allocation profiler (`divan::AllocProfiler`). Heap allocation counts are 100% deterministic across runs (0% OS scheduling jitter) and immediately distinguish algorithmic CPU work from allocator traffic.

---

## 3. Warm-Preparsed Rule Isolation (`D7`) Prevents Double-Counting `OnceLock` Initialization

- **What happened**: In a lazy-indexed architecture (`OnceLock` on `ParsedFile`), benchmarking individual rules on a freshly parsed `ParsedFile` attributes the entire cold-cache `OnceLock` build cost (`collect_call_candidates`, `collect_bindings`, `ImportMap`) to whichever rule happens to run first.
- **Solution**: Following Oxc's `benches/linter.rs`, pre-parsing `ParsedFile` and pre-warming all `OnceLock` indexes in `bencher.with_inputs(...)` outside the timed loop (`D7`) cleanly separates Group 2 (`semantic::memoized_oncelock` cold-cache initialization) from Group 3 (`linter::family_*` and `linter::rule_*` marginal rule evaluation).
