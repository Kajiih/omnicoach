# Phase 4 — Execution Log: Universal `abi3` Wheel, Standalone Release Binaries & Zero-Import Playground WASM

## 1. Verification Summary

Execution of the architecture specified in [03_design.md](./03_design.md) and summarized in [ADR 014](../decisions/014_distribution_and_wasm_targets.md).

### Quantitative Outcomes Against Target Budgets

| Metric | Target ([03_design.md](./03_design.md)) | Measured Result |
| :--- | :--- | :--- |
| Standalone binary cold latency (`p50`) | $\le 5\text{ ms}$ | `1.66 ms` (`x86_64-unknown-linux-musl` static release binary) |
| Compressed `cp310-abi3` `.whl` size | $\le 5\text{ MiB}$ | `3.57 MiB` |
| Multi-interpreter `abi3` reuse & `sdist` install | CPython `3.10`–`3.15+` + clean `sdist` build | Verified single wheel across CPython `3.10` and `3.13` + `MATURIN_PEP517_ARGS="--locked"` `sdist` build |
| CLI/hook parity & `BrokenPipe` exit | `100%` match; empty `stderr` | All 16 golden CLI cases + wheel-vs-native CLI & Antigravity hook parity + deterministic `BrokenPipe` |
| `omnicoach_playground.wasm` import count | `0` imports | `0` imports (`4.56 MiB`, `80` code rules) |
| Workspace `unsafe` exemptions added | `0` | `0` |

### Quality Gates Status: **ALL GREEN**

- `cargo fmt --check`: Clean (`0` diffs).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: Clean (`0` warnings).
- `cargo clippy -p omnicoach-playground --target wasm32-unknown-unknown --locked -- -D warnings`: Clean (`0` warnings).
- `cargo doc --workspace --no-deps --document-private-items --locked`: Clean (`0` warnings).
- `cargo test --workspace --locked`: All unit, CLI, `architecture_conformance`, `registry`, and doctest suites passing.
- `node playground/tests/verify_wasm.mjs`: `0` WebAssembly imports, end-to-end Python, Rust, `jj`-config, `explain`, and `catalog` verified.
- `scripts/verify_wheel.py`: Passed locally on CPython `3.10` and `3.13` (CLI cases with native parity, `python -m` parity, Antigravity hook round-trips, closed stdout pipe, `--locked` `sdist` build); CI also runs it on macOS and Windows.
- `omnicoach code src crates scripts tests playground/src`: **All checks passed!**
- `zizmor .github/workflows`: `0` findings.
- `cargo +nightly llvm-cov --lib --branch`: `93.45%` line coverage, `94.74%` function coverage, `78.26%` branch coverage.

---

## 2. Landed Commit Chain

1. **`ci: migrate retired macos-14 runners to macos-15`**: [ci.yml](../../../.github/workflows/ci.yml) and [release.yml](../../../.github/workflows/release.yml).
2. **`docs(wheel-and-wasm): record investigation probe, benchmarks, and design`**: [00_user_report.md](./00_user_report.md) through [03_design.md](./03_design.md).
3. **`fix(cli): exit cleanly when stdout is closed early`**:
   - [src/diagnostic.rs](../../../src/diagnostic.rs) and [src/bin/omnicoach.rs](../../../src/bin/omnicoach.rs): diagnostics, discovery output, hook responses, and footers treat `io::ErrorKind::BrokenPipe` as a normal early close instead of panicking or exiting `2`.
   - [tests/cli.rs](../../../tests/cli.rs): integration tests spawn the binary with stdout connected to an already-closed OS pipe (`std::io::pipe()`).
4. **`refactor(cli): extract Cli component with in-process entrypoint`**:
   - [src/cli.rs](../../../src/cli.rs), [src/architecture.rs](../../../src/architecture.rs), [src/lib.rs](../../../src/lib.rs): `run_from_args` parses with `CliArgs::try_parse_from` (`bin_name = "omnicoach"`) and returns exit codes instead of calling `std::process::exit`; [src/bin/omnicoach.rs](../../../src/bin/omnicoach.rs) becomes a one-line `main`.
   - Verification: byte-for-byte match across all 16 pre-refactor CLI invocations; `0` missed mutants in `cargo mutants` on [src/diagnostic.rs](../../../src/diagnostic.rs) and [src/cli.rs](../../../src/cli.rs).
5. **`fix(playground): eliminate wasm-bindgen imports on wasm32-unknown-unknown`**:
   - [Cargo.toml](../../../Cargo.toml), [shell.rs](../../../src/command_lint/command/shell.rs), [shell/brush_backend.rs](../../../src/command_lint/command/shell/brush_backend.rs): `brush-parser` is gated off `wasm32-unknown-unknown` in the child module paired with a browser stub, while `COMMAND_RULES` and `Topic::JJ` stay in `REGISTERED_RULES`.
   - [.cargo/config.toml](../../../.cargo/config.toml): `--cfg no_salsa_async_drops` for all `wasm32` targets, replacing workflow `RUSTFLAGS` overrides.
   - [verify_wasm.mjs](../../../playground/tests/verify_wasm.mjs), [ci.yml](../../../.github/workflows/ci.yml), [pages.yml](../../../.github/workflows/pages.yml), [release.yml](../../../.github/workflows/release.yml): zero imports and end-to-end execution through [omnicoach.js](../../../playground/www/omnicoach.js) enforced in CI, Pages, and Release.
6. **`feat(py): package PyPI wheel as thin PyO3 abi3-py310 extension`**:
   - [Cargo.toml](../../../Cargo.toml) & [crates/omnicoach-py](../../../crates/omnicoach-py/Cargo.toml): shared `[workspace.package]` metadata, `pyo3 0.30.1` (`abi3-py310`) `run_cli` (`py.detach` + `py.check_signals()?`), and Python package (`__init__.py`, `__main__.py`, `_cli.py`, `_omnicoach.pyi`, `py.typed`) forwarding `argv` and mapping `KeyboardInterrupt` to `130`.
   - [pyproject.toml](../../../pyproject.toml), [verify_wheel.py](../../../scripts/verify_wheel.py), [ci.yml](../../../.github/workflows/ci.yml): `bindings = "pyo3"`, `[tool.pyrefly]`, `sdist` workspace includes, and CI wheel verification across Linux, macOS, and Windows.
7. **`ci(release): publish standalone native binaries and cargo-binstall metadata`**:
   - [Cargo.toml](../../../Cargo.toml) & [release.yml](../../../.github/workflows/release.yml): `[package.metadata.binstall]` with Linux `musl` and Windows `zip` overrides, 5-target `binaries` build matrix, all-builds publish gates, and `publish-crates` ordered after `github-release`.
8. **`docs: document dual distribution, Santa setup, and WASM roadmap`**: [README.md](../../../README.md), [releasing.md](../releasing.md), [ROADMAP.md](../../../ROADMAP.md).
9. **`docs(wheel-and-wasm): record ADR 014, execution log, and audit`**: [ADR 014](../decisions/014_distribution_and_wasm_targets.md), this log, and [05_cleanup_and_audit.md](./05_cleanup_and_audit.md).

---

## 3. Surgical Adaptations During Execution

1. **Workspace `sdist` completeness for `--locked` builds**: Installing from `sdist` with `MATURIN_PEP517_ARGS="--locked"` in [verify_wheel.py](../../../scripts/verify_wheel.py) required `benches/**` (referenced by `[[bench]]` in root [Cargo.toml](../../../Cargo.toml)) and `playground/**` (present in [Cargo.lock](../../../Cargo.lock)). Both are included in `[tool.maturin] include` (`format = "sdist"`).
2. **Pyrefly import-root resolution (`[tool.pyrefly]`)**: Because root [pyproject.toml](../../../pyproject.toml) sits beside Rust's `src/`, adding `search-path = ["crates/omnicoach-py/python"]` pointed IDE Python type checking at the Python package.
3. **`cargo-binstall` `{ archive-suffix }`**: Upstream `cargo-binstall` soft-deprecated `{ archive-format }`; `{ archive-suffix }` probes `.tgz`/`.tar.gz` on Unix and `.zip` on Windows cleanly.
4. **Linux `musl` assembly toolchain (`musl-tools`)**: Transitive dependency `psm` (`stacker`) compiles `.s` assembly via `cc-rs`, requiring `musl-tools` (`musl-gcc`) on Linux runners to produce a statically linked ELF binary.
5. **`cargo update --workspace --offline` in [releasing.md](../releasing.md)**: Refreshes both `omnicoach` and `omnicoach-py` entries in [Cargo.lock](../../../Cargo.lock) on version bumps.
