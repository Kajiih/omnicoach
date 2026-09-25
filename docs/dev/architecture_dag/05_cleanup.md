# Phase 5: Clean Up — Architectural DAG & Conformance

This document records **Phase 5 (Clean up)** for `omni`'s architectural specification and conformance engine.

**Status**: Validated by user — proceeding to **Phase 6 (Review and Audit)**.

---

## 1. Clean-Up Actions Performed

1. **Removed Temporary Scratch Artifacts**:
   - Deleted temporary `scratch/test_strum_doc.rs` created during initial `strum::EnumMessage` investigation.
   - Verified no leftover `scratch/pocs/` or `tests/poc*` files exist in the repository working tree.

2. **Removed Legacy String Heuristics & Duplicate Stubs**:
   - Removed the 3 duplicate `macro_rules! architecture_component { ($component:ident) => {}; }` definitions from [src/bin/omni-code-lint.rs](../../../src/bin/omni-code-lint.rs), [src/bin/omni-command-lint.rs](../../../src/bin/omni-command-lint.rs), and [src/bin/ast_dumper.rs](../../../src/bin/ast_dumper.rs).
   - Removed the legacy line-based `declares_own_macro_path` and `extract_architecture_component` functions from [tests/architecture.rs](../../../tests/architecture.rs).

3. **Eliminated Redundant Parsing & Unified Naming (`D1`)**:
   - Updated `extract_architecture_components`, `second_path_declarations`, and `relative_import_declarations` in [tests/architecture.rs](../../../tests/architecture.rs) to parse each source string once via `ParsedFile::rust(source)` and filter by `is_in_production_code` rather than re-parsing after `strip_inline_tests`.
   - Cached `discover_module_components()` using `std::sync::OnceLock` so `architecture_conformance_rules()` does not re-walk and re-parse `src/` across multiple tests.
   - Replaced all remaining abbreviated local variable names (`vis`, `arg`, `def`, `dep`, `deps`, `rel`, `decl`, `desc`, `v`) in [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs) and [tests/architecture.rs](../../../tests/architecture.rs) with full, self-explanatory domain words (`visibility`, `argument`, `definition`, `dependency`, `dependencies`, `relative_path`, `declaration`, `description`, `violation`).

---

## 2. Verification Status
- `cargo fmt --check`: Pass
- `cargo clippy --all-targets -- -D warnings`: Pass
- `cargo test --test architecture`: Pass (12 passed)
- `cargo run --bin omni-code-lint`: Pass (0 violations)
