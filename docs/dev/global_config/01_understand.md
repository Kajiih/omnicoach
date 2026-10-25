# Phase 1: Understand — Global & Domain-Scoped Configuration

## 1. Problem Statement & Motivation

`omnicoach` enforces high-signal static analysis code rules and Jujutsu (`jj`) command workflow safety rules across both CLI invocations and coding agent lifecycle hooks (`omnicoach hook`).

Previously, configuration resolution had three major architectural limitations:

1. **Monorepo and Unconfigured Repository Gap**:
   In corporate monorepos (such as virtual cloud workspaces) or external team codebases, checking a repository-level `.omnicoach.toml` into a team-owned directory is often prohibited or undesirable. Developers and autonomous agents working in these environments still require personalized baseline rule configurations, custom test patterns, and per-file ignores across both `omnicoach code` and `omnicoach hook`.

2. **Domain Conflict between Code and Command Rules**:
   A repository's `.omnicoach.toml` often uses `select = [...]` to restrict analysis to specific languages or categories (for example, `select = ["python"]`). Because `select` acts as an exhaustive allowlist, this inadvertently disables all command workflow guardrails (such as `edit-of-described-commit`). Command rules protect personal and agent workflow hygiene (e.g. preventing editing described commits or running unsafe history rewrites) and should follow user-global policies across all repositories rather than being disabled by a repository's code lint settings.

3. **Subdirectory Invocation & Glob Mis-anchoring**:
   CLI commands (`omnicoach code`, `omnicoach graph`, `omnicoach --explain`) only checked `./.omnicoach.toml` directly in `current_dir()`, failing to discover parent `.omnicoach.toml` files when invoked from subdirectories (e.g. `cd tests && omnicoach code cli.rs`). Furthermore, `Config` did not track its root directory, causing `per-file-ignores` and `context.test_patterns` globs (such as `"tests/**"`) to fail to match when relative paths were supplied from subdirectories.

---

## 2. Goals & Non-Goals

### Goals

- **Domain-Scoped Configuration Resolution**:
  - **Code Domain** (`code`, `graph`, hook `FileEdit`, `--explain <code-rule>`):
    Prioritizes repository-level reproducibility.
    Hierarchy: `--config <PATH>` $\to$ nearest ancestor `.omnicoach.toml` (stopping at VCS root) $\to$ global configuration (`$XDG_CONFIG_HOME/omnicoach/config.toml` / `~/.config/omnicoach/config.toml`) $\to$ `Config::default()`.
  - **Command Domain** (`command`, hook `RunCommand`, `--explain <command-rule>`):
    Prioritizes user workflow safety across all repositories.
    Hierarchy: `--config <PATH>` $\to$ global configuration $\to$ nearest ancestor `.omnicoach.toml` (stopping at VCS root) $\to$ `Config::default()`.
- **Nearest Ancestor Discovery**:
  - Discovers the nearest `.omnicoach.toml` by walking ancestor directories, stopping at the repository VCS boundary (`.jj` or `.git`) to avoid picking up unrelated parent configs, while traversing up freely in VCS-less virtual monorepo workspaces.
- **Unified Subdirectory Glob Anchoring**:
  - `Config` retains `root_dir: Option<PathBuf>`. `is_test_path` and `is_rule_enabled_for_path` globs are anchored relative to `root_dir` regardless of whether the CLI was invoked from the repository root or a subdirectory.
- **Hermeticity & CI Isolation**:
  - In-process unit tests (`#[cfg(test)]`) ignore host environment variables by default without requiring `unsafe { std::env::set_var(...) }`.
  - Integration tests in `tests/cli.rs` isolate subprocess runs with an empty temporary `XDG_CONFIG_HOME` directory (`isolated_xdg_config_home()`) by default.

### Non-Goals

- **No Multi-File TOML Cascading or Merging**:
  The winning configuration file (whether explicit, ancestor, or global) is parsed as an autonomous unit. No deep dictionary merging across parent and child TOML files is performed, avoiding ambiguous override semantics and silent inheritance bugs.
- **No In-Tree Repository Mutation**:
  The global configuration file lives strictly in user configuration directories (`$XDG_CONFIG_HOME` or `$HOME/.config`), never in workspace source trees.

---

## 3. Validated Design Decisions

- **Domain Precedence Split**:
  Code rules are repo-first (fallback to global); Command rules are global-first (fallback to repo).
- **Single Winner Ancestor Discovery**:
  Nearest ancestor `.omnicoach.toml` wins completely.
- **Root Directory Glob Anchoring**:
  `Config::root_dir` anchors all path matching relative to the discovered project root.
- **Standard XDG Isolation**:
  `XDG_CONFIG_HOME` environment redirection guarantees hermetic builds and testing.
