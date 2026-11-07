# Phase 1 — Understand: Locked-Down Wheel Distribution & WebAssembly Targets

## 1. Problem Statement

`omnicoach` was distributed to Python and multi-language repositories as a PyPI wheel built with `maturin` (`bindings = "bin"` in [pyproject.toml](../../../pyproject.toml)) and published across Linux, macOS, and Windows by [release.yml](../../../.github/workflows/release.yml). Installing via `uv tool install omnicoach` or `uv sync` placed an ad-hoc-signed Rust Mach-O executable directly at `bin/omnicoach`.

On corporate macOS workstations running [Santa](https://northpole.security/) in `Lockdown` mode ([00_user_report.md](00_user_report.md), [01b_macos_santa_probe.md](01b_macos_santa_probe.md)), the kernel EndpointSecurity hook blocks `execve` on any unallowlisted Mach-O executable:

1. **Agent lifecycle hooks fail or spam block notifications**: Every `PreInvocation` and `PreToolUse` event invokes `omnicoach hook antigravity ...` ([src/hook.rs](../../../src/hook.rs)).
2. **VCS pre-push hooks fail**: `command -v omnicoach` succeeds because `bin/omnicoach` is present and executable, causing `omnicoach code .` to abort `jj git push`.
3. **Per-release hash churn**: Self-service peer voting allowlists unsigned binaries strictly by SHA-256, breaking every new release until re-approved.
4. **Downstream WASM shims drift from the CLI**: Consumer repositories worked around the block with ~440-line Python wrappers driving `omnicoach_playground.wasm` ([playground/src/lib.rs](../../../playground/src/lib.rs)). Because the playground only serves single-file `lint`, `explain`, and `catalog` requests over `wasm32-unknown-unknown`, those wrappers re-implement CLI parsing, directory walking, and config loading while omitting `hook`, `command`, `graph`, `--diff`, and cross-file rules.
5. **`wasm-bindgen` import leak on `main` in `omnicoach_playground.wasm`**: Replacing `ast-grep` with `brush-parser` pulled transitive `wasm-bindgen` dependencies onto `wasm32-unknown-unknown` ([02_references.md](02_references.md)). Unreleased `main` builds deployed to GitHub Pages imported `__wbindgen_*` symbols, breaking zero-import instantiation in [playground/www/omnicoach.js](../../../playground/www/omnicoach.js).

---

## 2. Goals & Non-Goals

### Goals

- **Zero-friction execution under Santa `Lockdown`**: Installing via `uv tool install omnicoach` or `uv sync` must run out of the box on Santa `Lockdown` macOS hosts without per-release SHA-256 voting, `/usr/bin/codesign` calls, or repository-local shims.
- **Full CLI and hook parity**: Every subcommand and flag in [src/bin/omnicoach.rs](../../../src/bin/omnicoach.rs)—`code`, `command`, `graph`, `hook antigravity`, `--explain`, `--list-rules`, `--list-tags`, and `.omnicoach.toml` discovery—must run from one Rust implementation.
- **Hook-grade startup and native throughput**: Agent hook calls sit on the critical path of every tool call and must stay within $\le 50\text{ ms}$ ($p_{50}$) and $\le 150\text{ ms}$ ($p_{99}$). Full-repository `omnicoach code` runs must keep multi-core throughput.
- **Standalone Rust binary and architectural conformance**: `cargo install omnicoach` and `cargo test` must continue building a standalone native binary with zero Python or Node requirements, conforming to [ADR 006](../decisions/006_architectural_dag_and_conformance.md), [ADR 012](../decisions/012_module_placement_by_abstraction.md), and `unsafe_code = "deny"`.
- **Zero-import browser playground WASM**: `omnicoach_playground.wasm` must compile with zero host imports so [playground/www/omnicoach.js](../../../playground/www/omnicoach.js) instantiates cleanly, guarded in CI.

### Non-Goals

- **Second CLI implementation in Python**: `omnicoach` will not maintain a parallel CLI parser, file walker, or config loader in Python.
- **Public `import omnicoach` Python library API**: The Python package inside the wheel serves strictly as a CLI launcher; a public in-process Python SDK is tracked on [ROADMAP.md](../../../ROADMAP.md) §4.
- **Bypassing endpoint security policies**: Packaging relies strictly on standard Python extension wheels loaded by allowlisted interpreters, identical to `pydantic_core` and `markupsafe`.
- **Expanding the browser playground UI**: Adding `command`, `graph`, and multi-file project support to `playground/www/` is tracked on [ROADMAP.md](../../../ROADMAP.md) §4.

---

## 3. Validated Assumptions

All environment and budget assumptions were measured on the target Apple Silicon Santa `Lockdown` workstation in [01b_macos_santa_probe.md](01b_macos_santa_probe.md):

| Assumption | Status | Empirical Finding ([01b_macos_santa_probe.md](01b_macos_santa_probe.md)) |
| :--- | :--- | :--- |
| **Ad-hoc `.so` `dlopen` under `Lockdown`** | **Validated** | Even with `Bundle Scanning: Yes`, both `pydantic_core` and a freshly linked unseen ad-hoc `.abi3.so` (`Rule: None`) loaded cleanly in Homebrew Python 3.14 and `uv` Managed CPython 3.15. |
| **Allowlist model** | **Validated** | `santactl status` shows `108,688` `Binary` rules vs `205` centrally managed `TeamID` rules; self-service requires peer votes per binary SHA-256 on each release. |
| **Available runtimes** | **Validated** | `uv python find` resolves to allowlisted standalone CPython 3.15. `node` 26 is present on the Mac, while `wasmtime`, `deno`, and `bun` are absent (and `node` is not guaranteed in Python containers). |
| **Hook cold-start budget ($\le 50\text{ ms}$ $p_{50}$ / $\le 150\text{ ms}$ $p_{99}$)** | **Validated** | `uv` Managed Python + unseen `.so` `dlopen` runs in **~19 ms median (~24 ms p95)**. By contrast, `santactl fileinfo` alone takes ~33 ms, Node WASM compilation takes ~56 ms, and Homebrew Python $\to$ `node:wasi` takes ~101 ms before compiling `omnicoach`. |
| **Wheel size ($\le 5\text{ MB}$ compressed)** | **Validated** | The `cp310-abi3` wheel measures **3.3 MiB** compressed ([02_references.md](02_references.md)), with one wheel per OS/architecture covering CPython `3.10`–`3.15+`. |
| **Target platforms & CPython versions** | **Validated** | Linux (`x86_64`, `aarch64`), macOS (`arm64`, `x86_64`), and Windows (`x64`) across CPython $\ge 3.10$; `uv` on target Macs already resolves to CPython `3.15`, making `abi3-py310` forward compatibility essential. |

---

## 4. Scope Decisions

- **Prototype head-to-head before committing**: Benchmark a thin PyO3 `abi3-py310` wheel against a `wasm32-wasip1` CLI under `node:wasi` across CLI parity, cold hook latency, multi-core throughput, artifact size, and pipe/signal behavior ([02_references.md](02_references.md)).
- **Never invoke `/usr/bin/codesign`**: Santa blocks `/usr/bin/codesign` (`exit=-9`) via a `SigningID` rule ([01b_macos_santa_probe.md](01b_macos_santa_probe.md)), so macOS builds must rely exclusively on linker-emitted ad-hoc signatures (`rustc` / Apple `ld`).
- **Ship dual native channels**: Publish the `cp310-abi3` extension wheel to PyPI for Python and Santa `Lockdown` users, and publish prebuilt standalone native binary archives (with `cargo-binstall` metadata) on GitHub Releases alongside crates.io for ~3 ms zero-Python startup.
- **Support `python -m omnicoach` and enforce zero WASM imports in CI**: Include `omnicoach/__main__.py` alongside the `omnicoach` console script, and gate `omnicoach_playground.wasm` in CI so host imports cannot regress.
