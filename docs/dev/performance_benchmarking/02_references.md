# Performance & Benchmarking: 02 SOTA References & Internal Audit

> [!NOTE]
> **Status: DONE (2026-10-07).** Synthesizes SOTA linter benchmarking designs (Ruff, Oxlint/Oxc,
> Biome, `rust-analyzer`), Rust benchmark harness trade-offs (`divan` vs. `criterion` vs.
> `gungraun`), and a complete pipeline + extractor audit of Omni's 36 code rules.

---

## 1. How SOTA Rust Linters Design Durable, Interpretable Benchmarks

### 1.1 Ruff (`crates/ruff_benchmark/`, `ruff_python_parser/benches/`, `ruff_linter/benches/`)
- **Canonical Real-World Archetypes over Micro-Snippets**:
  Ruff benchmarks 5 distinct file archetypes chosen to stress orthogonal AST/lexer/rule axes:
  1. `numpy/__init__.py` (~10–15 KB): Small, import- and module-globals-heavy (stresses import resolution and module-scope checks).
  2. `pypinyin` (~20 KB): Unicode string and identifier heavy (stresses UTF-8 character/byte indexing and `LineIndex`).
  3. `pydantic/types.py` (~50–100 KB): Dense classes, decorators, type annotations, and generics (stresses signature/class/annotation visitors).
  4. `numpy/ctypeslib.py` (~30 KB): Procedural code dense with function calls, control flow, and indexing.
  5. `large/dataset.py` (~400 KB): Large file stressing AST heap allocation scaling and cache locality.
- **Strict Stage Separation (`Precomputed` AST vs. Full Parse+Lint)**:
  - `parser.rs` benchmarks `ruff_python_parser::parse_module(source)` in isolation (`MB/s`).
  - `linter.rs` pre-parses the AST outside the timed loop (`ParseSource::Precomputed`) so semantic-model construction and rule checks are measured **without** parser cost masking rule changes, and tests both `RuleSelector::Default` and `RuleSelector::All`.

### 1.2 Oxlint / Oxc (`tasks/benchmark/`)
- Splits benchmarks into:
  1. **Phase-isolated micro-benchmarks** (`lexer` → `parser` → `semantic` → `linter`), where each downstream phase receives pre-built upstream inputs in the setup closure outside the timed loop.
  2. **End-to-end multi-core CLI macro-benchmarks** via `hyperfine` across multi-file corpora to measure Rayon scheduling and directory walk overhead.

### 1.3 Biome (`biome_js_parser/benches/`, `biome_js_analyze/benches/`) & `rust-analyzer`
- Both use **Rowan lossless CSTs** (`SyntaxNode` / green tree), identical to Omni's `ra_ap_syntax` backend for Rust.
- Both separate **green-tree CST parsing** (`SourceFile::parse`) from **red-tree traversal & AST projection** (`SyntaxNode::descendants()`), because materializing red-tree parent pointers and walking `.descendants()` repeatedly across rules is often more expensive than the initial parse itself.

---

## 2. Rust Benchmark Harness Comparison

| Dimension | `divan` (`0.1.21`) | `criterion` (`0.5` / `0.8`) | `gungraun` (ex-`iai-callgrind`) | Custom Table Runner |
| :--- | :--- | :--- | :--- | :--- |
| **Terminal Output** | **Compact hierarchical tree + aligned matrix table** (`fastest \| slowest \| median \| mean \| samples \| iters`) | Verbose multi-line blocks (4–6 lines per bench; 36 rules × fixtures = 500+ lines) | Multi-line instruction / cache / RAM blocks per bench | Custom ASCII/Markdown tables |
| **Throughput (`MB/s`)** | Built-in via `divan::counter::BytesCount` (printed inline in the tree table) | Built-in via `Throughput::Bytes` | None (counts instructions, not `MB/s`) | Manual calculation |
| **Heap Allocation Profiling** | **Built-in on stable Rust**: `#[global_allocator] static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();` adds `allocs` & `bytes` columns directly beneath each timing cell | None built-in (requires separate `dhat-rs` binary) | Built-in via Valgrind DHAT (requires host `valgrind`) | Requires custom `GlobalAlloc` |
| **Pre-parsed Input Setup** | `bencher.with_inputs(|| ...).bench_local_values(...)` / `bench_local_refs(...)` | `iter_batched` | Library benchmark setup | Manual loop |
| **Dependencies & Compile Time** | **Lightweight** (~5s compile, no plotting crates); drop-in compatible with `codspeed-divan-compat` | Heavy (`plotters`, `ciborium`, `oorandom`) | Requires host `valgrind` + runner binary | Zero external dependencies |

### Why `divan` + `AllocProfiler` + `BytesCount` is the Strongest Fit for Omni
1. **Single-glance interpretability**: Every row in the tree shows **wall-clock time (`fastest` / `median` / `mean`) + throughput (`MB/s`) + heap allocations (`alloc count` / `allocated bytes`)**. When a rule or extractor is slow because it clones strings or re-allocates vectors on every AST walk, the allocation column explains *why* immediately.
2. **Note on `unsafe_code = "forbid"` in `Cargo.toml`**: `#[global_allocator]` in Rust is a safe attribute (`static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();` requires zero `unsafe` blocks in `benches/*.rs`), so it works cleanly with Omni's workspace-wide `unsafe_code = "forbid"`.

---

## 3. Internal Audit of Omni's Pipeline & All 36 Code Rules

### 3.1 The 5 Stages of `lint_file(path, content, config)`
1. **Stage 0 — Fast-Path Gating**: `Language::from_path` → `config.is_test_path` (`GlobSet`) → `should_skip_ast_parse`.
2. **Stage 1 — Parse & Line Index (`ParsedFile::new`)**:
   - Allocates `source: String`, builds `LineIndex::new(source)`, and parses via `ruff_python_parser::parse_module` (Python) or `ra_ap_syntax::SourceFile::parse` (Rust).
3. **Stage 2 — Suppression Directive Scan (`SuppressionTracker::from_file`)**:
   - Fast path (`!content.contains("omni:")`) vs. active path (`ast::collect_comment_nodes` + directive parsing).
4. **Stage 3 — Rust Inline Test Pre-pass**:
   - Non-test Rust files call `ast::rust::collect_inline_test_ranges(&file)` upfront (`OnceLock`).
5. **Stage 4 — Rule Dispatch (`CODE_RULES` — 36 rules across 35 modules)**:
   - Calls `rule.check_file(path, &file, overrides)` and filters diagnostics by `RuleTarget`.
6. **Stage 5 — Suppression Filtering & Audit**:
   - `tracker.filter_diagnostics(...)` + `tracker.audit(...)`.

### 3.2 Critical Architectural Finding: Memoized (`OnceLock`) vs. Unmemoized Shared Extractors

Auditing all 36 rules in `CODE_RULES` by the AST/semantic extractors they call reveals why naive per-rule timing is misleading and what our benchmark **must** separate:

| Extractor Group | Caching on `ParsedFile` | Lang | Rules Using It | What Happens During `lint_file` |
| :--- | :--- | :--- | :--- | :--- |
| **A. Call Candidates** (`ast::collect_call_candidates` + `ImportMap`) | **Memoized (`OnceLock`)** | Py, Rs | **10 rules** (`unstructured-task`, `sleep-in-tests`, `zero-sleep-in-tests`, `mock-in-tests`, `mock-call-assertion`, `error-log-in-except`, `suppressed-exception`, `type-cast`, `dynamic-attribute-access`, `environment-variable-in-function`) | First rule in `CODE_RULES` pays the AST walk + import resolution; the other 9 rules iterate a cached slice in nanoseconds. |
| **B. Bindings** (`ast::collect_bindings`) | **Memoized (`OnceLock`)** | Py, Rs | **4 rules** (`single-letter-name`, `abbreviated-name`, `primitive-duration`, `type-suffixed-name`) | First naming rule pays the AST walk; the other 3 filter the cached slice. |
| **C. Python Function Signatures** (`ast::python::extract_function_signatures`) | **UNMEMOIZED** | Py | **6 rules** (`identical-positional-types`, `concrete-collection-parameter`, `concrete-collection-return`, `mutable-collection-parameter`, `mutable-collection-return`, `specific-collection-parameter`) | **Walks the Python AST 6 separate times per file**, re-allocating `Vec<PythonFunctionSignature>` every time. |
| **D. Python Classes & Attributes** (`extract_classes` [3], `collect_class_attributes` [2], `collect_instance_attribute_annotations` [1]) | **UNMEMOIZED** | Py | **6 rules** (`fake-without-protocol`, `mutable-dataclass`, `unslotted-dataclass`, `concrete-collection-attribute`, `mutable-collection-attribute`, `inline-public-attribute-annotation`) | **Walks the Python AST 6 separate times per file** (`collect_class_attributes` also walks method bodies for mutations). |
| **E. Dedicated Single-Rule Extractors** (`collect_literal_occurrences`, `collect_positional_reads`, `find_unwrapped_multiline_strings`, `find_nested_functions`, `collect_format_strings`, `collect_logger_calls`, etc.) | Unmemoized (1 rule each) | Py, Rs | **10 rules** (`repeated-literal`, `repeated-index-access`, `bare-multiline-string`, `nullable-collection-return`, `mutable-module-constant`, `too-many-assertions`, `packed-assertion`, `nested-function`, `unmatched-logger-placeholder`, `quote-wrapped-placeholder`) | Each rule owns its own specialized AST/CST pass. |

### 3.3 Visibility & Self-Dogfooding Constraints for `benches/`
- **Public API**: Almost every stage, extractor, and rule (`ParsedFile::new`, `SuppressionTracker::from_file`, `lint_file`, `run_code_lint`, `CODE_RULES`, `ast::*`, `ast::python::*`, `ast::rust::*`, `command_lint::*`) is already `pub` on the `omni` library crate.
- **`tests/architecture_conformance.rs`**: Only inspects `src/`, so `benches/*.rs` requires no `architecture_component!` macro.
- **Self-Dogfooding (`test_self_dogfooding_code_lint`)**:
  - `omni-code-lint` walks all `.rs` and `.py` files in the repo, and `.omnilint.toml` only marks `tests/**` as `test_patterns`.
  - Therefore, benchmark harness code in `benches/*.rs` must obey Omni's production Rust rules (no single-letter names, no banned abbreviations like `cfg`/`src`/`ctx`), and any benchmark fixture files containing intentional lint patterns should use non-`.py`/`.rs` extensions (e.g., `.py.fixture` / `.rs.fixture` loaded via `include_str!`) so `omni-code-lint .` never flags benchmark data.
