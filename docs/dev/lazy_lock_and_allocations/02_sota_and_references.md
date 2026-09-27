# Phase 2: SOTA & References — Idiomatic `LazyLock` Caching & Per-File Allocation Cleanups

This document records **Phase 2 (Gather Resources and Reference)** for the `LazyLock` and per-file allocation cleanup track.

---

## 1. Standard Library & Crate References

1. **`std::sync::LazyLock` (Rust 1.80+ Standard Library)**:
   - Replaces runtime per-call construction of static sets/tables with a thread-safe, once-initialized static that dereferences (`Deref`) transparently to `&T`.
   - Applicable to:
     - `SUPPRESSIBLE_RULES: LazyLock<HashSet<&'static str>>` in `src/code_lint/runner.rs`
     - `DEFAULT_TEST_MATCHER: LazyLock<globset::GlobSet>` in `src/core.rs`
     - `RULE_SOURCES: LazyLock<Vec<(PathBuf, String)>>` in `tests/registry.rs`

2. **`globset::GlobSet` (`globset` crate, already in `Cargo.toml`)**:
   - `globset::GlobSetBuilder` compiles multiple `Glob` patterns into a single combined regex/Aho-Corasick automaton (`GlobSet`), matching a path against all patterns simultaneously in a single pass rather than compiling a new `GlobMatcher` per pattern on every call.

3. **`serde::Deserialize` on `&serde_json::Value` (`serde_json` crate)**:
   - `&serde_json::Value` implements `serde::Deserializer<'de>` directly, so `T::deserialize(val)` deserializes `T` from a borrowed `&Value` without cloning the `Value` tree first (`serde_json::from_value(val.clone())`).

4. **`DoubleEndedIterator::next_back` on `std::str::Lines`**:
   - `str::lines()` implements `DoubleEndedIterator`, allowing the trailing line of a multiline string literal to be dropped in $O(1)$ via `lines.next_back()` without collecting all lines into a heap-allocated `Vec<&str>`.
