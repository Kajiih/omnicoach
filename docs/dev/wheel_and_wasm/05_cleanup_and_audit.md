# Phase 5 & 6 — Clean Up, Multi-Reviewer Audit & Triage Report

## 1. Overview

Three independent read-only review passes audited the commit chain along technology boundaries (Rust CLI & shell gate, Python `abi3` extension & packaging, and browser WASM, CI/Release workflows & documentation), followed by whole-chain simplicity and cold-reader reviews. Accepted findings were folded into the commits that introduced the affected behavior; full verification results are in [04_execution_log.md](./04_execution_log.md) §1.

---

## 2. Clean Up Summary

- **Prototype cleanup**: Confirmed the disposable Phase 2 `wasm32-wasip1` vs `pyo3` prototype revision left zero files, dependencies, or feature flags in the repository.
- **Deduplicated build flags**: Removed duplicate `RUSTFLAGS: --cfg no_salsa_async_drops` overrides in [pages.yml](../../../.github/workflows/pages.yml) and [release.yml](../../../.github/workflows/release.yml) once consolidated into [.cargo/config.toml](../../../.cargo/config.toml), and omitted redundant `strip = true` in [pyproject.toml](../../../pyproject.toml) (covered by workspace `[profile.release] strip = "symbols"`).

---

## 3. Multi-Reviewer Audit Triage

| Boundary | Finding Summary | Severity | Verdict | Remediation & Rationale |
| :--- | :--- | :---: | :---: | :--- |
| CI / Release Workflows | GitHub Actions retired `macos-14` runners in November 2026; macOS CI and release jobs would fail to provision. | Critical | **Fix** | Migrated `macos-14` jobs in [ci.yml](../../../.github/workflows/ci.yml) and [release.yml](../../../.github/workflows/release.yml) to `macos-15` in a dedicated base commit. |
| Release Workflow | Asymmetric `needs:` graph in [release.yml](../../../.github/workflows/release.yml) allowed partial publishes on sibling build failures and ran `publish-crates` before `github-release` uploaded `cargo-binstall` archives. | Major | **Fix** | Gated `github-release` and `publish-pypi` (`skip-existing: true`) on all 6 build jobs, made `github-release` clobber-safe on retry, and ordered `publish-crates` after `github-release`. |
| CLI & Wheel Tests | Subprocess `BrokenPipe` tests closed the reader after brief output (`< 64 KiB`), fitting inside the OS pipe buffer without triggering `EPIPE`. | Major | **Fix** | Switched to pre-closed OS pipes (`std::io::pipe()` in [tests/cli.rs](../../../tests/cli.rs); `os.pipe()` in [verify_wheel.py](../../../scripts/verify_wheel.py)) so byte 1 deterministically hits `EPIPE`. |
| Wheel Verification | Antigravity hook check in [verify_wheel.py](../../../scripts/verify_wheel.py) sent Claude Code schema keys (`tool_name`/`tool_input`), passing vacuously via fail-open parsing. | Major | **Fix** | Switched to Antigravity `{"toolCall": {"name", "args"}}` payloads and added a violating `write_to_file` `pre-tool-use` $\to$ `pre-invocation` round-trip. |
| CLI & Python Entrypoint | `python -m omnicoach --help` rendered `Usage: __main__.py` and Windows `.exe` builds risked `Usage: omnicoach.exe` drift. | Major | **Fix** | Pinned `bin_name = "omnicoach"` on `CliArgs` in [src/cli.rs](../../../src/cli.rs), normalized `argv[0]` in [_cli.py](../../../crates/omnicoach-py/python/omnicoach/_cli.py), and asserted `python -m` parity. |
| Python Type Stub | [_omnicoach.pyi](../../../crates/omnicoach-py/python/omnicoach/_omnicoach.pyi) declared `PathLike[str]`, whereas PyO3 `Vec<OsString>` raises `TypeError` on `pathlib.Path`. | Major | **Fix** | Narrowed stub signature to `def run_cli(args: Sequence[str]) -> int: ...`. |
| `sdist` Packaging | Building from `sdist` with `--locked` failed because `omnicoach-playground` was in [Cargo.lock](../../../Cargo.lock) and `benches/fixtures/**` was omitted. | Major | **Fix** | Added `benches/fixtures/**` and `playground/**` to `sdist` includes in [pyproject.toml](../../../pyproject.toml) and enforced `MATURIN_PEP517_ARGS="--locked"` in [verify_wheel.py](../../../scripts/verify_wheel.py). |
| Rust Core & Architecture | `Cli` omitted direct `Config`/`Diagnostic` edges in [src/architecture.rs](../../../src/architecture.rs); `print_diagnostics` wrapped pre-rendered strings in `BufWriter`; `run_from_args` unit tests lacked `--config` isolation. | Minor | **Fix** | Declared `Config` and `Diagnostic` on `Cli`, wrote directly to `StdoutLock<'_>`, and isolated `run_from_args` unit tests with an empty temp config. |
| Packaging, Harnesses & Docs | `.git` boundary and `PYTHONPATH` scrubbing in [verify_wheel.py](../../../scripts/verify_wheel.py), `CARGO_TARGET_DIR` and `WebAssembly.Module` coverage in [verify_wasm.mjs](../../../playground/tests/verify_wasm.mjs), native `--version` smoke test in [release.yml](../../../.github/workflows/release.yml), and Python `3.10`+`3.13` CI matrix. | Minor | **Fix** | Applied in each origin commit and verified across CPython `3.10` and `3.13`. |
| Pages Workflow & Historical WASMs | Claim that [pages.yml](../../../.github/workflows/pages.yml) needed an `--imports-only` filter to skip historical releases (`v0.3.0`) carrying `wasm-bindgen` imports. | Major | **Reject** | False premise: `v0.1.0`–`v0.3.0` predate `brush-parser` and have `0` imports (verified against the published `v0.3.0` asset), and [release.yml](../../../.github/workflows/release.yml) gates future releases. |
| Python Signal Disposition | Proposal to mutate `signal.signal(SIGINT, SIG_DFL)` in [_cli.py](../../../crates/omnicoach-py/python/omnicoach/_cli.py) instead of propagating `KeyboardInterrupt`. | Minor | **Reject** | `signal.signal` fails off the main thread and leaks `SIG_DFL` into in-process Python callers; `py.detach` + `py.check_signals()?` $\to$ `KeyboardInterrupt` $\to$ `SystemExit(130)` is idiomatic PyO3. |
| Diagnostic Module Boundary | Proposal to move `OutputFormat` and `print_diagnostics` from [src/diagnostic.rs](../../../src/diagnostic.rs) into [src/cli.rs](../../../src/cli.rs) and make `EXIT_*` private. | Minor | **Reject** | Single-consumer type moves violate [ADR 012](../decisions/012_module_placement_by_abstraction.md) and diverge from domain `clap` types across `src/`, and private `EXIT_*` breaks `private_intra_doc_links`. |
| Speculative Harness Checks | Proposals for a Mach-O signature parser, `importlib.metadata` version check, `.omnicoach.toml` suppression check in `verify_wheel.py`, and `Compress-Archive -Force`. | Minor | **Reject** | Already covered by macOS `dlopen` in CI, `maturin`'s `Cargo.toml` metadata, [tests/cli.rs](../../../tests/cli.rs), and fresh release checkouts. |

---

## 4. Whole-Chain Simplifications

| Finding | Remediation & Rationale |
| :--- | :--- |
| Unreachable `BrokenPipeError` branch in [_cli.py](../../../crates/omnicoach-py/python/omnicoach/_cli.py) (Rust writes straight to file descriptors `1` and `2`). | Removed; `_cli.py` only forwards `argv` and translates `KeyboardInterrupt` to `130`. |
| Wheel CI ran only on Linux while `omnicoach-py` has no Rust test targets. | Expanded the CI `wheel` matrix in [ci.yml](../../../.github/workflows/ci.yml) to Linux, macOS, and Windows. |
| [verify_wheel.py](../../../scripts/verify_wheel.py) (~760 lines, 34 single-use constants) dwarfed the wrapper it tests. | Rewritten around a compact `Venv` helper and `CLI_CASES` table (~400 lines, same parity and boundary checks). |
| Inline `mod brush_backend` re-indented ~750 lines inside [shell.rs](../../../src/command_lint/command/shell.rs). | Extracted unchanged to file-backed [shell/brush_backend.rs](../../../src/command_lint/command/shell/brush_backend.rs); `shell.rs` holds only shell domain types. |
| Verbose wheel internals in [README.md](../../../README.md) installation and hook sections (`~/.cargo/bin` listed before `~/.local/bin`). | Put `~/.local/bin` first and moved packaging internals out of end-user installation instructions into [ADR 014](../decisions/014_distribution_and_wasm_targets.md). |
