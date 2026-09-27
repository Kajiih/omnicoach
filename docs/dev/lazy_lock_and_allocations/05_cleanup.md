# Phase 5: Clean Up — Idiomatic `LazyLock` Caching & Per-File Allocation Cleanups

This document records **Phase 5 (Clean up)** for the `LazyLock` and per-file allocation cleanup track.

---

## 1. Hygiene & Verification Checklist

- **No Dead Code or Unused Imports**: Verified via `cargo clippy --all-targets -- -D warnings` (standardized on `#![cfg(test)]` across `tests/*.rs` as the workaround for `rust-lang/rust-clippy#13981` so `clippy.toml` `allow-*-in-tests` applies to module-level helpers and `LazyLock` statics).
- **Formatting**: Verified via `cargo fmt --check`.
- **Self-Dogfooding**: Verified via `cargo test --test cli test_self_dogfooding_code_lint` (renamed `DEFAULT_TEST_GLOB_SET` to `DEFAULT_TEST_MATCHER` to satisfy `no-hungarian-notation`, which bans `_SET` type suffixes).
- **Roadmap Sync**: Completed items under Section 2 of `ROADMAP.md` removed.
