# Project Structure & Naming

## Decision: Single Crate Package Architecture (Consolidated from Workspace)

To ensure the toolkit is **standalone, modular, and not tied to any specific downstream environment**, we structure the codebase as a single Cargo package (`omni`) with domain-specific library modules and binary targets. While we initially designed the system around a **Cargo Workspace**, we consolidated it into a single crate to reduce dependency and build overhead while maintaining strict logical boundaries via modules.

We use the name **Omni** for the crate, reflecting the tool's ability to check everything (code, commands, and workflow context).

## Crate & Module Layout

*   **`src/lib.rs` / `src/core.rs`**: The shared core library logic. Contains agnostic evaluation logic, configuration parsing (`.omnilint.toml`), the common `Diagnostic` structure, and rule/tag generic traits.
*   **`src/rules.rs`**: The central registry containing the `Tag` enum and rule registry declarations (`CodeLintRule` and `CommandLintRule`).
*   **`src/bin/omni-code-lint.rs`**: The static codebase linter binary. Leverages `src/code_lint.rs` to run static checks on files.
*   **`src/bin/omni-command-lint.rs`**: The command safety linter binary. Leverages `src/command_lint.rs` to check command syntax and version control environment context.
*   **`src/code_lint.rs`**: Module containing entry points and dispatcher for static code structure analysis.
*   **`src/command_lint.rs`**: Module containing entry points, `InterceptedCommand` parsing, and dispatchers for command-line validation.
*   **`src/command_lint/vcs.rs`**: VCS adapter layer (traits and clients for git and jujutsu).
*   **`src/command_lint/rules/jj_edit.rs`**: Rule implementation for blocking `jj edit` commands on described commits.
