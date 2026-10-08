# Performance & Benchmarking: 09 Impact Breakdown Across Milestones

> [!NOTE]
> **Measured 2026-10-08** on the SOTA corpora from [fetch_corpora.sh](../../../benches/fetch_corpora.sh).
> Supersedes the synthetic `pycorpus` row of the one-time comparison in
> [04_execution_log.md](04_execution_log.md) §7.

## 1. Stages

| Stage | Revision | Change |
|---|---|---|
| 1 | `ytmwoktu` | ast-grep / tree-sitter baseline (28 grammars) |
| 2 | `yvmzyprn` | Dedicated ASTs (`ruff_python_parser`, `ra_ap_syntax`) |
| 3 | `kwkyuqvs` | Semantic index (`ImportMap`, recorded call/binding context) |
| 4 | `qzpkuzqp` | Unreadable files reported instead of aborting the run |
| 5 | `syzwvnto` | O1: statement range pruning in `find_expr_at_span` |
| 6 | `vkovvrxv` | O2: buffered plain-text output |

Stages 4–6 are each measured on top of the previous one (variants built from the same tree with
the later patches reverted).

## 2. Method and Noise Controls

Machine: 16-vCPU cloud VM (8 cores × 2 SMT, Xeon @ 2.0 GHz, no `cpufreq` control), shared with
background services (load average 6–12 during the runs). Runner: an interleaved, paired script
(kept in the session scratch directory).

- **Interleaving**: 15 rounds (+1 discarded warm-up); each round runs every stage once, in a
  fresh random order, so drift in background load hits all stages alike.
- **Pairing**: stage-to-stage ratios are computed within each round, then summarized by their
  median with a bootstrap 95% confidence interval.
- **A/A control**: the final binary runs twice per round under two names. Its ratio is the
  measured noise floor: **CPU ±1.5%, wall ±10%**.
- **CPU time** (`user + sys`, from `wait4` rusage) is reported next to wall time.
- **Same output**: one captured run per stage; diagnostic counts are listed per stage.
- **CPython**: a hard-linked copy without its 3 non-UTF-8 files (`cpython-utf8`), because
  stages 1–3 abort on them.

Reading the tables: a ratio whose interval excludes 1.00 *and* exceeds the A/A floor is a real
effect.

## 3. Results

### CPython 3.14.8 (Python, 2,154 files)

| Stage | Binary | Diagnostics | Wall median (IQR) | CPU median | Wall vs prev [95% CI] | CPU vs prev [95% CI] |
|---|---|---|---|---|---|---|
| 1 ast-grep | 44.5 MiB | DNF | > 120 s | — | — | — |
| 2 dedicated AST | 5.7 MiB | 86,985 | 2.73 s (2.08–3.09) | 10.62 s | — | — |
| 3 semantic index | 5.6 MiB | 86,981 | 2.68 s (2.27–2.93) | 8.66 s | 0.96 [0.91, 0.99] | **0.81** [0.80, 0.82] |
| 4 unreadable fix | 5.6 MiB | 86,981 | 2.49 s (2.40–2.81) | 8.63 s | 1.00 [0.93, 1.07] | 1.00 [0.99, 1.01] |
| 5 O1 | 5.6 MiB | 86,981 | 2.46 s (2.09–2.82) | 8.46 s | 0.99 [0.94, 1.05] | **0.97** [0.97, 1.00] |
| 6 O2 | 5.6 MiB | 86,981 | 1.29 s (1.14–1.53) | 7.26 s | **0.52** [0.51, 0.54] | **0.86** [0.86, 0.88] |
| A/A | — | — | 1.33 s | 7.25 s | 1.00 [0.97, 1.06] | 1.00 [1.00, 1.01] |

### cargo 0.100.0 (Rust, 38 MB checkout)

| Stage | Binary | Diagnostics | Wall median (IQR) | CPU median | Wall vs prev [95% CI] | CPU vs prev [95% CI] |
|---|---|---|---|---|---|---|
| 1 ast-grep | 44.5 MiB | DNF | > 120 s | — | — | — |
| 2 dedicated AST | 5.7 MiB | 19,315 | 0.84 s (0.77–0.95) | 3.49 s | — | — |
| 3 semantic index | 5.6 MiB | 19,315 | 0.85 s (0.74–0.93) | 3.28 s | 0.97 [0.85, 1.14] | **0.93** [0.92, 0.94] |
| 4 unreadable fix | 5.6 MiB | 19,315 | 0.81 s (0.76–0.89) | 3.26 s | 1.01 [0.92, 1.09] | 1.00 [1.00, 1.01] |
| 5 O1 | 5.6 MiB | 19,315 | 0.86 s (0.70–0.93) | 3.23 s | 1.05 [0.84, 1.16] | 1.00 [0.98, 1.01] |
| 6 O2 | 5.6 MiB | 19,315 | 0.56 s (0.47–0.67) | 2.97 s | **0.69** [0.62, 0.75] | **0.92** [0.91, 0.93] |
| A/A | — | — | 0.65 s | 3.02 s | 1.10 [1.06, 1.32] | 1.01 [1.01, 1.03] |

### ripgrep 15.2.0 (Rust, 4.6 MB)

| Stage | Binary | Diagnostics | Wall median (IQR) | CPU median | Wall vs prev [95% CI] | CPU vs prev [95% CI] |
|---|---|---|---|---|---|---|
| 1 ast-grep | 44.5 MiB | DNF | > 120 s | — | — | — |
| 2 dedicated AST | 5.7 MiB | 1,541 | 0.200 s (0.173–0.220) | 0.584 s | — | — |
| 3 semantic index | 5.6 MiB | 1,541 | 0.184 s (0.160–0.210) | 0.536 s | **0.91** [0.89, 0.94] | **0.92** [0.89, 0.93] |
| 4 unreadable fix | 5.6 MiB | 1,541 | 0.184 s (0.160–0.197) | 0.534 s | 1.02 [0.90, 1.12] | 1.00 [0.99, 1.02] |
| 5 O1 | 5.6 MiB | 1,541 | 0.181 s (0.172–0.201) | 0.533 s | 1.02 [0.97, 1.08] | 0.99 [0.98, 1.00] |
| 6 O2 | 5.6 MiB | 1,541 | 0.173 s (0.148–0.186) | 0.514 s | 0.96 [0.81, 1.07] | **0.96** [0.96, 0.98] |
| A/A | — | — | 0.166 s | 0.509 s | 0.96 [0.86, 1.08] | 0.99 [0.97, 1.00] |

Max RSS is unchanged across stages 2–6 (CPython 221 MiB, cargo 89 MiB, ripgrep 26 MiB).

## 4. Interpretation

| Change | Effect | Where it shows |
|---|---|---|
| ast-grep → dedicated ASTs | **Unusable → usable**: stage 1 does not finish any corpus in 120 s; one 797-line ripgrep file takes 13.0 s (stage 6: 0.01 s). Binary 44.5 → 5.7 MiB. | Everything |
| Semantic index | **CPU −19%** on CPython, −7% on cargo, −8% on ripgrep, while adding import resolution. On CPython it reports 6 fewer diagnostics (5 `unstructured-task` on calls to `asyncio/tasks.py`'s own `ensure_future`, 1 `mock-in-tests` on a locally defined `Mock`) and 2 more (`environment-variable-in-function` through the `import os as _os` alias). Wall barely moves because output was the serial bottleneck. | CPU |
| Unreadable-file fix | Neutral (CPU 1.00). | Robustness only |
| O1 | **CPU −3%** on CPython (just beyond the A/A floor); neutral on Rust, as expected. Large only on annotation-heavy files (−75% on the pinned `pydantic/types.py` rule family). | Annotation-heavy Python |
| O2 | **Wall −48%** on CPython, **−31%** on cargo; CPU −14% / −8% (fewer syscalls). Neutral when output is small. | Many diagnostics |

**Cumulative, stage 2 → 6 on CPython: wall 2.73 s → 1.29 s (2.1×), CPU 10.62 s → 7.26 s (−32%).**

## 5. Noise: SOTA Methods and What Applies Here

| Method | Used by | Noise | Status here |
|---|---|---|---|
| Instruction counts via hardware counters (`perf stat -e instructions:u`) | rustc-perf (primary metric) | < 1% | `perf` not installed; PMU exposure in this VM unverified |
| Simulated instruction/cache counts (Valgrind Cachegrind / Callgrind) | CodSpeed (used by Ruff, Oxc, Biome), `iai-callgrind`/Gungraun | ~0.1%, machine-independent | Valgrind 3.27.1 available via linuxbrew, not installed |
| Machine tuning (fixed frequency, turbo off, isolated cores, ASLR off) | `pyperf system tune`, rustc-perf runners | Removes drift | Not possible: VM without `cpufreq`; root required |
| Interleaved, paired runs + A/A control + bootstrap CIs | Benchmarking literature (e.g. Kalibera & Jones), rustc-perf noise detection | Separates signal from load drift | **Applied** (this document) |
| CPU time instead of wall time for compute changes | hyperfine reports both | ~1.5% vs ~10% here | **Applied** |

Wall time stays noisy on this shared VM whatever the statistics; the reliable lever is counting
instructions instead of time. Recommended next step: Callgrind instruction counts for the
`omni_bench` micro-benchmarks and for single-threaded end-to-end runs, which is what CodSpeed
does in CI for Ruff and Oxc.
