# Phase 7: Learn — Idiomatic `LazyLock` Caching & Per-File Allocation Cleanups

This document records **Phase 7 (Learn)** for the `LazyLock` and per-file allocation cleanup track.

---

## 1. Key Takeaways

1. **Fast-Path `LazyLock` Caching Beats Custom Deserialization State**:
   - Pre-compiling default glob patterns into a `static DEFAULT_TEST_MATCHER: LazyLock<GlobSet>` and checking `if self.per_file_ignores.is_empty()` captures the hot-path speedup without polluting `Config` or `ContextConfig` with custom `Deserialize` implementations or cached struct fields.

2. **Self-Dogfooding Catches Hungarian Type Suffixes on Statics**:
   - Our `no-hungarian-notation` rule checks `const` and `static` identifiers for banned collection/type suffixes like `_SET` (`DEFAULT_TEST_GLOB_SET` → `DEFAULT_TEST_MATCHER`). Naming statics by their domain role (`MATCHER`, `RULES`, `SOURCES`) is both more idiomatic and compliant with our own lint suite.

3. **`&serde_json::Value` Implements `Deserializer` Directly**:
   - When deserializing from an existing `&serde_json::Value` map entry, `T::deserialize(val)` deserializes in-place without the deep tree clone required by `serde_json::from_value(val.clone())`.

4. **`DoubleEndedIterator::next_back` Avoids Intermediate Line Vecs**:
   - `str::lines()` implements `DoubleEndedIterator`, allowing prefix-all-but-last-line checks (`lines.next_back(); lines.any(...)`) in zero allocations instead of collecting `Vec<&str>` just to compute `lines.len() - 1`.

5. **`#![cfg(test)]` in Integration Tests (`rust-lang/rust-clippy#13981`)**:
   - Clippy's `is_in_test` checks `is_in_test_function || is_in_cfg_test`. Because integration test files (`tests/*.rs`) are compiled as standalone test crates without an enclosing `#[cfg(test)] mod`, top-level helpers and `LazyLock` statics in `tests/*.rs` do not automatically inherit `clippy.toml`'s `allow-*-in-tests` settings. Adding `#![cfg(test)]` at the top of each `tests/*.rs` file marks the crate root with `cfg(test)` so `clippy.toml` remains the single source of truth instead of duplicating `#![allow(clippy::...)]` attributes.

