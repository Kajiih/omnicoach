# Phase 4: Execution Log — Idiomatic `LazyLock` Caching & Per-File Allocation Cleanups

This document records **Phase 4 (Execute)** for the `LazyLock` and per-file allocation cleanup track.

---

## 1. Changes Executed

1. **[src/code_lint/runner.rs](../../../../src/code_lint/runner.rs)**:
   - Extracted `static SUPPRESSIBLE_RULES: std::sync::LazyLock<HashSet<&'static str>>` so `lint_file` never rebuilds the set of suppressible rule names on each file.
2. **[src/core.rs](../../../../src/core.rs)**:
   - Added `DEFAULT_TEST_PATTERNS: &[&str]` and `static DEFAULT_TEST_MATCHER: std::sync::LazyLock<globset::GlobSet>` so `Config::is_test_path` matches the default test patterns with a single pre-compiled `GlobSet` automaton.
   - Added an early `if self.per_file_ignores.is_empty() { return true; }` guard in `Config::is_rule_enabled_for_path`, avoiding ~48 `normalize_path_for_glob` calls (and `std::env::current_dir()` syscalls) per file when `per_file_ignores` is empty.
   - Updated `Config::get_rule_config` to deserialize in-place from `&serde_json::Value` via `T::deserialize(val).ok()` instead of cloning `val`.
3. **[src/code_lint/ast/rust.rs](../../../../src/code_lint/ast/rust.rs)**:
   - Hoisted `node.field("name")` out of the `for child in node.children()` loop in `traverse_rust`.
   - Borrowed `Cow<str>` in `is_enclosed_in_macro` instead of allocating `.trim().to_string()`.
   - Replaced `let lines: Vec<&str> = text.lines().collect();` in `is_multiline_string_literal` with `let mut lines = text.lines(); lines.next_back();`.
4. **[src/code_lint/ast/python.rs](../../../../src/code_lint/ast/python.rs), [src/code_lint/rules/no_identical_positional_types.rs](../../../../src/code_lint/rules/no_identical_positional_types.rs), [src/code_lint/suppression.rs](../../../../src/code_lint/suppression.rs), [src/bin/omni-code-lint.rs](../../../../src/bin/omni-code-lint.rs), [tests/registry.rs](../../../../tests/registry.rs), [ROADMAP.md](../../../../ROADMAP.md)**:
   - Merged `"list_splat_pattern" | "dictionary_splat_pattern"` in `parse_param_parts`.
   - Passed `&[&PythonParameterInfo<'_>]` to `collect_duplicate_type_groups`, dropping `.cloned()` before the `min_args` check.
   - Used `.is_none_or(|&count| count == 0)` in `audit_directive`.
   - Typed `Cli::paths` directly as `Vec<PathBuf>` in `src/bin/omni-code-lint.rs`.
   - Cached `static RULE_SOURCES: LazyLock<Vec<(PathBuf, String)>>` in `tests/registry.rs`.
   - Pruned the completed items from `ROADMAP.md`.
