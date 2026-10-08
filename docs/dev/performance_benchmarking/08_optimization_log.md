# Performance & Benchmarking: 08 Optimization Log

> [!NOTE]
> **Status: in progress (2026-10-08).** Benchmark-driven optimizations following the backlog in
> [ROADMAP.md](../../../ROADMAP.md) §2. Rule: a change that adds complexity ships only with a
> measured win; every change must keep the output byte-identical. The figures here come from
> sequential `hyperfine` runs; [09_impact_breakdown.md](09_impact_breakdown.md) re-measures every
> stage with interleaved, paired runs and an A/A noise floor.

## Method

- **Micro**: `cargo bench --bench omni_bench -- <group>` before/after. The machine is noisy
  (median up to 2× fastest), so we compare `fastest` plus the deterministic allocation counts.
- **End-to-end**: build one release binary per variant, check that stdout and stderr are
  identical across variants (`cmp`), then run
  `hyperfine -N -w 2 -r 15 "<variant-a> <dir>" "<variant-b> <dir>" ...`.
- **Corpora** ([fetch_corpora.sh](../../../benches/fetch_corpora.sh), cloned into
  `target/bench-corpora/`, latest releases as of 2026-10-08):

  | Corpus | Source of the choice | Size | Output |
  |---|---|---|---|
  | CPython `v3.14.8` | Ruff's end-to-end benchmark | 160 MB checkout | 348,353 lines, 3 unreadable files |
  | ripgrep `15.2.0` | Clippy lintcheck crate list | 4.6 MB | 6,166 lines |
  | cargo `0.100.0` | Clippy lintcheck crate list | 38 MB | 77,287 lines |

- **Noise floor**: O1 only changes Python code, yet ripgrep (pure Rust) moved from 179 ms to
  144 ms between the "none" and "O1" variants. Differences under ~20% wall time on this machine
  are noise; CPU time (`User`) is steadier.

> [!WARNING]
> The first measurements in this log used `scratch/pycorpus`: 150 copies of a single 53 KB
> module (`pydantic/types.py`-like, annotation-heavy). It overstated O1 (−82% CPU there, −4% on
> CPython) and is no longer used.

## Prerequisite fix: unreadable files no longer abort the run

Running CPython exposed that one file that is not valid UTF-8 aborted the whole run, and which
file was reported depended on thread scheduling. `run_code_lint` now returns a `LintReport`
whose `unreadable_files` (sorted by path) are printed on stderr after the diagnostics of every
other file, with exit code 2. CPython 3.14.8 has 3 such files (two PEP 263 encoded test
modules and one intentionally invalid file). Regression test:
`test_code_lint_unreadable_file_reports_error_and_lints_the_rest` in `tests/cli.rs`.

All end-to-end variants below include this fix.

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
| CPython user CPU | 7.08 s | 6.77 s | −4% (within noise) |
| Output | — | identical | — |

The cost is quadratic in the number of annotations × statements per file, so it shows on
annotation-heavy files (the pinned `pydantic/types.py` fixture) and barely on CPython, whose
standard library is mostly unannotated. Kept because it removes a quadratic worst case with
no added complexity.

**Consequence for the backlog**: memoizing `extract_function_signatures` / `collect_class_attributes`
is not justified. The signature types borrow from `ParsedFile`, so caching them on `ParsedFile`
would need a span-based mirror plus rehydration, and the remaining Group C cost is ~1.9 ms per
88 KB of Python.

## O2: Buffer plain-text diagnostic output (accepted)

- **Hypothesis**: per-file work runs on 16 cores, but `print_diagnostics` called `println!` per
  diagnostic: a stdout lock and a line-buffered flush per line, all serial.
- **Change**: 4 lines in [diagnostic.rs](../../../src/diagnostic.rs): one locked
  `BufWriter<StdoutLock>`, `writeln!` with `?`, explicit `flush`. A closed pipe now returns an
  error instead of panicking inside `println!`.

| Corpus | Wall before (O1) | Wall after (O1+O2) | System time |
|---|---|---|---|
| CPython | 1.96 s | **1.43 s** (−27%) | 1.18 s → 0.61 s |
| cargo | 752 ms | **482 ms** (−36%) | 539 ms → 353 ms |
| ripgrep | 144 ms | 128 ms (noise) | 88 ms → 72 ms |
| Repo `src/` (0 diagnostics) | 63.8 ms | 64.0 ms | neutral |

Real projects linted without a tuned configuration produce tens of thousands of diagnostics, so
output cost matters in practice, not only on synthetic inputs.

## O3: Single-pass Rust CST extraction (measured, deferred)

- **Probe** (temporary `omni_bench` group, removed): one bare Rowan walk over the 4 Rust fixtures
  (49 KB) costs ~850 µs fastest and ~18.6k allocations with `descendants_with_tokens()`, and ~300 µs
  / ~7.6k allocations with `descendants()`.
- **Attribution**: Group E Rust rules cost 470–870 µs each (`bare-multiline-string` 785 µs,
  `repeated-literal` 869 µs, `packed-assertion` 619 µs, `nullable-collection-return` 601 µs,
  `repeated-index-access` 469 µs). Each rule is roughly **one full Rowan walk**, so walk cost
  dominates rule cost.
- **Ceiling**: fusing the walks could save roughly 3–4 ms per 49 KB of Rust in-process. End to
  end, cargo (38 MB checkout) takes 2.6 s of CPU and 0.48 s wall on 16 cores, so the saving is a
  fraction of that.
- **Why deferred**: the walks use different styles (`descendants`, `descendants_with_tokens`,
  `preorder`, recursive `children_with_tokens`) with different filters. Fusing them means a shared
  visitor that every extractor plugs into, which is a large structural change. Caching red nodes
  on `ParsedFile` is not possible because Rowan's `SyntaxNode` is `!Send`/`!Sync`. Not worth it
  until Rust repositories large enough to make it user-visible become a target.

## O4: Suppression fast-path false trigger (not pursued)

The fallthrough on `"omni:"` inside string literals is correct, and no measurement shows it
mattering on real repositories.

## Next lead

After O2, CPython still spends ~1.4 s wall for ~7.3 s of CPU on 16 cores (ideal ≈ 0.5 s), so
~0.9 s remains serial or poorly parallel. Candidates to measure: single-threaded directory
discovery (`ignore::WalkBuilder::build()`, see the roadmap item), the final sort, and per-line
`format!` in `print_diagnostics`.
