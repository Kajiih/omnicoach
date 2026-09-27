# Phase 6: Review and Audit — Idiomatic `LazyLock` Caching & Per-File Allocation Cleanups

This document records **Phase 6 (Review and Audit)** for the `LazyLock` and per-file allocation cleanup track.

---

## 1. RICR & Simplicity Audit

1. **`src/code_lint/runner.rs` (`SUPPRESSIBLE_RULES`)**:
   - **Audit**: `static SUPPRESSIBLE_RULES: std::sync::LazyLock<HashSet<&'static str>>` is initialized once on first `lint_file` call and shared across all parallel rayon threads via `Deref`.
   - **Disposition**: Sound. Eliminates ~48-element `HashSet` allocation on every linted file.

2. **`src/core.rs` (`DEFAULT_TEST_PATTERNS`, `DEFAULT_TEST_MATCHER`, `per_file_ignores` fast-path, `T::deserialize(val)`)**:
   - **Audit**:
     - `DEFAULT_TEST_MATCHER` compiles all 5 default test globs with `literal_separator(false)` into a single `GlobSet` automaton once, matching `glob_matches` semantics exactly while avoiding 5 `GlobMatcher` compilations per file when default test patterns are used.
     - `Config::is_rule_enabled_for_path` short-circuits when `self.per_file_ignores.is_empty()`, avoiding ~48 `normalize_path_for_glob` calls (and `std::env::current_dir()` syscalls) per file.
     - `Config::get_rule_config` uses `T::deserialize(val).ok()`, eliminating deep `serde_json::Value` clones while keeping `Config` and `ContextConfig` as plain public-field structs.
   - **Disposition**: Sound.

3. **`src/code_lint/ast/rust.rs`, `src/code_lint/ast/python.rs`, `src/code_lint/rules/no_identical_positional_types.rs`, `src/code_lint/suppression.rs`, `src/bin/omni-code-lint.rs`**:
   - **Audit**:
     - Hoisted `node.field("name")` out of the child loop in `traverse_rust`.
     - Replaced `.trim().to_string()` allocation in `is_enclosed_in_macro` with borrowed `Cow<str>` deref (`full_text.as_deref().map_or("", str::trim)`).
     - Replaced `Vec<&str>` collection in `is_multiline_string_literal` with `DoubleEndedIterator::next_back()`.
     - Merged identical `"list_splat_pattern" | "dictionary_splat_pattern"` arms in `src/code_lint/ast/python.rs`.
     - Passed `&[&PythonParameterInfo<'_>]` to `collect_duplicate_type_groups`, avoiding `PythonParameterInfo` string clones before the `min_args` check.
     - Used `.is_none_or(|&count| count == 0)` in `src/code_lint/suppression.rs` and `Vec<PathBuf>` in `src/bin/omni-code-lint.rs`.
   - **Disposition**: Sound.

4. **`tests/registry.rs` (`RULE_SOURCES`)**:
   - **Audit**: During review, simplified the `.map(...).chain(std::iter::once(...))` iterator adapter in `test_rule_sources_do_not_sort_diagnostics` to `RULE_SOURCES.iter().chain([&suppression_entry])`.
   - **Disposition**: Applied and verified (`tests/registry.rs` executes in `0.01s`).
