# Phase 4: Execution Log — Declaration Ordering (`Topic::DECLARATION_ORDER`)

> [!IMPORTANT]
> **Status**: **COMPLETED**

---

## 1. Execution Summary

All 7 `Topic::DECLARATION_ORDER` rules and their shared extractors specified in [ADR 011](../../../decisions/011_colocated_abstraction_ordering.md) and [03_design_plan.md](03_design_plan.md) were implemented, self-dogfooded across all 35 source files in `src/`, and verified:

1. **Step 1 — Self-Dogfooding Reorder (`D7`)**:
   - Reordered all 35 Rust source files in `src/` so every module and inherent `impl` scope follows the 2-place helper rule and top-down abstraction order:
     - Single-user private helpers (`|Roots(h)| == 1`) are placed either in **Place 1** (immediately after their owning public entrypoint in vertical-slice modules like `ast.rs`, `config.rs`, and `ast/rust.rs`) or in **Place 2** (in the trailing private helper section after all public entrypoints in public-first scopes like `InterceptedCommand`, `SuppressionTracker`, `CommentIndex`, `test_utils.rs`, and `scopes.rs`).
     - Multi-user private helpers (`|Roots(h)| >= 2`, such as `read_config_file` in `rule_selection.rs`) are placed in **Place 2** in the trailing helper section after all public entrypoints.
     - Among private helpers (`Private -> Private`), callers precede callees (`callee-before-caller`).

2. **Step 2 — Retire `call-before-definition` (`D1`) and `private-before-public-method` (`D5`)**:
   - Deleted `src/code_lint/rules/call_before_definition.rs`, `src/code_lint/rules/private_before_public_method.rs`, and `docs/dev/define_before_use/`.
   - Removed the orphaned bottom-up scope helpers from [src/code_lint/ast/python/scopes.rs](../../../src/code_lint/ast/python/scopes.rs) and added `collect_local_bound_names` for local-shadowing exclusion in Python call-graph extraction.

3. **Step 3 — Shared Cross-Language Type-Method & Call-Cluster Extractors (`collect_type_method_scopes`, `collect_call_cluster_findings`)**:
   - Added `MethodVisibility`, `TypeMethod`, `TypeMethodScope`, `CallableItem`, `CallableScope`, `CallOrderFinding`, `CallClusterFindings`, `collect_type_method_scopes`, and memoized `collect_call_cluster_findings` (`OnceLock<CachedCallClusterFindings>` on `ParsedFile`, backed by `analyze_callable_scope`, `is_valid_helper_placement`, `compute_public_roots`, and `compute_tarjan_scc`) in [src/code_lint/ast.rs](../../../src/code_lint/ast.rs).
   - Implemented the Python extractors in [src/code_lint/ast/python/classes.rs](../../../src/code_lint/ast/python/classes.rs) (`collect_type_method_scopes`, `collect_callable_scopes`, `build_module_callable_scopes`, `build_class_callable_scope`, `group_python_callables`, `python_method_visibility`, `PYTHON_CONSTRUCTOR_NAMES`), grouping `@overload` signatures and `@<prop>.getter` / `.setter` / `.deleter` accessors with their first definition, bridging top-level function references through module-level classes, and splitting scopes when a function name is redefined.
   - Implemented the Rust extractors in [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs) (`collect_type_method_scopes`, `collect_callable_scopes`, `build_rust_module_callable_scopes`, `collect_rust_impl_callees`, `build_rust_impl_callable_scope`, `collect_rust_impl_methods`, `impl_self_type_name`, `is_rust_constructor_name`), extracting non-test `fn` items from modules and inherent `impl` blocks, bridging module function references through `impl` blocks and macro `token_tree`s, and splitting scopes on redefined names.

4. **Step 4 — Python Class Field & Main-Guard Extractors**:
   - Added `PythonFieldAfterMethod` and `collect_fields_after_methods` to [src/code_lint/ast/python/classes.rs](../../../src/code_lint/ast/python/classes.rs) and re-exported them from [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs).
   - Added `collect_statements_after_main_guard` and `is_main_guard_statement` to [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs).

5. **Step 5 — Rust Associated Item Extractor**:
   - Added `RustAssociatedItemAfterMethod` and `collect_associated_items_after_methods` to [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs).

6. **Step 6 — Implement and Register the 7 `Topic::DECLARATION_ORDER` Rules**:
   - [src/code_lint/rules/field_after_method.rs](../../../src/code_lint/rules/field_after_method.rs) (`field-after-method`, Python).
   - [src/code_lint/rules/associated_item_after_method.rs](../../../src/code_lint/rules/associated_item_after_method.rs) (`associated-item-after-method`, Rust).
   - [src/code_lint/rules/constructor_after_method.rs](../../../src/code_lint/rules/constructor_after_method.rs) (`constructor-after-method`, Python + Rust).
   - [src/code_lint/rules/uncolocated_helper.rs](../../../src/code_lint/rules/uncolocated_helper.rs) (`uncolocated-helper`, Python + Rust).
   - [src/code_lint/rules/private_before_public_function.rs](../../../src/code_lint/rules/private_before_public_function.rs) (`private-before-public-function`, Python + Rust).
   - [src/code_lint/rules/callee_before_caller.rs](../../../src/code_lint/rules/callee_before_caller.rs) (`callee-before-caller`, Python + Rust).
   - [src/code_lint/rules/statement_after_main_guard.rs](../../../src/code_lint/rules/statement_after_main_guard.rs) (`statement-after-main-guard`, Python).
   - Registered all 7 rules in `CODE_RULES` in [src/code_lint/rules.rs](../../../src/code_lint/rules.rs).
   - Updated [tests/snapshots/cli__list_rules.snap](../../../tests/snapshots/cli__list_rules.snap) and [ROADMAP.md](../../../ROADMAP.md).

---

## 2. Deviations & Adjustments During Execution

1. **Per-method local-name filtering when bridging Rust `impl` blocks (`collect_rust_impl_callees`)**:
   - In Rust modules, a public entrypoint frequently delegates AST/CST traversal to a helper struct with an `impl` block in the same module (`LiteralOccurrenceCollector`, `ParameterUseVisitor`, `BindingVisitor`). Bridging module-level function calls through `impl` blocks ensures those helpers are recognized as belonging to the entrypoint's component unit.
   - However, scanning the entire `impl` syntax node with an empty `local_names` set caused a closure parameter (`|collection_type|` in `PythonCollectionType::joined_paths` inside `annotations.rs`) to be mistaken for a reference to `pub fn collection_type`. Collecting `collect_rust_fn_local_names(&method)` per `ast::AssocItem::Fn` inside `collect_rust_impl_callees` eliminated the false reference while preserving visitor-struct call bridging.
2. **Scope segmentation on redefined function names (`rule_test!` compatibility)**:
   - Because `assert_rule_fail` in [src/test_utils.rs](../../../src/test_utils.rs) repeats top-level `fail` snippets twice in one file (`format!("{code}\n{second_copy}")`), both `build_module_callable_scopes` (Python) and `build_rust_module_callable_scopes` (Rust) start a new `CallableScope` segment whenever a non-overload, non-property function name is redefined in the same module. This isolates repeated test copies cleanly and prevents shadowed module-level functions from corrupting call-graph edges.
3. **Constructor-cluster contiguity in `analyze_callable_scope`**:
   - When a class or `impl` block defines multiple constructors (`__new__` and `__init__` in Python, or `new` and `try_new` in Rust) where `__new__` calls an exclusive helper `_allocate`, placing `["__new__", "__init__", "_allocate"]` keeps all constructors at the top of the type (`constructor-after-method`) while keeping `_allocate` adjacent to the constructor block. `analyze_callable_scope` treats constructors and their exclusive helpers as a unified constructor cluster when checking `uncolocated-helper` contiguity.

---

## 3. Per-Exemption Mutation Check Results

Each named exemption has one dedicated case, and [scripts/exemption_mutations.py](../../../scripts/exemption_mutations.py) maps each one to the source edit that disables it. Run it from the repository root; it reports any case that still passes without its exemption, any listed case that no longer exists, and any `pass` case of these rules it does not cover. It replaces the hand-written table this section held: that table cited cases that did not exist, and re-running the mutations found 7 cases that still passed with their exemption removed (now fixed).

Rust trait `impl` blocks are also skipped by `constructor-after-method`, `uncolocated-helper` and `private-before-public-function`, but no case can observe it there: trait `impl` methods have no visibility, so they are neither constructors nor public callers. The former cases claiming to test it passed with the skip removed and were deleted.
