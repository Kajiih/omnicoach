# Reusability Strategy

To maintain a cohesive and performant toolkit while avoiding "reinventing the wheel," we rely on world-class ecosystem crates to perform the heavy lifting across our package modules.

## Why Not a Pre-Existing Framework?

While tools like `ast-grep` and `semgrep` exist for parsing code, they do not support orchestrating Command Guards or Environment Context. Our package acts as a unified orchestrator (sharing config and reporting via `src/config.rs` and `src/diagnostic.rs`) to bring these disparate domains under a single `.toml` configuration and CLI binary (`omnicoach code` and `omnicoach command`).

## Heavy-Lifting Crates to Reuse

Instead of writing complex traversal or parsing logic from scratch, the modules delegate to the following dependencies:

### 1. Diagnostics & Reporting (`src/diagnostic.rs`)

* **Standard Output & Zero-Dependency Formatting**: To minimize dependencies and compilation overhead, we avoid complex reporting engines like `miette`. Instead, we calculate line and column offsets dynamically using a fast, inline helper function and output diagnostics in the standard compiler format (`path:line:col: [rule-name] message`). This ensures effortless integration with IDEs, terminal output readers, and CI pipelines out of the box.

### 2. Code Parsing & Static Analysis (`src/code_lint/`)

* **`ruff_python_parser` & `ruff_python_ast`**: Powers Python parsing and typed AST traversal with native string/bytes escape decoding and zero-allocation AST slices. Python comment extraction uses a lightweight lexer state machine over `&file.source` (handling quotes, raw prefixes, escapes, and PEP 701 nested f-strings) to avoid pulling in `ruff_python_trivia`.
* **`ra_ap_syntax`**: Powers error-tolerant, lossless Rust CST/AST parsing (`rust-analyzer`'s syntax crate), used both by Rust lint rules and by `tests/architecture_conformance.rs` (`summarize_rust_file`).

### 3. Command Validation (`src/command_lint.rs`)

* **`ast-grep-core` / `tree-sitter-bash` & `shell-words`**: Currently used by `InterceptedCommand::parse_all` to extract shell commands and split arguments safely (scheduled to be replaced by a pure-Rust shell AST per `ROADMAP.md`).

### 4. Environment & Context Guards (`src/command_lint/rules/`)

* **`std::process::Command` (for Git & Jujutsu)**: To keep compilation fast and avoid borrow-checker conflicts and API churn from raw library dependencies (like `gix` or `jj-lib`), we spawn subprocess calls (e.g., `git status`, `jj log`) to query repository state. The ~5ms overhead is completely negligible for a local hook execution.

### 5. File System Traversal (`src/bin/omnicoach.rs`)

* **`ignore`**: Rather than writing custom recursive directory walkers and manually parsing gitignore rules, we embed the `ignore` crate (powering `ripgrep`). It respects `.gitignore`, `.ignore`, global exclusions, and hidden files out of the box.
