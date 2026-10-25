# Phase 4 — Execution Log: Global & Domain-Scoped Configuration

## 1. Overview & Verification Summary

This document records the Phase 4 execution of the Global and Domain-Scoped Configuration feature designed in [`03_design.md`](./03_design.md) and governed by `/phased-engineering-workflow`.

All changes were implemented using Test-Driven Development (Audit $\to$ RED $\to$ GREEN $\to$ Verify), following all architectural DAG boundaries ([ADR 006](../decisions/006_architectural_dag_and_conformance.md)), colocation ordering rules ([ADR 011](../decisions/011_colocated_abstraction_ordering.md)), and component placement ([ADR 012](../decisions/012_module_placement_by_abstraction.md)).

### Quality Gates Status: **ALL GREEN**

- `cargo fmt --check`: Clean (0 diffs).
- `cargo clippy --all-targets -- -D warnings`: Clean (0 warnings, 0 errors).
- `cargo test`: All 2,473 tests passing (0 failures).
- `cargo run -- code .` (Self-dogfooding lint): **All checks passed!** (0 diagnostics).

---

## 2. Completed TDD Tasks

### Task T1: `Config.root_dir` & Pure `normalize_path_with_roots`

- **Files Modified**:
  - `src/config.rs`:
    - Added `pub root_dir: Option<PathBuf>` to `Config`.
    - Implemented `pub(crate) fn lexical_normalize(path: &Path) -> PathBuf` to collapse `.` and `..` segments lexically.
    - Implemented `fn normalize_path_with_roots(path: &Path, root_dir: Option<&Path>, cwd: Option<&Path>) -> String` to handle absolute and relative paths without repetitive `getcwd` syscalls in hot glob loops, deferring `current_dir()` until strictly necessary.
- **Tests Added/Verified**:
  - `config::tests::test_normalize_path_with_roots` (parameterized over absolute, relative-from-root, relative-from-subdir, parent traversal, and `./` forms).
  - `config::tests::test_root_dir_strips_prefix_for_per_file_ignores`.

### Task T2: Domain-Scoped Config Resolution & Hermetic Environment

- **Files Modified**:
  - `src/rule_selection.rs`:
    - Defined `default_global_config_path`: returns `None` under `#[cfg(test)]`, and calls `load_env()` to resolve `$XDG_CONFIG_HOME/omnicoach/config.toml` $\to$ `~/.config/omnicoach/config.toml` under normal execution.
    - Implemented pure `resolve_global_config_path(xdg_config_home, home)`.
    - Implemented domain-scoped loaders: `load_config`, `load_command_config`, `load_config_from_root`, `load_command_config_from_root`, and domain-aware `load_rule_status`.
    - Implemented `resolve_domain_config_with` returning `ResolvedDomainConfig { content, root_dir }`.
- **Tests Added/Verified**:
  - `rule_selection::tests::test_resolve_global_config_path_variants` (covering XDG path, Home fallback, empty env fallback, and all unset).
  - `rule_selection::tests::test_domain_scoped_config_resolution` (verifying Code domain prefers repo config, Command domain prefers global config).
  - `rule_selection::tests::test_code_domain_stops_at_vcs_boundary_and_uses_global_config`.

### Task T3: CLI Subcommands & Command Wiring

- **Files Modified**:
  - `src/bin/omnicoach.rs`:
    - Routed `DomainCommand::Command` to `load_command_config`.

### Task T4: Hook Integration & Deduping

- **Files Modified**:
  - `src/hook/runner.rs`:
    - Replaced duplicate `normalize_path` with `crate::config::lexical_normalize`.
    - Consolidated `load_hook_command_config` and `load_hook_code_config` into a single fail-open shared helper `load_hook_config(project_root, load_fn)`.
    - Passed `&target_file_path` directly to `lint_file` and `filter_relevant_diagnostics` in `lint_proposed_edit`, letting `Config.root_dir` normalize paths cleanly.
- **Tests Added/Verified**:
  - `hook::runner::tests::evaluate_pre_tool_use_fails_open_when_config_is_invalid`.

### Task T5: Hermetic CLI & Hook Integration Tests

- **Files Modified**:
  - `tests/cli.rs`:
    - Hermetically isolated `run_cli`, `run_hook_stdin`, and `test_self_dogfooding_code_lint` with `.env("XDG_CONFIG_HOME", isolated_xdg_config_home())`.
    - `test_global_config_fallback_for_code_and_repo_precedence`: exercises production `XDG_CONFIG_HOME` discovery.
    - `test_global_config_precedence_for_command`: exercises command rule precedence.
    - `test_hook_global_config_precedence_for_command`: verifies `omnicoach hook` intercepts commands under global config even when repo config targets code rules.
    - `test_global_config_precedence_for_explain`: verifies domain-aware rule explanations.
    - `test_subdirectory_invocation_discovers_ancestor_config_and_anchors_globs`: non-vacuous test exercising ancestor config discovery, `"tests/**"` glob anchoring, and `..` parent traversal from subdirectories.

---

## 3. Deviations & Surgical Adaptations During Execution

1. **`multi-phase-test` in `tests/cli.rs`**:
   - Split compound command + explain test into two single-phase tests (`test_global_config_precedence_for_command` and `test_global_config_precedence_for_explain`), each containing a single arrange-act-assert phase.
2. **`logic-in-tests` in `tests/cli.rs`**:
   - Replaced conditional `if !describe_output.status.success() { ... }` in test setup with moving `assert!(describe_output.status.success())` to the assertion phase alongside the outcome check.
3. **`clippy::option_if_let_else` in `src/config.rs`**:
   - Refactored nested `if let / else` path stripping into idiomatic combinators (`.ok().map(Path::to_path_buf).or_else(...)`).
4. **`clippy::too_long_first_doc_paragraph` in `src/rule_selection.rs`**:
   - Shortened first doc comment paragraphs on `load_command_config`, `load_config_from_root`, and `load_command_config_from_root` to single concise summary sentences.
5. **Code Duplication Removal across Modules**:
   - Reused `pub(crate) fn lexical_normalize` from `src/config.rs` inside `src/hook/runner.rs`, deleting 25 lines of duplicate component-normalization code.
6. **Eliminated Redundant Syscalls in Hot Loops**:
   - Extracted `normalize_path_with_roots` in `src/config.rs` to eliminate ~145 redundant `getcwd` syscalls per file during Rayon parallel lint walks when `per_file_ignores` is non-empty.
