# Reusability Strategy

To maintain a cohesive and performant toolkit while avoiding "reinventing the wheel," we rely on world-class ecosystem crates to perform the heavy lifting across our package modules.

## Why Not a Pre-Existing Framework?
While tools like `ast-grep` and `semgrep` exist for parsing code, they do not support orchestrating Command Guards or Environment Context. Our package acts as a unified orchestrator (sharing config and reporting via `src/core.rs`) to bring these disparate domains under a single `.toml` configuration, while deploying them as separate, focused binaries (`omni-code-lint` and `omni-command-lint`).

## Heavy-Lifting Crates to Reuse
Instead of writing complex traversal or parsing logic from scratch, the modules delegate to the following dependencies:

### 1. Diagnostics & Reporting (`src/diagnostic.rs`)
*   **Standard Output & Zero-Dependency Formatting**: To minimize dependencies and compilation overhead, we avoid complex reporting engines like `miette`. Instead, we calculate line and column offsets dynamically using a fast, inline helper function and output diagnostics in the standard compiler format (`path:line:col: [CODE] message`). This ensures effortless integration with IDEs, terminal output readers, and CI pipelines out of the box.

### 2. Code Parsing & Static Analysis (`src/code_lint.rs`)
*   **`tree-sitter` & `tree-sitter-<language>`**: We use the official Rust bindings to parse code into a unified AST across all supported languages. We do not write lexers or parsers.
*   **`ast-grep-core`**: For the "Declarative First" branch of our rule registry, we embed the `ast-grep-core` crate. This allows us to evaluate YAML/String-based code patterns without writing our own matching engine or resorting to raw, unreadable Tree-sitter `.scm` queries.

### 3. Command Validation (`src/command_lint.rs`)
*   **`shell-words`**: To avoid compiling heavy tree-sitter grammars and ast-grep-core for simple CLI checking, we use `shell-words` to tokenize command strings safely. We then run direct pattern validations on the parsed argument list.

### 4. Environment & Context Guards (`src/command_lint/rules/`)
*   **`std::process::Command` (for Git & Jujutsu)**: To keep compilation fast and avoid borrow-checker conflicts and API churn from raw library dependencies (like `gix` or `jj-lib`), we spawn subprocess calls (e.g., `git status`, `jj log`) to query repository state. The ~5ms overhead is completely negligible for a local hook execution.

### 5. File System Traversal (`src/bin/omni-code-lint.rs`)
*   **`ignore`**: Rather than writing custom recursive directory walkers and manually parsing gitignore rules, we embed the `ignore` crate (powering `ripgrep`). It respects `.gitignore`, `.ignore`, global exclusions, and hidden files out of the box.
