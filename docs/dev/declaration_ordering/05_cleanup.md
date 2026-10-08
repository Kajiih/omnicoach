# Phase 5: Consolidation & Cleanup — Declaration Ordering (`Topic::DECLARATION_ORDER`)

> [!IMPORTANT]
> **Status**: **COMPLETED**

---

## 1. Shared Helper Consolidation

Per [rule_batch_playbook.md](../rule_batch_playbook.md) Phase 5, all AST and call-graph extraction helpers introduced for the 7 `Topic::DECLARATION_ORDER` rules were audited for duplication and consolidated:

1. **Memoized 3-Stage Call-Cluster Analyzer (`collect_call_cluster_findings`)**:
   - `private-before-public-function` (Stage 1), `uncolocated-helper` (Stage 2), and `callee-before-caller` (Stage 3) all operate on the same scope-local call graph, Tarjan Strongly Connected Components (`compute_tarjan_scc`), and public-entrypoint reachability sets (`compute_public_roots`).
   - Rather than recomputing the call graph three times per file or duplicating mutual-exclusion logic across three rule modules, `ParsedFile` caches `OnceLock<CachedCallClusterFindings>` via `ast::collect_call_cluster_findings(file)` in [src/code_lint/ast.rs](../../../src/code_lint/ast.rs). Each of the three rules projects its respective vector (`uncolocated_helpers`, `private_before_public`, `callee_before_caller`) in O(1) after the first rule runs.
2. **Shared Python Callable Grouping (`group_python_callables`)**:
   - Both `python::classes::collect_type_method_scopes` and `python::classes::collect_callable_scopes` in [src/code_lint/ast/python/classes.rs](../../../src/code_lint/ast/python/classes.rs) share `group_python_callables` and `python_method_visibility` so `@overload` series and `@<prop>.getter` / `.setter` / `.deleter` accessors are collapsed identically across constructor ordering and call-cluster ordering.
3. **Shared Self-Type Name Extraction in Rust (`impl_self_type_name`)**:
   - `rust::collect_type_method_scopes`, `rust::collect_callable_scopes`, and `rust::collect_associated_items_after_methods` in [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs) share `impl_self_type_name(impl_item, &file.source)` and `is_rust_constructor_name` to identify inherent `impl` target types and constructors consistently.
4. **Orphan Cleanup from Retiring `call-before-definition` and `private-before-public-method`**:
   - Removed `src/code_lint/rules/call_before_definition.rs`, `src/code_lint/rules/private_before_public_method.rs`, and the orphaned `docs/dev/define_before_use/` directory, and updated all references in [ROADMAP.md](../../../ROADMAP.md) and [tests/snapshots/cli__list_rules.snap](../../../tests/snapshots/cli__list_rules.snap).

---

## 2. Verification Gate After Consolidation

- `cargo fmt --check`: **0 diffs**
- `cargo clippy --all-targets -- -D warnings`: **0 warnings**
- `cargo test`: **all unit tests, `tests/architecture_conformance.rs`, `tests/cli.rs` (including `test_self_dogfooding_code_lint`), `tests/registry.rs`, and doctests pass**
- `cargo doc --no-deps --document-private-items`: **0 warnings**
