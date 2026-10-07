# Performance & Benchmarking: 04 Execution Log & Empirical Evaluation

> [!NOTE]
> **Status: COMPLETED (2026-10-08).** Records the implementation of the SOTA `divan` benchmark suite
> ([omni_bench.rs](../../../benches/omni_bench.rs)), the complete empirical measurements across all
> 8 pinned real-world and kitchen-sink fixtures (`D5`–`D7`), the 4 architectural performance insights
> uncovered by the benchmark, and the one-time historical comparison (`D4`).

---

## 1. Execution Summary

| Step | Artifact / File | Verification |
| :--- | :--- | :--- |
| **1. Cargo Configuration** | [Cargo.toml](../../../Cargo.toml) (`divan = "0.1.21"`, `[[bench]] name = "omni_bench" harness = false`, `[profile.profiling]`) | `cargo check --benches` (exit 0) |
| **2. Pinned Fixture Corpus (`D3`, `D5`)** | `benches/fixtures/{py_real_small,py_real_typed_lib,py_real_test_suite,py_kitchen_sink}.py.fixture`<br>`benches/fixtures/{rs_real_small,rs_real_ast_module,rs_real_test_suite,rs_kitchen_sink}.rs.fixture` | 8 non-repeated real-world + kitchen-sink fixtures (88.6 KB Python, 51.3 KB Rust) isolated from `omni-code-lint .` via `.fixture` extension |
| **3. SOTA Benchmark Harness (`D2`, `D6`, `D7`)** | [omni_bench.rs](../../../benches/omni_bench.rs) (`parser`, `semantic`, `linter`, `command_lint` + `divan::AllocProfiler` + `BytesCount`) | Full quality gate: `fmt=0 clippy=0 test=0 doc=0` (1,421 lib + 11 arch + 23 CLI + 17 registry + 8 doctests) |
| **4. Full Benchmark Execution** | `cargo bench --bench omni_bench` (30 samples × 8 fixtures / 49 language-rule cases) | 100% completion across all 4 top-level groups |

---

## 2. Group 1 — Stage 1 Parser (`parser::parse_file`)

Measures `ParsedFile::new` (`Language::from_path` + `ruff_python_parser::parse_module` / `ra_ap_syntax::SourceFile::parse` + `LineIndex::new`), without running any semantic extractors or lint rules.

| Language | Fixture | Size | Fastest | Median | Throughput (Median) | Heap Allocs | Allocated Bytes |
| :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| **Python** (`ruff_python_parser`) | `py_real_small` (`numpy/globals.py`) | 3.0 KiB | 19.63 µs | **21.48 µs** | **144.0 MB/s** | 97 | 17.54 KB |
| **Python** (`ruff_python_parser`) | `py_kitchen_sink` | 15.8 KiB | 460.2 µs | **547.8 µs** | **29.47 MB/s** | 2,127 | 228.5 KB |
| **Python** (`ruff_python_parser`) | `py_real_typed_lib` (`pydantic/types.py`) | 26.1 KiB | 730.9 µs | **843.6 µs** | **31.70 MB/s** | 2,318 | 365.4 KB |
| **Python** (`ruff_python_parser`) | `py_real_test_suite` (`large/dataset.py`) | 41.7 KiB | 1.375 ms | **1.531 ms** | **27.86 MB/s** | 5,940 | 557.2 KB |
| **Rust** (`ra_ap_syntax`) | `rs_real_small` (`config.rs`) | 6.1 KiB | 599.2 µs | **801.1 µs** | **7.79 MB/s** | 1,089 | 190.4 KB |
| **Rust** (`ra_ap_syntax`) | `rs_kitchen_sink` | 8.3 KiB | 652.1 µs | **976.7 µs** | **8.75 MB/s** | 1,355 | 258.5 KB |
| **Rust** (`ra_ap_syntax`) | `rs_real_test_suite` (`tests/cli.rs`) | 12.0 KiB | 1.119 ms | **1.192 ms** | **10.35 MB/s** | 1,444 | 257.6 KB |
| **Rust** (`ra_ap_syntax`) | `rs_real_ast_module` (`annotations.rs`) | 23.6 KiB | 2.135 ms | **2.391 ms** | **10.10 MB/s** | 2,448 | 414.5 KB |

**Interpretation**:
- `ruff_python_parser` parses real-world Python at **28–144 MB/s** (`~85–140` heap allocations per KiB of source).
- `ra_ap_syntax` constructs a full lossless Rowan green-tree CST for Rust at **8–10 MB/s** (`~100–175` heap allocations per KiB of source), making Stage-1 CST construction roughly **3× slower per byte** in Rust than AST construction in Python.

---

## 3. Group 2 — Semantic Layer (`semantic`)

### 3.1 Memoized `OnceLock` Indexes (`semantic::memoized_oncelock`, Cold-Cache Cost)

Measures the first-call initialization cost of each `OnceLock` index on a freshly parsed `ParsedFile`:

| Index | Fixture | Size | Median Time | Throughput | Heap Allocs | Allocated Bytes |
| :--- | :--- | ---: | ---: | ---: | ---: | ---: |
| **`suppression_scan`** | `py_real_small` (no `omni:`) | 3.0 KiB | **396.4 ns** | **7.80 GB/s** | **0** | **0 B** |
| **`suppression_scan`** | `py_real_typed_lib` (no `omni:`) | 26.1 KiB | **1.682 µs** | **15.90 GB/s** | **0** | **0 B** |
| **`suppression_scan`** | `py_real_test_suite` (no `omni:`) | 41.7 KiB | **3.025 µs** | **14.10 GB/s** | **0** | **0 B** |
| **`suppression_scan`** | `py_kitchen_sink` (has `omni:ignore`) | 15.8 KiB | **20.85 µs** | **774.2 MB/s** | 19 | 5.21 KB |
| **`suppression_scan`** | `rs_real_ast_module` (no `omni:`) | 23.6 KiB | **1.868 µs** | **12.93 GB/s** | **0** | **0 B** |
| **`suppression_scan`** | `rs_real_test_suite` (has `omni:` in test strings) | 12.0 KiB | **521.1 µs** | **23.66 MB/s** | 2,213 | 89.24 KB |
| **`call_candidates`** | `py_real_typed_lib` (26 KiB) | 26.1 KiB | **46.46 µs** | **575.5 MB/s** | 380 | 32.02 KB |
| **`call_candidates`** | `py_real_test_suite` (41 KiB) | 41.7 KiB | **192.6 µs** | **221.5 MB/s** | 2,000 | 145.6 KB |
| **`call_candidates`** | `rs_real_ast_module` (23 KiB) | 23.6 KiB | **435.5 µs** | **55.44 MB/s** | **6,293** | **267.9 KB** |
| **`bindings`** | `py_real_typed_lib` (26 KiB) | 26.1 KiB | **22.60 µs** | **1.18 GB/s** | 28 | 11.78 KB |
| **`bindings`** | `py_real_test_suite` (41 KiB) | 41.7 KiB | **67.62 µs** | **630.9 MB/s** | 88 | 21.84 KB |
| **`bindings`** | `rs_real_ast_module` (23 KiB) | 23.6 KiB | **355.5 µs** | **67.92 MB/s** | **4,928** | **209.1 KB** |
| **`comment_nodes`** | `py_real_test_suite` (41 KiB) | 41.7 KiB | **6.88 µs** | **6.20 GB/s** | 12 | 4.86 KB |
| **`comment_nodes`** | `rs_real_ast_module` (23 KiB) | 23.6 KiB | **460.7 µs** | **52.41 MB/s** | **8,863** | **356.5 KB** |
| **`rust_inline_test_ranges`** | `rs_real_ast_module` (23 KiB) | 23.6 KiB | **375.9 µs** | **64.23 MB/s** | **8,640** | **345.6 KB** |

### 3.2 Unmemoized Shared Extractors (`semantic::unmemoized_extractors`, Per-Call Cost)

Measures the cost of each unmemoized AST extractor on a pre-parsed `ParsedFile`:

| Extractor | Callers in `CODE_RULES` | Fixture | Median Time | Throughput | Heap Allocs | Allocated Bytes |
| :--- | :--- | :--- | ---: | ---: | ---: | ---: |
| **`py_function_signatures`** | **7 rules** (`concrete-collection-*`, `mutable-collection-*`, `nullable-collection-return`, `specific-collection-parameter`, `identical-positional-types`) | `py_real_typed_lib` (26 KiB)<br>`py_real_test_suite` (41 KiB)<br>`py_kitchen_sink` (15 KiB) | **102.4 µs**<br>**70.94 µs**<br>**42.47 µs** | 261.0 MB/s<br>601.3 MB/s<br>379.7 MB/s | 513<br>179<br>259 | 49.44 KB<br>22.51 KB<br>26.27 KB |
| **`py_parameter_usages`** | **3 rules** (`concrete-collection-parameter`, `mutable-collection-parameter`, `specific-collection-parameter`) | `py_real_typed_lib` (26 KiB)<br>`py_real_test_suite` (41 KiB)<br>`py_kitchen_sink` (15 KiB) | **31.91 µs**<br>**95.18 µs**<br>**26.12 µs** | 837.9 MB/s<br>448.1 MB/s<br>617.5 MB/s | 67<br>31<br>37 | 10.97 KB<br>4.59 KB<br>6.40 KB |
| **`py_classes`** | **5 rules** (`mutable-dataclass`, `unslotted-dataclass`, `fake-without-protocol`, `concrete-collection-attribute`, `mutable-collection-attribute`) | `py_real_typed_lib` (26 KiB)<br>`py_real_test_suite` (41 KiB)<br>`py_kitchen_sink` (15 KiB) | **25.29 µs**<br>**55.12 µs**<br>**19.21 µs** | 1.06 GB/s<br>773.8 MB/s<br>839.4 MB/s | 53<br>56<br>31 | 2.41 KB<br>3.48 KB<br>1.73 KB |
| **`py_class_attributes`** | **2 rules** (`concrete-collection-attribute`, `mutable-collection-attribute`) | `py_real_typed_lib` (26 KiB)<br>`py_real_test_suite` (41 KiB)<br>`py_kitchen_sink` (15 KiB) | **43.23 µs**<br>**61.07 µs**<br>**28.29 µs** | 618.4 MB/s<br>698.4 MB/s<br>570.0 MB/s | 129<br>128<br>118 | 2.70 KB<br>3.45 KB<br>3.37 KB |
| **`literal_occurrences`** | **1 rule** (`repeated-literal`) | `py_real_test_suite` (41 KiB)<br>`rs_real_ast_module` (23 KiB) | **367.8 µs**<br>**457.3 µs** | 116.0 MB/s<br>52.79 MB/s | 2,847<br>4,458 | 121.0 KB<br>176.5 KB |
| **`positional_reads`** | **1 rule** (`repeated-index-access`) | `py_real_test_suite` (41 KiB)<br>`rs_real_ast_module` (23 KiB) | **188.6 µs**<br>**197.8 µs** | 226.1 MB/s<br>122.0 MB/s | 399<br>4,388 | 18.78 KB<br>175.7 KB |

---

## 4. Group 3 — Linter End-to-End, Rule Families & Per-Rule Attribution (`linter`)

### 4.1 End-to-End All Rules vs. No-Rules Runner Floor (`end_to_end_all_rules` vs. `end_to_end_no_rules`)

| Language | Fixture | Size | `end_to_end_no_rules` (Parse + Suppression Floor) | `end_to_end_all_rules` (Full Linter) | Rule Evaluation Share (%) | Total Allocs (`all_rules`) | Total Allocated (`all_rules`) |
| :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| **Python** | `py_real_small` | 3.0 KiB | 31.11 µs (`99.4 MB/s`) | **250.3 µs** (`12.4 MB/s`) | 87.6% | 629 | 57.52 KB |
| **Python** | `py_kitchen_sink` | 15.8 KiB | 570.2 µs (`28.3 MB/s`) | **4.655 ms** (`3.47 MB/s`) | 87.8% | 12,188 | 797.5 KB |
| **Python** | `py_real_typed_lib` | 26.1 KiB | 826.9 µs (`32.3 MB/s`) | **12.56 ms** (`2.13 MB/s`) | **93.4%** | 12,232 | 1.11 MB |
| **Python** | `py_real_test_suite` | 41.7 KiB | 1.564 ms (`27.3 MB/s`) | **8.498 ms** (`5.02 MB/s`) | 81.6% | 26,357 | 1.65 MB |
| **Rust** | `rs_real_small` | 6.1 KiB | 652.1 µs (`9.57 MB/s`) | **1.869 ms** (`3.34 MB/s`) | 65.1% | 14,507 | 748.8 KB |
| **Rust** | `rs_kitchen_sink` | 8.3 KiB | 720.2 µs (`11.9 MB/s`) | **2.861 ms** (`2.99 MB/s`) | 74.8% | 23,517 | 1.15 MB |
| **Rust** | `rs_real_test_suite` | 12.0 KiB | 1.644 ms (`7.50 MB/s`) | **4.439 ms** (`2.78 MB/s`) | 63.0% | 32,027 | 1.49 MB |
| **Rust** | `rs_real_ast_module` | 23.6 KiB | 2.130 ms (`11.3 MB/s`) | **4.835 ms** (`4.99 MB/s`) | 55.9% | **46,843** | **2.22 MB** |

### 4.2 Warm-Preparsed Rule Families (`linter::family_python` & `linter::family_rust`, `D7`)

Evaluated across all 4 fixtures of each language (88.4 KB Python; 50.5 KB Rust) with `ParsedFile` pre-parsed and all `OnceLock` indexes (`call_candidates`, `bindings`, `ImportMap`, `rust_inline_test_ranges`, `comment_nodes`) pre-warmed outside the timed loop:

| Language | Rule Family | Rules in Family | Median Time | Throughput | Heap Allocs | Allocated Bytes | Share of Warm Rule Time |
| :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| **Python** | **`C_py_signatures_unmemoized`** | 6 | **8.303 ms** | **10.67 MB/s** | 8,330 | 670.6 KB | **38.6%** |
| **Python** | **`A_call_candidates_oncelock`** | 10 | **5.622 ms** | **15.77 MB/s** | 29,119 | 1.91 MB | **26.1%** |
| **Python** | **`E_dedicated_extractors`** | 10 | **5.019 ms** | **17.66 MB/s** | 12,088 | 532.4 KB | **23.3%** |
| **Python** | **`D_py_classes_unmemoized`** | 6 | **1.956 ms** | **45.32 MB/s** | 1,858 | 98.58 KB | **9.1%** |
| **Python** | **`B_bindings_oncelock`** | 4 | **634.3 µs** | **139.7 MB/s** | 8,440 | 336.9 KB | **2.9%** |
| **Rust** | **`E_dedicated_extractors`** | 6 | **6.275 ms** | **8.17 MB/s** | **68,726** | **2.76 MB** | **78.0%** |
| **Rust** | **`A_call_candidates_oncelock`** | 3 | **1.404 ms** | **36.50 MB/s** | 3,369 | 291.0 KB | **17.4%** |
| **Rust** | **`B_bindings_oncelock`** | 4 | **368.1 µs** | **139.3 MB/s** | 3,129 | 122.5 KB | **4.6%** |

### 4.3 Top Slowest Individual Rules on Warm `ParsedFile` (`linter::rule_python` & `linter::rule_rust`)

#### Python Top 10 Rules (over 88.6 KB Python corpus, warm `ParsedFile`)

| Rank | Rule Name | Family | Median Time | Throughput | Heap Allocs | Allocated Bytes |
| :---: | :--- | :--- | ---: | ---: | ---: | ---: |
| 1 | `concrete-collection-parameter` | `C_py_signatures` | **1.883 ms** | 47.07 MB/s | 1,622 | 120.7 KB |
| 2 | `mutable-collection-parameter` | `C_py_signatures` | **1.855 ms** | 47.78 MB/s | 1,636 | 122.4 KB |
| 3 | `specific-collection-parameter` | `C_py_signatures` | **1.611 ms** | 54.98 MB/s | 1,518 | 113.4 KB |
| 4 | `repeated-literal` | `E_dedicated` | **1.470 ms** | 60.27 MB/s | 6,885 | 297.1 KB |
| 5 | `nullable-collection-return` | `E_dedicated` (uses `extract_function_signatures`) | **1.194 ms** | 74.22 MB/s | 1,454 | 113.8 KB |
| 6 | `concrete-collection-return` | `C_py_signatures` | **910.4 µs** | 97.36 MB/s | 1,454 | 109.9 KB |
| 7 | `mutable-collection-return` | `C_py_signatures` | **835.0 µs** | 106.1 MB/s | 1,200 | 104.7 KB |
| 8 | `zero-sleep-in-tests` | `A_call_candidates` | **685.3 µs** | 129.3 MB/s | 2,901 | 187.9 KB |
| 9 | `concrete-collection-attribute` | `D_py_classes` | **678.9 µs** | 130.6 MB/s | 645 | 37.98 KB |
| 10 | `mutable-collection-attribute` | `D_py_classes` | **662.2 µs** | 133.9 MB/s | 561 | 35.14 KB |

#### Rust All 13 Rules (over 50.5 KB Rust corpus, warm `ParsedFile`)

| Rank | Rule Name | Family | Median Time | Throughput | Heap Allocs | Allocated Bytes |
| :---: | :--- | :--- | ---: | ---: | ---: | ---: |
| 1 | `bare-multiline-string` | `E_dedicated` | **1.437 ms** | 35.22 MB/s | **20,572** | **822.6 KB** |
| 2 | `packed-assertion` | `E_dedicated` | **1.067 ms** | 47.45 MB/s | **12,276** | **493.4 KB** |
| 3 | `zero-sleep-in-tests` | `A_call_candidates` | **1.057 ms** | 47.88 MB/s | 1,113 | 96.21 KB |
| 4 | `nullable-collection-return` | `E_dedicated` | **1.008 ms** | 50.23 MB/s | **11,399** | **458.1 KB** |
| 5 | `repeated-literal` | `E_dedicated` | **978.1 µs** | 51.77 MB/s | **10,446** | **424.8 KB** |
| 6 | `repeated-index-access` | `E_dedicated` | **662.7 µs** | 76.42 MB/s | **9,945** | **400.1 KB** |
| 7 | `environment-variable-in-function` | `A_call_candidates` | **285.3 µs** | 177.4 MB/s | 1,143 | 98.60 KB |
| 8 | `sleep-in-tests` | `A_call_candidates` | **275.4 µs** | 183.8 MB/s | 1,113 | 96.21 KB |
| 9 | `abbreviated-name` | `B_bindings` | **228.2 µs** | 221.9 MB/s | 1,612 | 60.82 KB |
| 10 | `too-many-assertions` | `E_dedicated` (`TestsOnly`) | **147.5 µs** | 343.3 MB/s | 4,088 | 163.2 KB |
| 11 | `primitive-duration` | `B_bindings` | **91.48 µs** | 553.5 MB/s | 761 | 25.21 KB |
| 12 | `type-suffixed-name` | `B_bindings` | **37.37 µs** | 1.36 GB/s | 728 | 22.26 KB |
| 13 | `single-letter-name` | `B_bindings` | **15.31 µs** | 3.31 GB/s | 28 | 14.22 KB |

---

## 5. Group 4 — Command Linter (`command_lint`)

| Benchmark | Workload | Size | Fastest | Median | Throughput | Heap Allocs | Allocated Bytes |
| :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| `parse_shell_commands` | `simple_cargo_test` | 39 B | 20.79 µs | **23.46 µs** | 1.66 MB/s | 17 | 977 B |
| `parse_shell_commands` | `env_wrapped_command` | 74 B | 21.40 µs | **36.25 µs** | 2.04 MB/s | 17 | 1.06 KB |
| `parse_shell_commands` | `compound_pipeline` | 76 B | 30.13 µs | **50.90 µs** | 1.49 MB/s | 35 | 1.84 KB |
| `run_command_lint_pipeline` | `simple_cargo_test` | 39 B | 13.11 µs | **18.39 µs** | 2.12 MB/s | 17 | 977 B |
| `run_command_lint_pipeline` | `env_wrapped_command` | 74 B | 21.92 µs | **36.29 µs** | 2.04 MB/s | 17 | 1.06 KB |
| `run_command_lint_pipeline` | `compound_pipeline` | 76 B | 31.28 µs | **52.04 µs** | 1.46 MB/s | 35 | 1.84 KB |

**Interpretation**:
- Over **95%** of `run_command_lint` time and 100% of steady-state heap allocations (`17` to `35` allocs) occur inside `InterceptedCommand::parse_all` (`tree-sitter-bash` via `ast-grep` + `shell_words::split`). Evaluating all command rules adds `< 2 µs` and `0` steady-state heap allocations when no external VCS subprocess is spawned.

---

## 6. Four Architectural Insights Uncovered by the SOTA Benchmark Suite

### Insight 1: Quadratic $O(N^2)$ `find_expr_at_span` in Python Type-Annotation Inspection ([annotations.rs](../../../src/code_lint/ast/python/annotations.rs))
- **Symptom**: On `py_real_typed_lib` (`pydantic/types.py`, 26.1 KiB), parsing takes `843.6 µs`, yet `end_to_end_all_rules` takes **`12.56 ms`** (`93.4%` in rules), and `C_py_signatures_unmemoized` + `nullable-collection-return` + `concrete/mutable-collection-attribute` account for **>65% of all Python rule time** (`~10.8 ms` out of `21.5 ms` across the 4 fixtures).
- **Root Cause**: `extract_function_signatures` itself only takes `102.4 µs` on `py_real_typed_lib`. However, `PythonParameter::type_node` and `PythonFunctionSignature::return_type_node` (and `PythonClassAttribute::type_node`) store an untyped `AstNode<'a>` (which holds only a `SourceSpan`). When `collect_collection_types(type_node, ...)` ([annotations.rs](../../../src/code_lint/ast/python/annotations.rs#L398)) inspects a parameter or return annotation, it calls `find_expr_at_span(parsed.syntax(), type_node.span())` ([python.rs](../../../src/code_lint/ast/python.rs#L69)), whose `ExprFinder` implements `SourceOrderVisitor::visit_expr` with span pruning **but does not override `visit_stmt`** — so it traverses every `Stmt` in the module before pruning at the expression level! (Similarly, `find_parameters_at_span` and `find_body_at_span` in `python.rs` lines 101–171 walk module statements without `visit_stmt` span pruning.)
- **Impact**: For a file with $F$ functions/params and $N$ AST statements, `collect_collection_types` is $O(F \times N)$ per rule — and because 8 rules (`concrete-collection-*`, `mutable-collection-*`, `nullable-collection-return`, `specific-collection-parameter`) call it without memoizing either `extract_function_signatures` or the resolved collection types, `pydantic/types.py` performs **thousands of full-module statement walks**!
- **Fix (logged to [ROADMAP.md](../../../ROADMAP.md))**: Pre-compute or attach the `Vec<CollectionType>` (or `&Expr` reference / pre-extracted annotation summary) directly during the single-pass `extract_function_signatures` and `collect_class_attributes` visitors (and add `visit_stmt` range pruning to `ExprFinder` / `ParametersFinder` / `BodyFinder`), plus memoize `extract_function_signatures`, `extract_classes`, and `collect_class_attributes` on `ParsedFile` via `OnceLock`.

### Insight 2: Rowan Red-Tree Cursor Allocation Multiplication in Rust CST Traversals ([rust.rs](../../../src/code_lint/ast/rust.rs))
- **Symptom**: On `rs_real_ast_module` (`annotations.rs`, 23.6 KiB), `end_to_end_all_rules` performs **46,843 heap allocations (2.22 MB)** for a single 23.6 KiB file! Across the 4 Rust fixtures, `E_dedicated_extractors` performs **68,726 heap allocations (2.76 MB)**.
- **Root Cause**: `ra_ap_syntax` uses Rowan's green/red tree architecture. Unlike `ruff_python_ast` (where AST nodes live in contiguous `Vec`/`Box` slices and traversing them allocates **0 bytes**), iterating `syntax.descendants()` or `syntax.descendants_with_tokens()` on a Rowan `SyntaxNode` dynamically allocates heap-backed red-node wrappers for every interior node visited (`~4,400` to `~8,800` heap allocations per full-file walk on `annotations.rs`).
- **Impact**: Every Rust extractor that independently calls `syntax.descendants()` (`bare-multiline-string` = 20,572 allocs, `packed-assertion` = 12,276 allocs, `nullable-collection-return` = 11,399 allocs, `repeated-literal` = 10,446 allocs, `repeated-index-access` = 9,945 allocs) pays an additional `~4,400–8,800` Rowan red-node heap allocations per file.
- **Fix (logged to [ROADMAP.md](../../../ROADMAP.md))**: Fuse the Rust `OnceLock` and dedicated CST extractors into a single top-down `syntax.descendants_with_tokens()` pass per file so the Rowan red-tree is materialized only once.

### Insight 3: `A_call_candidates_oncelock` Allocation Overhead in `enclosing_non_exempt_function_name`
- **Symptom**: Even when `call_candidates` is pre-warmed in `OnceLock`, each banned-call rule (`sleep-in-tests`, `zero-sleep-in-tests`, `type-cast`, `unstructured-task`, `suppressed-exception`) takes `450–685 µs` and `2,870–2,901` allocations on the Python corpus.
- **Root Cause**: `find_banned_calls` iterates over all `call_candidates` and checks `ImportMap::resolve` / `enclosing_non_exempt_function_name` or `CallPattern` matching per rule.

### Insight 4: `SuppressionTracker::from_file` Fast-Path Effectiveness — and False-Trigger on String Literals
- **Fast-path verified**: On real-world files without the substring `"omni:"` (`py_real_small`, `py_real_typed_lib`, `py_real_test_suite`, `rs_real_small`, `rs_real_ast_module`), `SuppressionTracker::from_file` runs at **7.8–15.9 GB/s** (`396 ns` – `3.0 µs`) with **0 heap allocations**.
- **Edge case discovered on `rs_real_test_suite` (`tests/cli.rs`)**: `tests/cli.rs` has no actual `// omni:ignore` comments, yet `suppression_scan` took **521.1 µs** and **2,213 heap allocations** because `tests/cli.rs` contains the substring `"omni:"` *inside test string literals*, triggering `collect_comment_nodes` (`syntax.descendants_with_tokens()`) on the Rowan CST.

---

## 7. One-Time 3-Way Historical Evaluation (`D4`)

Per Decision `D4`, we recorded a one-time comparison across the three architectural milestones:
1. **`B1` (`ast-grep` baseline, pre-P3)**: Tree-sitter + `ast-grep` meta-variable pattern matching across 28 bundled C/C++ grammars (`scratch/perf/omni-code-lint.baseline`).
2. **`B2` (`yvmzyprn`, P3 Dedicated AST Migration)**: Pure-Rust `ruff_python_parser` + `ra_ap_syntax`, `OnceLock` on `call_candidates`/`bindings`/`comment_nodes`/`rust_inline_test_ranges` (`scratch/omni-base`).
3. **`B3` (`kwkyuqvs`, SOTA Semantic Index)**: Adds `ImportMap` (`OnceLock<ImportMap>`) for canonical import-alias resolution across banned calls and Python collection annotations (`target/release/omni-code-lint`).

| Metric / Workload | `B1` (`ast-grep` Baseline) | `B2` (`yvmzyprn`, P3 Migration) | `B3` (`kwkyuqvs`, Semantic Index) | Delta (`B2` → `B3`) |
| :--- | ---: | ---: | ---: | :--- |
| **Binary Size (`omni-code-lint`)** | 46,631,936 B (44.47 MiB, unstripped; ~28 grammars) | **5,932,464 B (5.66 MiB)** | **5,888,776 B (5.62 MiB)** | **−43.7 KB (−0.7%)**; **−87.4%** vs `B1` |
| **Single 28 KB Rust File (`scratch/perf/core.rs`)** | 9.36 s real / 9.16 s user | **0.01 s real / 0.01 s user** | **0.01 s real / 0.00 s user** | **~900× faster** than `B1` |
| **Rust Repo Corpus (`src/` + `tests/`: 87 files, 1.25 MB)** | > 10 s (timed out) | **0.08 s real / 0.31 s user** (18.3 MB RSS) | **0.08 s real / 0.30 s user** (18.1 MB RSS) | **−3.2% user CPU** |
| **Mixed 40-File Pinned Corpus (20 Py + 20 Rs, 700 KB)** | > 10 s (timed out) | **0.04 s real / 0.23 s user** (17.1 MB RSS) | **0.04 s real / 0.19 s user** (17.3 MB RSS) | **−17.4% user CPU** |
| **Python 20-File Pinned Corpus (443 KB)** | > 10 s (timed out) | **0.04 s real / 0.14 s user** (15.7 MB RSS) | **0.04 s real / 0.11 s user** (15.8 MB RSS) | **−21.4% user CPU** |
| **Large Python Corpus (`scratch/pycorpus`: 150 files, 8.02 MB)** | N/A | **3.82 s real / 24.81 s user** (125.6 MB RSS) | **3.49 s real / 20.86 s user** (125.5 MB RSS) | **−15.9% user CPU (−3.95 CPU-s)** |
