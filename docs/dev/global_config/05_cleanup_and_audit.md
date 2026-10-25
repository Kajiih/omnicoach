# Phase 5 & 6 — Clean Up, Multi-Reviewer Audit & Triage Report: Global & Domain-Scoped Configuration

## 1. Overview

Following `/phased-engineering-workflow`, this document records the Phase 5 (Clean up) and Phase 6 (Independent Multi-Reviewer Audit) activities for the Global and Domain-Scoped Configuration feature.

Two independent read-only review agents evaluated the uncommitted changes in parallel across strict architectural boundaries:

1. **Area 1 Auditor (`d13ff7b7`)**: Core Config, Glob Normalization & Rule Selection (`src/config.rs`, `src/rule_selection.rs`).
2. **Area 2 Auditor (`fd541ec5`)**: CLI Subcommands, Hook Integration, E2E Tests & Documentation (`src/bin/omnicoach.rs`, `src/hook/runner.rs`, `tests/cli.rs`, `ROADMAP.md`, `docs/dev/global_config/`).

---

## 2. Phase 5: Clean Up Summary

1. **Structured Return & Redundant Clone Removal (`src/rule_selection.rs`)**:
   - Replaced raw tuple returns with the structured `ResolvedDomainConfig { content, root_dir }` record type.
   - Derived `local_config_path` before moving `discovered_root` into `root_dir`, eliminating a duplicate `PathBuf` clone.
   - Removed `.filter(|candidate| candidate.is_file())` before `read_optional_config_at`, eliminating a redundant `stat` syscall and allowing genuine I/O errors to propagate rather than masking as missing files.
2. **Code Deduplication & Syscall Elimination (`src/config.rs` & `src/hook/runner.rs`)**:
   - Made `lexical_normalize` `pub(crate)` in `src/config.rs` and deleted the duplicate 25-line `normalize_path` function in `src/hook/runner.rs`.
   - Extracted `normalize_path_with_roots(path, root_dir, cwd)` in `src/config.rs`, eliminating ~145 `getcwd` syscalls per file in hot Rayon glob-matching loops.
   - Consolidated `load_hook_command_config` and `load_hook_code_config` into a single shared `load_hook_config` helper in `src/hook/runner.rs`.
   - Passed `&target_file_path` directly to `lint_file` and `filter_relevant_diagnostics` in `lint_proposed_edit`, removing double prefix stripping.
3. **Declaration Order & Architecture Conformance**:
   - Maintained [ADR 011](../decisions/011_colocated_abstraction_ordering.md) strict declaration order:
     - Caller-before-callee stepdown ordering in `src/rule_selection.rs`: `load_domain_config` $\to$ `ResolvedDomainConfig` $\to$ `resolve_domain_config` $\to$ `default_global_config_path` $\to$ `load_env` $\to$ `resolve_global_config_path` $\to$ `resolve_domain_config_with`.
     - Private helpers colocated directly above their public caller points or in the trailing shared helper section.
   - Maintained [ADR 006](../decisions/006_architectural_dag_and_conformance.md) architectural DAG conformance: `src/rule_selection.rs` (`RuleSelection`) depends only on permitted lower components (`Diff`, `RuleDeclaration`, `CodeLintRules`, `CodeLintSuppression`, `CommandLintRules`, `Config`), with zero forbidden cycles or dependencies.
4. **Test Hygiene & Isolation**:
   - Subprocess runners in `tests/cli.rs` (`run_cli`, `run_hook_stdin`, and `test_self_dogfooding_code_lint`) were hermetically isolated by defaulting `XDG_CONFIG_HOME` to `isolated_xdg_config_home()`.
   - Replaced vacuous `x = 1` fixture in `test_subdirectory_invocation_discovers_ancestor_config_and_anchors_globs` with non-exempt `k = 1`, and verified parent-relative paths (`../root_sample.py`) fail appropriately when not covered by `"tests/**"`.
   - Added `test_hook_global_config_precedence_for_command` to verify `omnicoach hook` honors global command rules even in code-only repositories.

---

## 3. Phase 6: Independent Multi-Reviewer Audit Triage Table

| ID | Area / Boundary | Finding Description | Severity | Verdict | Rationale & Remediation |
| :--- | :--- | :--- | :---: | :---: | :--- |
| **A1-1** | Area 1 (Config) | `normalize_path_for_glob` calls `std::env::current_dir()` inside dead fallback on relative paths, corrupts hook paths from subdirectories, and test has unused `sub_dir`. | Major | **Fix** | **Applied**: Extracted pure `normalize_path_with_roots(path, root_dir, cwd)`, joins `cwd` only when `current != root && current.starts_with(root)`, and replaced test with parameterized `test_normalize_path_with_roots` + `test_root_dir_strips_prefix_for_per_file_ignores`. |
| **A1-2** | Area 1 (Config) | Dead `\| Component::CurDir` arm in `lexical_normalize` and redundant `relative.strip_prefix("./")` in `normalize_path_for_glob`. | Minor | **Fix** | **Applied**: Removed dead arm in `lexical_normalize` and removed redundant `strip_prefix("./")` in `normalize_path_with_roots`. |
| **A1-3** | Area 1 (Config) | Redundant `discovered_root.clone()` and redundant `.filter(\|c\| c.is_file())` in `resolve_domain_config_with`. | Minor | **Fix** | **Applied**: Derived `local_config_path` before moving `discovered_root` into `root_dir`, and removed `.filter(...)` so `read_optional_config_at` handles `NotFound` and surfaces real I/O errors. |
| **A1-4** | Area 1 (Config) | `test_resolve_global_config_path_variants` missing empty-string `Some("")` cases for `xdg_config_home` and `home`. | Nit | **Fix** | **Applied**: Added `#[case::empty_xdg_falls_back_to_home]` and `#[case::empty_home_returns_none]`. |
| **A2-1** | Area 2 (CLI/Tests) | Vacuous test: `test_subdirectory_invocation_discovers_ancestor_config_and_anchors_globs` used `x = 1`, which is exempt by default in `single-letter-name`. | Major | **Fix** | **Applied**: Changed fixture to non-exempt `k = 1`, and added parent-relative traversal check `../root_sample.py` asserting exit code `1` with `[single-letter-name]`. |
| **A2-2** | Area 2 (CLI/Tests) | Zero tests for `omnicoach hook` domain config precedence. | Major | **Fix** | **Applied**: Added `test_hook_global_config_precedence_for_command`. |
| **A2-3** | Area 2 (Hook) | (1) `lint_proposed_edit` pre-strips `project_root`, duplicating `Config.root_dir` stripping and breaking globs when hook `cwd` is a subdirectory; (2) `normalize_path` duplicates `lexical_normalize`. | Minor | **Fix** | **Applied**: (1) Passed `&target_file_path` directly to `lint_file` and `filter_relevant_diagnostics`; (2) Exported `pub(crate) fn lexical_normalize` from `src/config.rs` and deleted `normalize_path` in `src/hook/runner.rs`. |
| **A2-4** | Area 2 (CLI/Tests) | Weak `!status.success()` assertions conflate exit code 1 with exit code 2; `test_self_dogfooding_code_lint` lacks explicit hermetic isolation. | Minor | **Fix** | **Applied**: Asserted `assert_eq!(output.status.code(), Some(1))` and added `.env("XDG_CONFIG_HOME", isolated_xdg_config_home())` to `test_self_dogfooding_code_lint`. |
| **A2-5** | Area 2 (Docs) | Drifted component names, test names, and terminology in docs; missing `04_execution_log.md`. | Minor | **Fix** | **Applied**: Corrected component (`RuleSelection`), test names, and terminology in `05_cleanup_and_audit.md`, created `04_execution_log.md`, and updated `ROADMAP.md`. |
| **A2-6** | Area 2 (Hook) | Duplicate fail-open config wrapper in `src/hook/runner.rs` violating ADR 011 Place 2 helper colocation. | Nit | **Fix** | **Applied**: Consolidated into single shared helper `load_hook_config(project_root, load_fn)` in Place 2. |

---

## 4. Verification Post-Remediation

Following all remediations, the full verification suite was rerun:

- `cargo fmt -- --check`: Clean (0 diffs).
- `cargo clippy --all-targets -- -D warnings`: Clean (0 warnings, 0 errors).
- `cargo test`: All 2,473 tests pass (0 failures).
- `cargo run -- code .`: All checks passed! (0 diagnostics).
