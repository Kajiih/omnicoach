# Phase 1: Understand — Idiomatic `LazyLock` Caching & Per-File Allocation Cleanups

This document records **Phase 1 (Understand)** for the roadmap track **Idiomatic `LazyLock` Caching & Per-File Allocation Cleanups**.

---

## 1. Problem Statement & Scope

Following the `LazyLock` and CST summary improvements in `tests/architecture_conformance.rs`, a codebase audit surfaced several places in `src/` and `tests/` where static data structures are rebuilt on every linted file, OS syscalls/glob compilations are repeated unnecessarily, or pre-modern Rust patterns allocate intermediate collections that are immediately discarded.

### Target Scope (In-Scope Items)

1. **`SUPPRESSIBLE_RULES` `LazyLock` (`src/code_lint/runner.rs`)**:
   - `lint_file` currently rebuilds `let suppressible_rules: HashSet<&'static str> = CODE_RULES.iter().filter(...).map(...).collect();` on every linted file.
   - Replace with a module-level `static SUPPRESSIBLE_RULES: LazyLock<HashSet<&'static str>>`.
2. **Fast-Path `per_file_ignores` & `DEFAULT_TEST_MATCHER` `LazyLock` (`src/core.rs`)**:
   - `Config::is_rule_enabled_for_path` unconditionally calls `normalize_path_for_glob(path)` (which invokes `std::env::current_dir()` when `path.is_absolute()`) ~48 times per file even when `self.per_file_ignores.is_empty()` (the default). Add an early `if self.per_file_ignores.is_empty() { return true; }` guard.
   - `Config::is_test_path` recompiles up to 5 `globset::GlobMatcher` automatons on every file. Cache the default test patterns in a `static DEFAULT_TEST_MATCHER: LazyLock<globset::GlobSet>` (used whenever `self.context.test_patterns` matches `DEFAULT_TEST_PATTERNS`) while keeping `Config` and `ContextConfig` as plain public-field structs with no custom `Deserialize` boilerplate.
3. **In-Place `serde_json::Value` Deserialization (`src/core.rs`)**:
   - In `Config::get_rule_config`, replace `serde_json::from_value(val.clone()).ok()` with `T::deserialize(val).ok()`, eliminating deep `serde_json::Value` cloning on every rule config lookup.
4. **Zero-Allocation AST & Rule Cleanups (`src/code_lint/`, `src/bin/`, `tests/`)**:
   - `src/code_lint/ast/rust.rs`:
     - `is_multiline_string_literal`: Use `DoubleEndedIterator::next_back` on `text.lines()` instead of allocating `Vec<&str>`.
     - `is_enclosed_in_macro`: Borrow `Cow<str>` from `macro_id.text()` instead of allocating `.trim().to_string()`.
     - `traverse_rust`: Hoist `node.field("name")` before the `for child in node.children()` loop instead of re-querying FFI on every child node.
   - `src/code_lint/ast/python.rs`:
     - Merge identical `"list_splat_pattern"` and `"dictionary_splat_pattern"` match arms in `parse_param_parts`.
   - `src/code_lint/rules/no_identical_positional_types.rs`:
     - Collect borrowed `Vec<&PythonParameterInfo<'_>>` instead of `.cloned().collect()` before the `positional_params.len() < min_args` threshold check.
   - `src/code_lint/suppression.rs`:
     - Replace `.get(target_rule).copied().unwrap_or(0) == 0` with `.get(target_rule).is_none_or(|&count| count == 0)`.
   - `src/bin/omni-code-lint.rs`:
     - Type `Cli::paths` directly as `Vec<PathBuf>` instead of `Vec<String>` + `.into_iter().map(PathBuf::from).collect()`.
   - `tests/registry.rs`:
     - Share a `static RULE_SOURCES: LazyLock<Vec<(PathBuf, String)>>` across `test_rule_sources_do_not_sort_diagnostics` and `test_rule_files_use_rule_test` and use `path.extension().is_some_and(|ext| ext == "rs")`.

---

## 2. Explicit Non-Goals (Pushing Back on Overcomplication — Rule 1 & Rule 2)

- **No Global Cache or Signature Churn for `FilterListDefaults::resolve_default_for_lang`**:
  - While `effective_banned_set` / `effective_allowed_set` allocate a small `HashSet<String>` per file, caching `FilterListDefaults` globally would require either a global lock (`Mutex<HashMap<usize, HashSet<String>>>` keyed by pointer address) or changing `Rule::effective_banned_set` / `effective_allowed_set` and all call-matching helpers to `Cow<'static, HashSet<String>>` or two-level lookups.
  - Because `get_rule_config` already short-circuits when `!self.rules.contains_key(rule_name)` (`HashMap::get` returns `None`), keeping `effective_banned_set -> HashSet<String>` simple and lock-free avoids overcomplication.

---

## 3. Success Criteria

1. All 6 in-scope improvements are implemented cleanly with zero public API breakage.
2. `ROADMAP.md` is updated to prune the completed items.
3. Full verification (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`) passes with 0 warnings/errors.
