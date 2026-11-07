# Phase 2 — Ecosystem References & Prototype Benchmarks

Ecosystem survey, codebase audit, and head-to-head prototype benchmarks comparing a thin PyO3 `abi3-py310` extension wheel against a `wasm32-wasip1` CLI under `node:wasi` ([01_understand.md](01_understand.md), [01b_macos_santa_probe.md](01b_macos_santa_probe.md)).

## TL;DR

- **Ship PyPI wheels as a thin PyO3 `cp310-abi3` extension**: It runs out of the box under macOS Santa `Lockdown` in ~19–32 ms median, keeps 100% of CLI and hook logic in Rust (`0` `unsafe` blocks), preserves 16-core `rayon` throughput, and packs into a single 3.3 MiB wheel per platform covering CPython `3.10`–`3.15+`.
- **Reject the `santactl` + `wasm32-wasip1` (`node:wasi`) fallback**: `node:wasi` cold-starts at ~220–310 ms (violating the 50 ms $p_{50}$ / 150 ms $p_{99}$ hook budget because V8 recompiles WASM per process), runs single-threaded (~8× slower on full-repo lints), fails on external symlinked paths, and requires an undeclared `node >= 22` runtime.
- **Gate `brush-parser` off `wasm32-unknown-unknown`**: Eliminates all 5 `wasm-bindgen` host imports and shrinks `omnicoach_playground.wasm` by ~380 KB (`-7.7%`) while keeping `COMMAND_RULES` and `Topic::JJ` registered in the playground catalog.

---

## 1. Ecosystem State of the Art

### 1.1 Rust-Backed Python Packages & CLI Tools

| Project | Maturin Binding | Wheel Payload | CLI Dispatch Pattern | Santa `Lockdown` Behavior |
| :--- | :--- | :--- | :--- | :--- |
| **`ruff`**, **`uv`**, **`ty`** | `bindings = "bin"` | Standalone Rust executable at `bin/<tool>` | Direct `execve` on the compiled binary | **Blocked** unless the binary SHA-256 is voted or signed with an allowlisted Apple Developer ID `TeamID` ([01b_macos_santa_probe.md](01b_macos_santa_probe.md)) |
| **`tach`** | `bindings = "pyo3"` (`abi3-py38`) | Native `.abi3.so` + Python `[project.scripts]` | Python parses `argparse` and calls multiple PyO3 functions; installs custom `ctrlc` handler | **Allowed** (`execve` runs allowlisted Python; `dlopen` loads `.abi3.so`), but duplicates CLI parsing in Python |
| **`watchfiles`** | `bindings = "pyo3"` (`abi3-py39`) | `_rust_notify.abi3.so` + Python CLI | Releases the GIL via `py.detach` during Rust work, then calls `py.check_signals()?` | **Allowed**; clean CPython `KeyboardInterrupt` propagation without overriding `SIGINT` handlers |
| **`pydantic-core`** | `bindings = "pyo3"` (`abi3-py39`) | `_pydantic_core.*.so` | Pure extension module (`dlopen`) | **Allowed** (`Rule: None` verified via `ctypes.CDLL` in [01b_macos_santa_probe.md](01b_macos_santa_probe.md)) |
| **`ast-grep-py`** | `bindings = "pyo3"` (`abi3-py38`) + separate `ast-grep-cli` (`bin`) | Split into two PyPI packages | Library wheel uses `abi3`; CLI wheel uses `bindings = "bin"` | `ast-grep-py` loads under `Lockdown`; `ast-grep-cli` hits the same `execve` block |

**Takeaways for `omnicoach`**:

1. **Single-implementation CLI over `bindings = "pyo3"`**: Forwarding `sys.argv` from a minimal Python `[project.scripts]` entrypoint into `omnicoach`'s Rust `clap` parser keeps all CLI, config, VCS, and hook logic in one Rust implementation.
2. **`pyo3 = "0.30.1"` (`abi3-py310`) covers CPython 3.15**: `pyo3` `0.28`–`0.29` cap `MAXIMUM_SUPPORTED_VERSION` at `3.14`, whereas `pyo3 = "0.30.1"` with `maturin >= 1.15,<2.0` supports CPython `3.10`–`3.15` from one `cp310-abi3` wheel per OS/architecture.
3. **Linker-signed ad-hoc Mach-O without `/usr/bin/codesign`**: Single-architecture `rustc` builds on macOS emit `MH_DYLIB` binaries signed in-process by Apple `ld` (`CS_ADHOC | CS_LINKER_SIGNED`), and `cctools` `strip -x` preserves that signature via `libcodedirectory` without spawning `/usr/bin/codesign`. `maturin >= 1.13.0` also skips `/usr/bin/codesign` when no external `.dylib` is bundled.
4. **In-process CPython hygiene**:
   - Calling `std::process::exit` inside a `#[pyfunction]` skips Python shutdown; the Rust CLI must use `clap::Parser::try_parse_from` and return a `u8` exit code.
   - `py.detach(|| ...)` releases the GIL so `rayon` uses all cores, and `py.check_signals()?` converts `Ctrl-C` into Python's native `KeyboardInterrupt` without mutating process-wide signal handlers.
   - Every stdout write path must treat `io::ErrorKind::BrokenPipe` as a normal early close rather than panicking or exiting `2`.

### 1.2 Rust-to-WebAssembly Linter Architectures

| Project | WASM Target | ABI / Binding Layer | Scope |
| :--- | :--- | :--- | :--- |
| **`dprint` plugins** | `wasm32-unknown-unknown` | 6-function linear-memory C ABI (`0` host imports) | Stateless single-file formatting; host owns filesystem and config |
| **`ruff_wasm`**, **`biome_wasm`** | `wasm32-unknown-unknown` | `wasm-bindgen` + generated per-build JS glue | Browser playgrounds; require `wasm-bindgen-cli` JS wrapper |
| **Node WASI CLIs (`wasm32-wasip1`)** | `wasm32-wasip1` | `wasi_snapshot_preview1` (`node:wasi` / `wasmtime`) | Sandboxed single-threaded filesystem access; no child process spawning |

**Takeaways for `omnicoach`**:

1. **Zero-import ABI for `omnicoach-playground`**: [playground/src/lib.rs](../../../playground/src/lib.rs) and [playground/www/omnicoach.js](../../../playground/www/omnicoach.js) use a 3-export linear-memory ABI (`prepare_input`, `execute_request`, `output_pointer`) instantiated with `{}` so one static JS loader serves `main` and every released `wasm/<tag>/omnicoach_playground.wasm`.
2. **Structural limits of `wasm32-wasip1` + `node:wasi`**:
   - **No persistent WASM compile cache in Node CLI runs**: `NODE_COMPILE_CACHE` caches JS bytecode only, and V8 rejects `v8.serialize(wasmModule)` with `DataCloneError`, forcing ~5.5 MiB of WASM recompilation on every cold hook call.
   - **Single-threaded execution**: `rayon` runs on one thread on `wasm32-wasip1`.
   - **Subprocess and path gaps**: WASI Preview 1 lacks subprocess spawning (requiring custom `unsafe extern "C"` host imports for `jj`/`git`), `std::fs::canonicalize` returns `Unsupported`, and `std::env::current_dir()` returns `"/"` unless remapped by the host.

---

## 2. Codebase Audit

1. **CLI coupling in [src/bin/omnicoach.rs](../../../src/bin/omnicoach.rs)**: Extracting a `Cli` component in `src/cli.rs` lets the native binary and `crates/omnicoach-py` share one implementation while keeping [src/lib.rs](../../../src/lib.rs) a pure module router per [ADR 006](../decisions/006_architectural_dag_and_conformance.md).
2. **`BrokenPipe` gap in [src/diagnostic.rs](../../../src/diagnostic.rs)**: `print_diagnostics` used `println!` for JSON (panicking when piped to `head`) and propagated `BrokenPipe` as `Err` for plain output (exiting `2`). Both paths must write to `io::stdout().lock()` and ignore `BrokenPipe`.
3. **Transitive `wasm-bindgen` leak & `Topic::JJ` taxonomy constraint**: `omnicoach_playground.wasm` on `main` carried 5 `__wbindgen_*` imports originating from `brush-parser 0.4.0` (`cached` $\to$ `web-time`, `getrandom`, `uuid` in [Cargo.lock](../../../Cargo.lock)). Gating `COMMAND_RULES` out of `REGISTERED_RULES` would drop `Topic::JJ` from [src/rule_selection/taxonomy.rs](../../../src/rule_selection/taxonomy.rs) and break `"jj"` selectors in the playground; gating only `brush-parser` behind `cfg(not(all(target_arch = "wasm32", target_os = "unknown")))` removes all 5 imports while preserving every rule and topic.
4. **Local `wasm32` `rustflags` ([.cargo/config.toml](../../../.cargo/config.toml))**: Moving `--cfg no_salsa_async_drops` from workflow `RUSTFLAGS` into `.cargo/config.toml` under `[target.'cfg(target_arch = "wasm32")']` prevents `ra_ap_syntax` drop-thread panics in local WASM builds too.
5. **Workspace versioning ([releasing.md](../releasing.md))**: Sharing `version` via `[workspace.package]` in root [Cargo.toml](../../../Cargo.toml) keeps crates.io, PyPI wheels, and GitHub Releases synchronized from one version bump.

---

## 3. Empirical Prototype Benchmarks

All candidates were prototyped in a disposable `jj` revision (abandoned after measurement) and benchmarked on a 16-core Linux `x86_64` workstation (`rustc` stable, `lto = "fat"`, `strip = "symbols"`, Node 26, system CPython 3.13, Linuxbrew CPython 3.14) alongside the macOS Santa `Lockdown` measurements in [01b_macos_santa_probe.md](01b_macos_santa_probe.md):

- **Native binary**: Standalone Rust release executable (`omnicoach`).
- **PyO3 `abi3` wheel**: Single `cp310-abi3` wheel installed into separate `uv` virtual environments on system Python 3.13 and Linuxbrew Python 3.14 without recompilation (`0` `unsafe` blocks).
- **Direct `node:wasi`**: Full Rust CLI compiled to `wasm32-wasip1` (`omnicoach.wasm`) with a `jj`/`git` subprocess host import, run via `node --no-warnings`.
- **Python $\to$ `node:wasi`**: Python console script delegating via `os.execvp("node", ...)` to the `wasm32-wasip1` module.

### 3.1 Artifact & Wheel Sizes

**Context**: Release builds (`lto = "fat"`, `strip = "symbols"`); wheel sizes reflect standard PEP 427 `ZIP_DEFLATED` archives.

| Artifact | Uncompressed | Compressed `.whl` | WASM Imports | Budget ($\le 5\text{ MB}$) |
| :--- | ---: | ---: | :--- | :--- |
| Native Rust binary (`omnicoach`) | 7.09 MiB | ~3.45 MiB (`bindings = "bin"`) | N/A | Baseline |
| **PyO3 `cp310-abi3` wheel** (`_omnicoach.abi3.so`) | **7.11 MiB** | **3.31 MiB** | N/A | **Pass** (1 wheel covers Python $\ge 3.10$) |
| `wasm32-wasip1` CLI (`omnicoach.wasm`) | 5.46 MiB | 1.98 MiB | 22 WASI + 1 host | Pass (requires external WASI runtime) |
| Playground WASM — *before fix* (`main`) | 4.72 MiB | N/A | **5 `__wbindgen_*` (broken)** | Fails zero-import contract |
| **Playground WASM — *after `brush-parser` target gate*** | **4.36 MiB** | N/A | **`0` imports** | **Pass (`-380 KB` / `-7.7%`)** |

**Analysis & Interpretation**:

- Switching from `bindings = "bin"` to `bindings = "pyo3"` (`abi3-py310`) adds ~14 KB (`+0.2%`) uncompressed and produces a **3.31 MiB** compressed wheel—well under the 5 MB budget.
- Gating `brush-parser` off `wasm32-unknown-unknown` removes all 5 imports and shrinks `omnicoach_playground.wasm` by ~380 KB while passing end-to-end Python, Rust, `jj`-config, `explain`, and `catalog` checks in [playground/www/omnicoach.js](../../../playground/www/omnicoach.js).

### 3.2 Functional Parity & Prototyping Defect Log

**Context**: All candidates ran against 11 CLI and hook scenarios (`--version`, `--list-rules`, `--list-tags`, `--explain`, `code` clean, `code --diff-rev`, `command --cmd "jj edit @-"`, `graph`, `hook antigravity pre-tool-use` on `write_to_file` and `run_command`, and unknown-flag error) plus closed-pipe `--list-rules | head -c 64`.

| Verification Dimension | Native Binary | PyO3 `abi3` Wheel (Py 3.13 & 3.14) | WASI CLI (`node:wasi` & Python $\to$ `node:wasi`) |
| :--- | :---: | :---: | :---: |
| Exact `stdout`, `stderr`, and exit-code parity | **All 11 cases** | **All 11 cases** | **All 11 cases** *(after 2 WASI workarounds; still failed `rc=2` on external symlinked corpus)* |
| `BrokenPipe` (`--list-rules \| head -c 64`) | `exit=0`, 0 stderr | `exit=0`, 0 stderr | `exit=0`, 0 stderr |
| Runtime prerequisites outside wheel | None | None (uses active Python) | Undeclared external `node >= 22` |
| Custom `unsafe` FFI code in `omnicoach` | `0` blocks | **`0` blocks** | `unsafe extern "C"` + linear-memory allocator |

**Defects Caught During Prototyping**:

1. **TTY-aware `clap` error rendering**: `clap_error.render().ansi()` forced ANSI codes even when `stderr` was piped. `clap_error.print()` preserves `anstream` terminal detection, matched the native binary byte-for-byte, and allowed closed-pipe `--help`/`--version` to exit `0`.
2. **`ra_ap_syntax` thread-spawn panic on WASM**: Without `--cfg no_salsa_async_drops`, `ra_ap_syntax` panicked spawning a drop thread when parsing Rust on `wasm32-wasip1` and local `wasm32-unknown-unknown` builds; `.cargo/config.toml` fixed both.
3. **WASI working-directory and symlink friction**: Under `node:wasi`, `std::env::current_dir()` returned `"/"` (breaking `jj diff` until remapped in JS) and external symlinked benchmark directories failed with `rc=2`.

### 3.3 Cold-Start Latency & Multi-Core Throughput

**Context**: 2 warmup runs followed by `N = 25` timed invocations for CLI/hook commands and `N = 7` (`N = 5` on `tach`) for full-repository `omnicoach code`. Lower is better.

| Workload | Native Binary (`p50` / `p95`) | PyO3 `abi3` Wheel, Sys Py 3.13 (`p50` / `p95`) | PyO3 `abi3` Wheel, Brew Py 3.14 (`p50` / `p95`) | Direct `node:wasi` (`p50` / `p95`) | Python 3.13 $\to$ `node:wasi` (`p50` / `p95`) |
| :--- | ---: | ---: | ---: | ---: | ---: |
| `omnicoach --version` (`N=25`) | `2.75` / `3.49 ms` | **`26.45` / `29.18 ms`** | `47.69` / `55.07 ms` | `249.15` / `320.97 ms` | `307.42` / `634.60 ms` |
| `omnicoach --explain single-letter-name` (`N=25`) | `4.29` / `5.71 ms` | **`33.11` / `36.17 ms`** | `58.44` / `74.27 ms` | `276.20` / `472.59 ms` | `318.52` / `472.48 ms` |
| `hook antigravity pre-tool-use` (`write_to_file`, `N=25`) | `3.08` / `3.95 ms` | **`32.17` / `34.44 ms`** | `56.82` / `104.69 ms` | `222.72` / `273.31 ms` | `248.44` / `326.90 ms` |
| `hook antigravity pre-tool-use` (`run_command`, `N=25`) | `2.49` / `3.10 ms` | **`26.25` / `28.75 ms`** | `46.18` / `50.72 ms` | `232.48` / `295.13 ms` | `271.70` / `460.42 ms` |
| `omnicoach code` (workspace, 16 cores, `N=7`) | `132.70` / `143.71 ms` | **`189.39` / `200.48 ms`** | `218.60` / `328.02 ms` | `1,536.35` / `2,252.09 ms` | `1,740.77` / `2,230.20 ms` |
| `omnicoach code` (`tach` Py+Rs corpus, `N=5`) | `69.07` / `75.20 ms` | **`84.96` / `93.08 ms`** | `109.96` / `119.75 ms` | *Failed (`rc=2`)* | *Failed (`rc=2`)* |

**Analysis & Interpretation**:

- **Hook latency ($\le 50\text{ ms}$ $p_{50}$, $\le 150\text{ ms}$ $p_{99}$)**: The `abi3` wheel runs in ~26–32 ms median on system Python 3.13 and ~19 ms median on the target Santa Mac (`uv` Python 3.15). Both `node:wasi` arms miss the budget on every call (~220–310 ms median, ~7×–9× slower) due to per-process WASM compilation.
- **Multi-core throughput**: With `py.detach`, the `abi3` wheel adds only Python's fixed ~25 ms startup over 16-core `rayon` (`189 ms` vs `133 ms` native), whereas single-threaded WASI takes ~1.5 s (~8× slower).

---

## 4. Conclusions & Next Steps

The benchmarks confirm the problem diagnosis in [00_user_report.md](00_user_report.md) while inverting its proposed architecture:

1. **Adopt the PyO3 `cp310-abi3` wheel (`crates/omnicoach-py`) as the PyPI distribution**: It runs under Santa `Lockdown` without `codesign` or SHA-256 voting, keeps all CLI logic in safe Rust, and meets both hook-latency and multi-core throughput budgets.
2. **Reject the `santactl` + `node:wasi` launcher**: Querying `santactl` alone (~33 ms) is slower than running the `abi3` wheel (~19 ms). Falling back to `node:wasi` misses the hook latency budget by ~7×–9×, runs single-threaded (~8× slower), requires `unsafe` host FFI and external `node >= 22`, and doubles wheel payload size.
3. **Publish prebuilt standalone native binaries on GitHub Releases**: Pair the PyPI extension wheel with standalone `musl` / macOS / Windows archives (`cargo-binstall` compatible) for unconstrained hosts wanting ~3 ms startup.
4. **Gate `brush-parser` off `wasm32-unknown-unknown` and defer standalone WASI**: Restore `omnicoach_playground.wasm` to zero imports with CI enforcement, and track standalone `wasm32-wasip1` on [ROADMAP.md](../../../ROADMAP.md) §4 for non-Python sandboxes.
