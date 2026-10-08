# Phase 5: Consolidation & Cleanup — Declaration Ordering (`Topic::DECLARATION_ORDER`)

> [!IMPORTANT]
> **Status**: **COMPLETED**

---

## 1. Shared Helper Consolidation

Per [rule_batch_playbook.md](../rule_batch_playbook.md) Phase 5, all AST extraction helpers introduced for the 5 `Topic::DECLARATION_ORDER` rules were audited for duplication and consolidated before implementation:

1. **Unified Type-Method Scope Representation (`collect_type_method_scopes`)**:
   - Both `constructor-after-method` and `private-before-public-method` inspect the ordered sequence of direct methods in a Python `class` or Rust inherent `impl` block.
   - Rather than duplicating class/`impl` traversal, `@overload` / `@<prop>.setter` deduplication, inline-test filtering, and self-type name extraction across two helpers, both rules share `ast::collect_type_method_scopes(file) -> Vec<TypeMethodScope<'_>>` ([src/code_lint/ast.rs](../../../src/code_lint/ast.rs)), which delegates to:
     - `python::classes::collect_type_method_scopes` ([src/code_lint/ast/python/classes.rs](../../../src/code_lint/ast/python/classes.rs)), reusing existing `direct_function_definitions` and `extract_decorators_from_slice`.
     - `rust::collect_type_method_scopes` ([src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs)), reusing `ParsedFile::is_in_rust_inline_test` and `span_from_rowan_range`.
2. **Shared Self-Type Name Extraction in Rust (`impl_self_type_name`)**:
   - Both `rust::collect_type_method_scopes` and `rust::collect_associated_items_after_methods` in [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs) share `impl_self_type_name(impl_item, &file.source)` to extract the terminal identifier of an `impl` target type (`Client` from `impl<T> crate::Client<T>`) with a fallback to the trimmed type slice for non-path types.
3. **Orphan Cleanup from Retiring `call-before-definition`**:
   - Removed `PythonFunctionScope`, `PythonFunctionDef`, `PythonLocalCall`, `collect_function_scopes`, `partition_scope_epochs`, and their private helpers from [src/code_lint/ast/python/scopes.rs](../../../src/code_lint/ast/python/scopes.rs) (~280 lines of dead code removed).
   - Removed the orphaned `docs/dev/define_before_use/` directory and updated all references in [ROADMAP.md](../../../ROADMAP.md).

---

## 2. Verification Gate After Consolidation

- `cargo fmt --check`: **0 diffs**
- `cargo clippy --all-targets -- -D warnings`: **0 warnings**
- `cargo test`: **all unit tests, `tests/architecture_conformance.rs`, `tests/cli.rs` (including `test_self_dogfooding_code_lint`), `tests/registry.rs`, and doctests pass**
- `cargo doc --no-deps --document-private-items`: **0 warnings**
