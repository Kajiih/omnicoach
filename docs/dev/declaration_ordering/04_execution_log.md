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
   - When a class or `impl` block defines multiple constructors (`__new__` and `__init__` in Python, or `new` and `with_capacity` in Rust) where `__new__` calls an exclusive helper `_allocate`, placing `["__new__", "__init__", "_allocate"]` keeps all constructors at the top of the type (`constructor-after-method`) while keeping `_allocate` adjacent to the constructor block. `analyze_callable_scope` treats constructors and their exclusive helpers as a unified constructor cluster when checking `uncolocated-helper` contiguity.

---

## 3. Per-Exemption Mutation Check Results

Every named exemption across the 7 rules was verified against a named `pass` test case in `rule_test!`:

| Rule | Exemption | Mutation Applied | Failing `pass` Test Case Confirmed |
| :--- | :--- | :--- | :--- |
| `constructor-after-method` | `E1` (Multiple constructors at top of scope) | Set `seen_non_constructor = true` on constructors | `pass::case_1_constructors_before_methods`, `pass::case_4_constructors_at_top_of_inherent_impl` |
| `constructor-after-method` | `E2` (Python `@overload` constructor grouping) | Disable `continues_overload` deduplication | `pass::case_2_overloaded_init_grouped_at_first_declaration_exempt` |
| `constructor-after-method` | `E3` / `E7` (Trait `impl`, nested classes, separate `impl` blocks) | Flatten scopes across classes / `impl` blocks | `pass::case_3_nested_class_tracks_constructors_independently`, `pass::case_6_trait_impl_and_separate_impl_blocks_exempt` |
| `constructor-after-method` | `E4` & `E5` (Rust private `fn new*`, `&self` methods, non-`Self` return) | Remove `is_exported && !has_self && returns_self_type` | `pass::case_5_private_new_helper_and_self_receiver_new_method_exempt` |
| `constructor-after-method` | `E6` (Rust `#[cfg(test)]` / `#[test]` items in `impl`) | Remove `is_in_rust_inline_test` skip | `pass::case_7_inline_test_before_constructor_exempt` |
| `uncolocated-helper` | Place 1 (immediately after consumer) & Place 2 (trailing helper section) | Reject Place 1 or Place 2 in `is_valid_helper_placement` | `pass::place_1_immediately_after_consumer_and_shared_at_end`, `pass::place_2_all_helpers_at_end_of_scope_allowed`, `pass::inherent_impl_place_1_and_place_2_allowed` |
| `uncolocated-helper` | Public-to-public calls stop root propagation | Propagate roots through public callers | `pass::public_calling_public_keeps_helper_single_use` |
| `uncolocated-helper` | Uncalled private functions (`\|Roots(h)\| == 0`) & constructor clusters | Flag `roots.is_empty()` or disable `is_in_constructor_cluster` | `pass::uncalled_private_and_constructor_clusters_exempt` |
| `uncolocated-helper` | Trait `impl` and `#[cfg(test)]` methods | Include trait `impl`s or remove `is_in_rust_inline_test` skip | `pass::trait_impl_and_inline_test_exempt` |
| `private-before-public-function` | Exclusive helpers colocated after owning entrypoint before next public | Flag any private function before `last_pub` regardless of `Roots(h)` | `pass::colocated_exclusive_helpers_between_public_entrypoints_allowed`, `pass::colocated_units_and_restricted_visibility_allowed` |
| `private-before-public-function` | Python `__dunder__` methods & `@<prop>.setter` / `@overload` grouping | Set `is_dunder = false` or disable property accessor grouping | `pass::shared_helper_below_all_its_public_callers_allowed`, `pass::property_accessors_and_private_only_scope_exempt` |
| `private-before-public-function` | Trait `impl` and `#[cfg(test)]` exempt | Include trait `impl` or remove `is_in_rust_inline_test` skip | `pass::trait_impl_and_inline_test_exempt` |
| `callee-before-caller` | Mutual recursion cycles (Tarjan SCCs) exempt | Remove `scc_id[caller] != scc_id[callee]` guard | `pass::mutual_recursion_scc_cycle_exempt` |
| `callee-before-caller` | Stage 1 & Stage 2 suppression and public-tier calls exempt | Remove `!flagged_p1_or_p2` or `MethodVisibility::Public` guard | `pass::top_down_within_each_unit_and_shared_layer`, `pass::public_callee_before_public_caller_exempt`, `pass::precedence_suppression_avoids_double_flagging_with_p1_and_p2` |
| `callee-before-caller` | Local variable shadowing & constructors exempt | Remove `local_names` check or `!is_constructor` guard | `pass::constructors_and_shadowed_local_variables_exempt` |
| `field-after-method` | `E1` (Unannotated `Stmt::Assign` after `def`) | Flag `Stmt::Assign` when `seen_method` is true | `pass::case_2_unannotated_method_alias_and_property_after_def_exempt` |
| `field-after-method` | `E2` & `E3` (Non-`Expr::Name` target, nested classes, method-local annotations) | Remove `Expr::Name` target guard | `pass::case_3_non_name_target_and_nested_class_exempt` |
| `associated-item-after-method` | `E1` & `E2` (Macro calls in `impl`/`trait` and local items in `fn` body) | Flag `AssocItem::MacroCall` or walk inside `fn` bodies | `pass::case_2_macro_call_and_local_items_inside_fn_exempt` |
| `associated-item-after-method` | `E3` (Separate `impl` blocks and inline `#[cfg(test)]` `fn`) | Remove `is_in_rust_inline_test` skip | `pass::case_3_separate_impl_blocks_and_inline_test_fn_exempt` |
| `statement-after-main-guard` | `E1` & `E2` (Reversed comparison order and guard `else` branch) | Only match `__name__` on left or flag statements inside `orelse` | `fail::case_2_reversed_main_guard_comparison`, `pass::case_1_main_guard_at_end_of_module` |
| `statement-after-main-guard` | `E3` (Non-guard `if` comparisons on `__name__`) | Match any `CmpOp` in `is_main_guard_statement` | `pass::case_2_non_main_name_comparisons_exempt` |
| `statement-after-main-guard` | `E4` (Nested `if __name__ == "__main__":` inside function) | Walk nested function bodies | `pass::case_3_nested_main_guard_inside_function_exempt` |
