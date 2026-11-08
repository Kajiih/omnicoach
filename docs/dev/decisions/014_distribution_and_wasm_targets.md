# ADR 014: Distribution Channels and WebAssembly Targets

## 1. Problem Statement

`omnicoach` was published to PyPI as a `maturin` `bindings = "bin"` wheel, installing the compiled Rust executable directly at `bin/omnicoach`. On macOS workstations running Santa in `Lockdown` mode, `execve` of an unallowlisted Mach-O executable is blocked. Self-service allowlisting is per SHA-256, so every release required re-approval, and `/usr/bin/codesign` is itself blocked on those machines. Consumers worked around this with per-repository Python shims over `omnicoach_playground.wasm` that re-implemented the CLI in Python and lacked `hook`, `command`, `graph`, and `--diff`.

Separately, `omnicoach_playground.wasm` on `main` had gained five `wasm-bindgen` host imports through `brush-parser`'s dependencies, breaking its zero-import contract with [playground/www/omnicoach.js](../../../playground/www/omnicoach.js).

The measurements behind this decision are in [01b_macos_santa_probe.md](../wheel_and_wasm/01b_macos_santa_probe.md) and [02_references.md](../wheel_and_wasm/02_references.md).

## 2. Decision

`omnicoach` ships one Rust CLI implementation, `omnicoach::cli::run_from_args` in [src/cli.rs](../../../src/cli.rs), through two native channels, plus a browser-only WebAssembly module:

| Channel | Artifact | Entry point | Startup |
| :--- | :--- | :--- | :--- |
| PyPI (`uv tool install`, `uv add`, `pipx`) | `cp310-abi3` wheel per OS/architecture containing `_omnicoach.abi3.so` / `.pyd` ([crates/omnicoach-py](../../../crates/omnicoach-py/src/lib.rs)) | Python console script and `python -m omnicoach`, calling `run_from_args` in-process | ~20–30 ms |
| GitHub Releases, `cargo binstall`, crates.io | Standalone native binary ([src/bin/omnicoach.rs](../../../src/bin/omnicoach.rs)); static `musl` on Linux | Direct `execve` | ~3 ms |
| GitHub Pages | `omnicoach_playground.wasm` for `wasm32-unknown-unknown`, zero host imports | [playground/www/omnicoach.js](../../../playground/www/omnicoach.js) | Browser only |

- **The wheel loads Rust as a Python extension, not as an executable.** Santa `Lockdown` gates `execve` rather than `dlopen`, so an allowlisted Python interpreter loads the linker-signed `.abi3.so` without per-release approval or `codesign`. One `abi3` wheel per platform covers CPython 3.10 and later.
- **No CLI logic in Python.** The Python package only forwards `argv` and maps `KeyboardInterrupt` to exit code 130. The extension releases the GIL while the CLI runs and checks for signals afterwards. `run_from_args` therefore never calls `std::process::exit` and treats a closed stdout as a normal early exit.
- **The browser module stays dependency-free.** `brush-parser` is excluded from `wasm32-unknown-unknown` builds; shell parsing lives in the cfg-gated [shell/brush_backend.rs](../../../src/command_lint/command/shell/brush_backend.rs), while command rules stay registered so `--explain` and config selectors keep working. CI fails if the module gains any import ([verify_wasm.mjs](../../../playground/tests/verify_wasm.mjs)).

## 3. Rejected and Deferred Options

- **Rejected: a `santactl` check that falls back to a `wasm32-wasip1` CLI under Node.** Querying `santactl` alone costs more than running the whole wheel, Node recompiles the module on every hook call (~250 ms), WASI runs single-threaded (~8× slower on a full repository), and the wheel would carry two binaries.
- **Rejected: an Apple Developer ID–signed `bindings = "bin"` wheel.** It requires signing infrastructure, and the extension-module wheel already runs on `Lockdown` machines.
- **Deferred ([ROADMAP.md](../../../ROADMAP.md) §4):** a standalone `wasm32-wasip1` CLI (trigger: a sandboxed host that cannot load native code), a public `import omnicoach` Python API (trigger: demand for in-process linting from Python), `command`, `graph`, and multi-file project support in the browser playground, and notarized macOS standalone release binaries.

## 4. Consequences

- A release publishes the wheels and sdist to PyPI, the standalone archives and playground module to GitHub Releases, and the `omnicoach` crate to crates.io, all from one tag ([releasing.md](../releasing.md)); `[workspace.package]` keeps their versions identical.
- Code reachable from `run_from_args` must not call `std::process::exit` (it would end the host Python process) or `println!` (it panics when stdout is closed).
- Dependencies added to code compiled for the playground must not pull in `wasm-bindgen` on `wasm32-unknown-unknown`; gate them as `brush-parser` is gated.
- The `omnicoach-py` crate has no Rust tests of its own; CI covers it by building and running the wheel on Linux, macOS, and Windows with [verify_wheel.py](../../../scripts/verify_wheel.py).
