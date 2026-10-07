# Performance & Benchmarking: 05 Cleanup & Roadmap Sync

> [!NOTE]
> **Status: COMPLETED (2026-10-08).** Documents workspace hygiene, temporary file removal,
> self-dogfooding verification, and [ROADMAP.md](../../../ROADMAP.md) synchronization.

---

## 1. Temporary Artifact & Fixture Hygiene

1. **Removed Prototype Synthetic Fixtures**:
   - Deleted the earlier synthetic prototype fixtures (`benches/fixtures/python_{small_module,types_and_classes,calls_and_literals,test_suite}.py.fixture` and `benches/fixtures/rust_{small_module,types_and_classes,calls_and_literals,test_suite}.rs.fixture`) that used `repeat_factor` (`source.repeat(N)`).
   - Replaced them with the 8 pinned real-world + kitchen-sink fixtures (`D5`):
     - `py_real_small.py.fixture` (Ruff `numpy/globals.py`, 3,094 B)
     - `py_real_typed_lib.py.fixture` (Ruff `pydantic/types.py`, 26,742 B)
     - `py_real_test_suite.py.fixture` (Ruff `large/dataset.py`, 42,660 B)
     - `py_kitchen_sink.py.fixture` (Omni Python stress module, 15,947 B)
     - `rs_real_small.rs.fixture` (`src/code_lint/config.rs`, 6,241 B)
     - `rs_real_ast_module.rs.fixture` (`src/code_lint/ast/python/annotations.rs`, 24,148 B)
     - `rs_real_test_suite.rs.fixture` (`tests/cli.rs`, 11,713 B)
     - `rs_kitchen_sink.rs.fixture` (Omni Rust stress module, 8,378 B)
2. **Removed Temporary 3-Way Corpus Directory**:
   - Confirmed `/usr/local/google/home/paquerot/.gemini/jetski/brain/904f3f0f-d119-4690-af30-3a9ec41733dd/scratch/bench_corpus_files` was cleaned up after timing completed.
3. **Production Code (`src/`) Untouched (`D1`)**:
   - Verified via `jj st` that zero files under `src/` or `tests/` were modified.

---

## 2. Roadmap Synchronization ([ROADMAP.md](../../../ROADMAP.md))

Updated [ROADMAP.md](../../../ROADMAP.md) across three sections:
1. **Architecture & Conformance**:
   - Marked **Dedicated AST Migration & Semantic Index for `code_lint`** as **Completed**, citing [04_execution_log.md](../ast_robustness/04_execution_log.md) and [04_execution_log.md](../semantic_index/04_execution_log.md).
2. **Performance & Concurrency Architecture**:
   - Added **Eliminate Quadratic $O(N^2)$ `find_expr_at_span` in Python Type Annotations & Memoize Python Signature/Class Extractors** with exact empirical measurements from [04_execution_log.md](04_execution_log.md).
   - Added **Single-Pass Rust CST Extraction to Eliminate Rowan Red-Tree Cursor Allocation Multiplication** with exact empirical measurements (`46,843` heap allocations on `annotations.rs`, `68,726` allocations in `E_dedicated_extractors`).
   - Removed the obsolete pre-P3 `Parse & Pipeline Floor Profiling` entry that referenced `ast-grep` Tree-sitter initialization.
3. **Performance Measurement, Tracing & Tooling**:
   - Marked **SOTA In-Process Benchmark Suite (`benches/omni_bench.rs`) & Profiling Profile (`[profile.profiling]`)** as **Completed** (`D1`–`D7`), and scoped future CI regression gating to simulated instruction-count tracking (CodSpeed / Valgrind Cachegrind).
