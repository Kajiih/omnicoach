# Phase 3: Design & Implementation Plan — Idiomatic `LazyLock` Caching & Per-File Allocation Cleanups

This document records **Phase 3 (Design / Plan)** for the `LazyLock` and per-file allocation cleanup track.

---

## 1. File-by-File Design

### Step 1: `src/code_lint/runner.rs` — `SUPPRESSIBLE_RULES` `LazyLock`
- Define a module-level `static SUPPRESSIBLE_RULES: LazyLock<HashSet<&'static str>>`:
  ```rust
  static SUPPRESSIBLE_RULES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
      crate::code_lint::rules::CODE_RULES
          .iter()
          .filter(|rule| !rule.tags().contains(&Tag::Suppression))
          .map(|rule| rule.name().0)
          .collect()
  });
  ```
- In `lint_file`, pass `&SUPPRESSIBLE_RULES` to `tracker.audit(path, config, &SUPPRESSIBLE_RULES)`.

### Step 2: `src/core.rs` — Fast-Path `per_file_ignores`, `DEFAULT_TEST_MATCHER`, and `T::deserialize(val)`
1. **Default Test Patterns & `DEFAULT_TEST_MATCHER`**:
   - Define `const DEFAULT_TEST_PATTERNS: &[&str] = &["**/tests/**", "**/test_*.py", "**/*_test.py", "**/*_test.rs", "**/tests.rs"];` and derive `default_test_patterns()` from it.
   - Define `static DEFAULT_TEST_MATCHER: LazyLock<globset::GlobSet>` built via `GlobSetBuilder` with `literal_separator(false)` over `DEFAULT_TEST_PATTERNS`.
   - In `Config::is_test_path(path)`: if `self.context.test_patterns == DEFAULT_TEST_PATTERNS`, match `&normalized` against `DEFAULT_TEST_MATCHER.is_match(&normalized)`; otherwise fall back to `self.context.test_patterns.iter().any(|pattern| glob_matches(pattern, &normalized))`.
2. **Fast-Path `Config::is_rule_enabled_for_path`**:
   - Right after `if !self.is_rule_enabled(rule) { return false; }`, add:
     ```rust
     if self.per_file_ignores.is_empty() {
         return true;
     }
     ```
     skipping `normalize_path_for_glob(path)` (and `std::env::current_dir()`) whenever `per_file_ignores` is empty.
3. **In-Place `Config::get_rule_config`**:
   - Replace `serde_json::from_value(val.clone()).ok()` with `T::deserialize(val).ok()`.

### Step 3: `src/code_lint/ast/rust.rs` — Zero-Allocation AST Helpers
1. **`traverse_rust` (lines 294–307)**:
   - Bind `let name_range = node.field("name").map(|name_node| { let range = name_node.range(); bindings.push(AstNode::from_raw(name_node)); range });` once before `for child in node.children()`, checking `if name_range.as_ref() == Some(&child.range()) { continue; }`.
2. **`is_enclosed_in_macro` (lines 641–650)**:
   - Borrow `Cow<str>` from `macro_id.text()` instead of `.trim().to_string()`:
     ```rust
     let terminal = macro_terminal_name_raw(&ancestor);
     let full_text = ancestor.field("macro").map(|macro_id| macro_id.text());
     let full_path = full_text.as_deref().map_or("", str::trim);
     if predicate(full_path, terminal.as_ref()) {
         return true;
     }
     ```
3. **`is_multiline_string_literal` (lines 690–695)**:
   - Replace `let lines: Vec<&str> = text.lines().collect();` with:
     ```rust
     let text = node.text();
     let mut lines = text.lines();
     lines.next_back();
     lines.any(|line| !line.trim_end().ends_with('\\'))
     ```

### Step 4: `src/code_lint/ast/python.rs`, `src/code_lint/rules/no_identical_positional_types.rs`, `src/code_lint/suppression.rs`, `src/bin/omni-code-lint.rs`, `tests/registry.rs`, and `ROADMAP.md`
1. **`src/code_lint/ast/python.rs`**: Merge `"list_splat_pattern" | "dictionary_splat_pattern"` in `parse_param_parts`.
2. **`src/code_lint/rules/no_identical_positional_types.rs`**: Change `collect_duplicate_type_groups(params: &[&PythonParameterInfo<'_>])` and drop `.cloned()` in `check_function_signature`.
3. **`src/code_lint/suppression.rs`**: Use `directive.matched_count.get(target_rule).is_none_or(|&count| count == 0)`.
4. **`src/bin/omni-code-lint.rs`**: Change `Cli::paths` to `Vec<PathBuf>` and pass `paths: cli.paths` directly to `LintOptions`.
5. **`tests/registry.rs`**: Cache `static RULE_SOURCES: LazyLock<Vec<(PathBuf, String)>>` and use `path.extension().is_some_and(|extension| extension == "rs")`.
6. **`ROADMAP.md`**: Remove the completed items from Section 2.

---

## 2. Verification Plan

1. Execute Steps 1–4 → verify: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`.
2. Run full test suite → verify: `cargo test` (unit tests, `architecture_conformance`, `cli`, and `registry` suites all pass).
