# Phase 4: Execution Log — Architectural DAG & Conformance

This document records **Phase 4 (Execute)** for `omni`'s architectural specification, compile-time colocation, and structural CST conformance engine.

**Status**: Validated by user — proceeding to **Phase 5 (Clean up)**.

---

## 1. Executed Tasks (`Audit -> RED -> GREEN -> Verify`)

### Task 1: Move Architecture Specification to `src/architecture.rs` & Enable Compile-Time `architecture_component!`
- **RED**: Updated [tests/architecture.rs](../../../tests/architecture.rs) to import `omni::architecture::{ARCHITECTURE_GRAPH, ArchitectureComponent, ComponentDefinition}` and `omni::architecture_graph`. Confirmed `cargo test --test architecture` failed with `E0432`.
- **GREEN**:
  - Created [src/architecture.rs](../../../src/architecture.rs) (`architecture_component!(FoundationPrimitives);`) containing `ComponentDefinition<Component>`, `define_architecture!`, `ArchitectureComponent`, `ARCHITECTURE_GRAPH`, and `ArchitectureComponent::description(&self)`.
  - Updated [src/lib.rs](../../../src/lib.rs) to expose `pub mod architecture;`, `#[macro_export] macro_rules! architecture_component` (expanding to `const _ARCHITECTURE_COMPONENT: $crate::architecture::ArchitectureComponent = $crate::architecture::ArchitectureComponent::$component;`), and `#[macro_export] macro_rules! architecture_graph`.
  - Removed the 3 copy-pasted no-op `macro_rules! architecture_component` stubs from [src/bin/omni-code-lint.rs](../../../src/bin/omni-code-lint.rs), [src/bin/omni-command-lint.rs](../../../src/bin/omni-command-lint.rs), and [src/bin/ast_dumper.rs](../../../src/bin/ast_dumper.rs), replacing them with `omni::architecture_component!(ApplicationBinaries);`.
- **Verify**: `cargo test --test architecture` and `cargo clippy --all-targets -- -D warnings` passed.

### Task 2: Fix Mid-File `#[cfg(test)]` Truncation in `strip_inline_tests`
- **RED**: Added `test_strip_inline_tests_preserves_production_code_after_conditional_test_item` in [tests/architecture.rs](../../../tests/architecture.rs). Confirmed it failed (`got: []`) because the old line-based `strip_inline_tests` truncated the remainder of any file at the first `#[cfg(test)]` line.
- **GREEN**:
  - Added `ParsedFile::rust(source: &str)` in [src/code_lint/ast.rs](../../../src/code_lint/ast.rs).
  - Updated `collect_inline_test_ranges` in [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs) to include each inline test item's preceding `attribute_item` spans.
  - Updated `strip_inline_tests` in [tests/architecture.rs](../../../tests/architecture.rs) to blank out non-newline bytes in `collect_inline_test_ranges` spans while preserving line numbers and all subsequent production items.
- **Verify**: `test_strip_inline_tests_preserves_production_code_after_conditional_test_item` and all 500 library unit tests passed.

### Task 3: Structural CST Extractors for Component Declarations, Single Path, and Relative Imports
- **RED**: Updated `test_second_path_declarations_are_detected` with raw string literals and comments containing `pub use` and `#[macro_export]`, and added `test_structural_extractors_ignore_strings_and_catch_after_conditional_test`. Confirmed failure under the old line-based implementation.
- **GREEN**:
  - Added `collect_second_path_declarations` and `collect_relative_use_declarations` in [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs).
  - Updated `extract_architecture_components`, `second_path_declarations`, and `relative_import_declarations` in [tests/architecture.rs](../../../tests/architecture.rs) to query `ParsedFile::rust` CST nodes outside `collect_inline_test_ranges` in a single parse pass.
  - Enforced exact single declaration (`[component_name]`) in `test_all_source_files_declare_architecture_component`, rejecting both missing (`[]`) and duplicate (`multiple`) declarations.
- **Verify**: Fixed a `clippy::unnecessary_join` warning and renamed `cfg_test` test identifiers to `conditional_test` (caught by `test_self_dogfooding_code_lint`'s `banned-abbreviations` rule).

### Task 4: Sync ADR `decisions/006_architectural_dag_and_conformance.md`
- Updated [decisions/006_architectural_dag_and_conformance.md](../../../decisions/006_architectural_dag_and_conformance.md) to document compile-time `architecture_component!` resolution, `src/architecture.rs`, and the 12 conformance and guard tests in `tests/architecture.rs`.

---

## 2. Verification Summary (`AC1`–`AC8`)

| Check | Command | Result |
| :--- | :--- | :--- |
| Formatting | `cargo fmt --check` | Pass (0 diffs) |
| Clippy (all targets, pedantic + nursery) | `cargo clippy --all-targets -- -D warnings` | Pass (0 warnings) |
| Unit + Integration Tests | `cargo test` | Pass (500 lib + 12 architecture + 14 CLI + 8 registry = 534 passed) |
| Self-Dogfooding Code Lint | `cargo run --bin omni-code-lint` | Pass (0 violations) |
