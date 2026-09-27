# Phase 5: Clean Up — Native CST Conformance Engine & Idiomatic Rust Policies

This document records **Phase 5 (Clean up)** for the native CST conformance engine cycle.

**Status**: Validated — proceeding to **Phase 6 (Review and Audit)**.

---

## 1. Dead Code & Dependency Removal

1. **Removed External Dev-Dependency (`Cargo.toml` / `Cargo.lock`)**:
   - Removed `rust_arkitect = "0.3.7"` from `Cargo.toml`, dropping `rust_arkitect` and its transitive dev-dependencies (`colored`, `env_logger`, `log`, and test-target `syn` traversal) from the dependency graph.
2. **Removed Obsolete CST Helpers (`src/code_lint/ast/rust.rs`)**:
   - Deleted 4 single-purpose test helpers superseded by `summarize_rust_file`:
     - `collect_second_path_declarations`
     - `collect_relative_use_declarations`
     - `collect_external_mod_declarations`
     - `collect_non_namespace_items`
   - Deleted `has_conditional_test_attribute` (superseded by forward-pass attribute tracking in `collect_inline_test_ranges_rec`).
3. **Removed `rust_arkitect` Adapter Glue (`tests/architecture_conformance.rs`)**:
   - Deleted `CRATE_NAME`, `logical_path`, `logical_path_for_file`, `dependency_spellings`, `must_not_depend_on`, `Except`, `strip_inline_tests` (byte-blanking hack), `is_in_production_code`, `extract_architecture_components`, `external_mod_declarations`, `non_namespace_items`, `second_path_declarations`, and `relative_import_declarations`.

---

## 2. Documentation & Roadmap Alignment

1. **Updated Developer Guide (`docs/dev/rule_design_guide.md` §7)**:
   - Replaced stale references to `tests/architecture.rs (rust_arkitect)`, the old 7-layer table (`code_lint::{bindings,calls,comments}`), and the blanket re-export ban with the current `ArchitectureComponent` DAG, `code_lint::semantic`, private-child facade re-exports, intra-component relative paths, and `tests/architecture_conformance.rs`.
2. **Updated ADR 006 (`decisions/006_architectural_dag_and_conformance.md`)**:
   - Documented `summarize_rust_file`, `OnceLock` summary caching, intra-component `super::`/`self::` resolution, and private-child `pub use` facades.
3. **Pruned Resolved Items from `ROADMAP.md`**:
   - Removed *Domain Router Classification vs. CST Content Verification*, *Architecture Test Parse Caching*, and *Architecture Conformance Watch List (`Inline super:: paths`)*.
