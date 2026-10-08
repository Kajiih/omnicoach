# Performance & Benchmarking: 08 Optimization Log

> [!NOTE]
> **Status: in progress (2026-10-08).** Benchmark-driven optimizations following the backlog in
> [ROADMAP.md](../../../ROADMAP.md) §2. Rule: a change that adds complexity ships only with a
> measured win; every change must keep diagnostics byte-identical.

## Method

- **Micro**: `cargo bench --bench omni_bench -- <group>` before/after. The machine is noisy
  (median up to 2× fastest), so we compare `fastest` plus the deterministic allocation counts.
- **End-to-end**: build both release binaries, check that their sorted outputs are identical
  (`cmp`), then run `hyperfine -N -w 1 -r 5 "<base> <dir>" "<candidate> <dir>"`.
- **Corpora**: `scratch/pycorpus` (150 copies of one 53 KB Python module, 8.3 MB, 423,601
  output lines; a synthetic worst case for output volume), and the repo's `src/` (Rust, 0
  diagnostics; the common case).

## O1: Range-prune statements in `find_expr_at_span` (accepted)

- **Hypothesis**: `ExprFinder` pruned by range in `visit_expr` but not in `visit_stmt`, so every
  annotation lookup walked all statements of the module.
- **Change**: 10 lines in [python.rs](../../../src/code_lint/ast/python.rs): a `visit_stmt` that
  skips statements whose range does not contain the target span. No new data or caches.

| Measurement | Before | After | Change |
|---|---|---|---|
| `family_python::C_py_signatures_unmemoized` (fastest) | 7.41 ms | 1.88 ms | −75% |
| `family_python::D_py_classes_unmemoized` (fastest) | 1.63 ms | 1.27 ms | −22% |
| `rule_python::specific-collection-parameter` (fastest) | 1.78 ms | 0.47 ms | −73% |
| `rule_python::concrete-collection-parameter` (fastest) | 1.85 ms | 0.50 ms | −73% |
| `pycorpus` user CPU | 20.70 s | 3.64 s | **−82%** |
| `pycorpus` wall | 5.05 s | 3.07 s | −39% |
| Diagnostics | 423,601 lines | identical | — |

**Consequence for the backlog**: memoizing `extract_function_signatures` / `collect_class_attributes`
is no longer justified. The signature types borrow from `ParsedFile`, so caching them on `ParsedFile`
would need a span-based mirror plus rehydration, and the remaining Group C cost is ~1.9 ms per
88 KB of Python.

## O2: Buffer plain-text diagnostic output (accepted)

- **Hypothesis**: after O1, each pycorpus file costs ~20 ms CPU, so on 16 cores the ~2 s wall time
  had to be serial. `print_diagnostics` called `println!` per diagnostic: a stdout lock and a
  line-buffered flush per line (1.2 s of sys time).
- **Change**: 4 lines in [diagnostic.rs](../../../src/diagnostic.rs): one locked
  `BufWriter<StdoutLock>`, `writeln!` with `?`, explicit `flush`. A closed pipe now returns an
  error instead of panicking inside `println!`.

| Measurement | Before (O1) | After | Change |
|---|---|---|---|
| `pycorpus` wall | 2.18 s | 0.65 s | **3.4× faster** |
| `pycorpus` sys | 1.26 s | 0.32 s | −75% |
| Repo `src/` wall (0 diagnostics) | 63.8 ms | 64.0 ms | neutral |
| Diagnostics | identical | identical | — |

Combined O1+O2 on `pycorpus`: **5.05 s → 0.65 s wall (7.8×)**.

## O3: Single-pass Rust CST extraction (measured, deferred)

- **Probe** (temporary `omni_bench` group, removed): one bare Rowan walk over the 4 Rust fixtures
  (49 KB) costs ~850 µs fastest and ~18.6k allocations with `descendants_with_tokens()`, and ~300 µs
  / ~7.6k allocations with `descendants()`.
- **Attribution**: Group E Rust rules cost 470–870 µs each (`bare-multiline-string` 785 µs,
  `repeated-literal` 869 µs, `packed-assertion` 619 µs, `nullable-collection-return` 601 µs,
  `repeated-index-access` 469 µs). Each rule is roughly **one full Rowan walk**, so walk cost
  dominates rule cost.
- **Ceiling**: fusing the walks could save roughly 3–4 ms per 49 KB of Rust in-process. End to end,
  linting the repo's `src/` takes 64 ms wall / 266 ms user, so the realistic saving is
  ~tens of ms per run.
- **Why deferred**: the walks use different styles (`descendants`, `descendants_with_tokens`,
  `preorder`, recursive `children_with_tokens`) with different filters. Fusing them means a shared
  visitor that every extractor plugs into, which is a large structural change. Caching red nodes
  on `ParsedFile` is not possible because Rowan's `SyntaxNode` is `!Send`/`!Sync`. Not worth it
  until Rust repositories large enough to make it user-visible become a target.

## O4: Suppression fast-path false trigger (not pursued)

The fallthrough on `"omni:"` inside string literals is correct, and on the repo's `src/` the whole
run is 64 ms. No measurement shows it mattering on real repositories.
