# Phase 1 — macOS Santa `Lockdown` Empirical Probe

Read-only measurements from a corporate Apple Silicon workstation running [Santa](https://northpole.security/) in `Lockdown` mode, validating the environment assumptions in [01_understand.md](01_understand.md).

---

## 1. TL;DR

1. **Unseen ad-hoc `.so` `dlopen` succeeds**: Santa `Lockdown` intercepts `execve`, not `dlopen`. Both installed PyO3 wheels (`pydantic_core`, `Rule: None`) and a freshly linked unseen ad-hoc `.abi3.so` (`Rule: None`) loaded cleanly (`exit=0`) in `uv` Managed CPython 3.15 and Homebrew Python 3.14.
2. **`/usr/bin/codesign` is blocked**: Santa kills `/usr/bin/codesign` (`exit=-9`) via a `SigningID` rule. Builds and installs must rely solely on linker-emitted ad-hoc signatures (`rustc` / Apple `ld`).
3. **`uv` Managed Python cold start is ~19 ms median (~24 ms p95)**: That sits well inside the 50 ms $p_{50}$ / 150 ms $p_{99}$ hook budget and beats a single `santactl fileinfo` query (~33 ms) or Node WASM compilation (~56 ms).
4. **Self-service allowlisting requires per-SHA-256 peer votes**: Only `205` centrally managed `TeamID` rules exist versus `108,688` `Binary` rules, so unsigned standalone executables require re-voting on every release.

---

## 2. Measured Target Environment

- **OS / Arch**: `Darwin 25.6.0 (arm64)`
- **Active `PATH` Python**: `/opt/homebrew/opt/python@3.14/bin/python3.14` $\to$ `Python.framework/.../bin/python3.14` (`Executable (arm64)`, `Rule: Allowed (Binary)`)
- **`uv` Python Resolution (`uv python find`, `--no-project`, `--managed-python`)**: `~/.local/share/uv/python/cpython-3.15.1-macos-aarch64-none/bin/python3.15` (`Executable (arm64)`, `Rule: Allowed (Binary)`)
- **Available WASI / JS Runtimes**: `node` `v26.5.0` (`Allowed (Binary)`); `wasmtime`, `deno`, and `bun` not installed.

### 2.1 Santa Daemon Status Summary

```text
Mode                        | Lockdown
Transitive Allowlisting     | Enabled (Compiler Rules: 1, Transitive Rules: 26)
Bundle Scanning             | Yes
Binary Rules                | 108,688
Certificate Rules           | 4,517
TeamID Rules                | 205
SigningID Rules             | 217
CDHash Rules                | 0
```

### 2.2 Unseen Ad-Hoc `.so` `dlopen` Verification

1. **`/usr/bin/codesign` block**: Invoking `/usr/bin/codesign` triggered a Santa `SigningID` block (`exit=-9`).
2. **Existing PyPI PyO3 `.so` wheel** (`pydantic_core/_pydantic_core.cpython-313-darwin.so`): `Type: Dynamic Library (arm64)`, `Rule: None` $\to$ `ctypes.CDLL` succeeded (`exit=0`) in both Homebrew Python 3.14 and `uv` Managed Python 3.15.
3. **Freshly compiled unseen linker-signed ad-hoc `.abi3.so`** (linked via `/usr/bin/cc -shared -fPIC`, which is not a Santa transitive compiler rule, with a unique timestamp symbol):

   ```text
   Type                   : Dynamic Library (arm64)
   Code-signed            : Yes, but ad-hoc
   Rule                   : None
   Expected Decision      : Blocked (Unknown, Lockdown mode)
   ```

   `santactl fileinfo` reports what `execve` would decide if executed as a binary; loading via `ctypes.CDLL` (`dlopen`) succeeded (`exit=0`) in both interpreters.

### 2.3 Cold-Start Subprocess Latency (`N = 20` runs)

| Execution Path | Median (ms) | p95 (ms) | Failures |
| :--- | ---: | ---: | ---: |
| `/usr/bin/true` (OS `exec` baseline) | 6.22 | 17.84 | 0 |
| `uv` Managed `python3.15 -S` + unseen `.so` `dlopen` (no `site`) | 17.41 | 18.54 | 0 |
| **`uv` Managed `python3.15` + unseen `.so` `dlopen` (`uv` console-script path)** | **18.76** | **23.65** | **0** |
| `santactl fileinfo --key Rule` | 33.25 | 75.93 | 0 |
| `node` `WebAssembly.compile` (`omnicoach_playground.wasm`, ~3.9 MB) | 56.28 | 78.11 | 0 |
| Homebrew `python3.14 -S` + `.so` `dlopen` (framework trampoline, no `site`) | 78.00 | 275.10 | 0 |
| Homebrew `python3.14` + `.so` `dlopen` (framework trampoline) | 86.24 | 275.21 | 0 |
| Homebrew `python3.14` $\to$ `node` `node:wasi` (58 B minimal WASM) | 101.08 | 157.51 | 0 |

**Analysis & Interpretation**:

- `uv` Managed Python spawns a single standalone Mach-O binary and loads an unseen `.so` in `18.76 ms` median—nearly twice as fast as querying `santactl fileinfo` (`33.25 ms`) before any CLI work begins.
- Homebrew Python on macOS uses a `Python.app` framework wrapper that performs a second `execve`, adding ~67 ms (`86.24 ms` vs `18.76 ms`); installing with `uv tool install --managed-python omnicoach` avoids that wrapper.

### 2.4 Target Environment Scope

- **Corporate macOS (`arm64` Santa `Lockdown`)**: Primary constrained environment measured above.
- **Linux `x86_64` workstations and VMs**: Unconstrained for standard `uv` ELF and `.so` wheels.
- **Personal macOS and Linux machines**: Unconstrained.
- **GitHub Pages Web Playground**: Consumes `omnicoach_playground.wasm` independently in browser and JS runtimes.
