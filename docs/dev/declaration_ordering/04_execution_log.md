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

Each named exemption has one dedicated `pass` case in `rule_test!` whose name states the exemption. Cases are cited without the generated `case_N_` prefix, which shifts whenever a case is added. **Re-run** marks the mutations re-run by the lead's mutation harness after the one-exemption-per-case split (every listed case failed under its mutation); the other rows were checked by hand during Phase 4 and not re-run.

| Rule | Exemption | Mutation applied | Dedicated `pass` case | Re-run |
| :--- | :--- | :--- | :--- | :--- |
| `constructor-after-method` | `E1` Several constructors at the top | Set `seen_non_constructor = true` on constructors | `constructors_before_methods`, `constructors_at_top_of_inherent_impl` | |
| `constructor-after-method` | `E2` Python `@overload` constructor grouped at its first declaration | Disable overload / property grouping | `overloaded_init_grouped_at_first_declaration_exempt` | ✓ |
| `constructor-after-method` | `E3` Nested class tracked independently | Flatten nested class scopes | `nested_class_tracks_constructors_independently` | |
| `constructor-after-method` | `E7` Separate `impl` blocks checked independently | Merge `impl` blocks of one type | `separate_impl_blocks_checked_independently` | |
| `constructor-after-method` | `E4` Rust private `fn new*` | Ignore `is_exported` | `private_new_fn_exempt` | ✓ |
| `constructor-after-method` | `E5` Rust `new*` with a `self` receiver | Drop `!has_self` | `self_receiver_new_method_exempt` | ✓ |
| `constructor-after-method` | `E5` Rust `new*` not returning `Self` | Drop `returns_self_type` | `new_fn_not_returning_self_exempt` | ✓ |
| `constructor-after-method` | `E6` Rust `#[cfg(test)]` / `#[test]` items | `is_in_rust_inline_test` always false | `inline_test_before_constructor_exempt` | ✓ |
| `uncolocated-helper` | Place 1: right after its only public caller | `is_just_after_owner = false` | `helper_right_after_its_only_caller_allowed` (Python, Rust) | ✓ |
| `uncolocated-helper` | Place 2: shared helper in the trailing section | Shared helpers never valid | `shared_helper_in_trailing_section_allowed` (Python, Rust) | ✓ |
| `uncolocated-helper` | Place 2: single-caller helper in the trailing section | `is_at_scope_end = false` | `all_private_helpers_at_end_of_scope` | ✓ |
| `uncolocated-helper` | A second constructor keeps Place 2 valid (`C1` / review `C9`) | Restore the pre-fix `is_at_scope_end` predicate | `second_constructor_keeps_trailing_section_valid` (Python, Rust) | ✓ |
| `uncolocated-helper` | Constructor helper after the constructor cluster | Drop the constructor clause of `in_owner_cluster` | `constructor_helper_after_constructor_cluster_allowed` | ✓ |
| `uncolocated-helper` | A public caller of a public function does not share its helpers | Continue the caller walk through public callers | `public_caller_of_public_function_does_not_share_its_helpers` | ✓ |
| `uncolocated-helper` | Private function no public entrypoint reaches | Report it as `uncolocated-helper` | `uncalled_private_function_exempt` | ✓ |
| `uncolocated-helper` | Python method-local binding does not reference a module function (review `C10`) | Ignore local names | `class_method_local_binding_does_not_bridge_module_helper` | ✓ |
| `uncolocated-helper` | Rust `#[cfg(test)]` / `#[test]` items | `is_in_rust_inline_test` always false | `inline_test_fn_exempt` | ✓ |
| `private-before-public-function` | Helper below its only public caller, before the next public function | Flag any private function before the last public one | `colocated_exclusive_helpers_between_public_entrypoints_allowed` (Python), `colocated_exclusive_helper_between_public_methods_allowed` (Rust) | ✓ |
| `private-before-public-function` | Python dunder methods are public | Treat dunders as private | `dunder_method_is_public` | ✓ |
| `private-before-public-function` | Rust `pub(crate)` / `pub(super)` are public | Only bare `pub` is public | `restricted_visibility_is_public` | ✓ |
| `private-before-public-function` | Python `@property` accessors and `@overload` grouped at their first definition | Disable grouping | `property_accessors_and_overloads_grouped_at_first_definition` | ✓ |
| `private-before-public-function` | Rust `#[cfg(test)]` / `#[test]` items | `is_in_rust_inline_test` always false | `inline_test_fn_exempt` | ✓ |
| `callee-before-caller` | Mutual recursion (same strongly connected component) | Drop the `scc_id` guard | `mutual_recursion_exempt` (Python, Rust) | ✓ |
| `callee-before-caller` | Function already flagged by an earlier stage | Drop the `flagged_p1_or_p2[callee]` guard | `function_flagged_by_earlier_stage_not_reported_again` | ✓ |
| `callee-before-caller` | Calls involving a public function | Drop both visibility guards | `public_callee_before_public_caller_exempt` | ✓ |
| `callee-before-caller` | Local binding shadowing a sibling function | Ignore local names | `local_binding_shadowing_exempt` (Python, Rust) | ✓ |
| `callee-before-caller` | Rust trait `impl` blocks | Build scopes for trait `impl` blocks | `trait_impl_exempt` | ✓ |
| `callee-before-caller` | Rust `self.field` inside a macro is not a method call | Drop the `(` check | `macro_field_access_is_not_a_method_call` | ✓ |
| `field-after-method` | `E1` Unannotated assignment after a method | Flag `Stmt::Assign` after a method | `unannotated_assignment_after_method_exempt` | |
| `field-after-method` | `E2` Method-local annotation | Walk method bodies | `method_local_annotation_exempt` | |
| `field-after-method` | `E2` Non-name annotation target | Drop the `Expr::Name` target guard | `attribute_target_annotation_exempt` | |
| `field-after-method` | `E3` Nested class checked independently | Share `seen_method` with nested classes | `nested_class_checked_independently` | |
| `associated-item-after-method` | `E1` Macro call after a `fn` | Flag `AssocItem::MacroCall` | `macro_call_after_fn_exempt` | |
| `associated-item-after-method` | `E2` Local items inside a `fn` body | Walk `fn` bodies | `local_items_inside_fn_exempt` | |
| `associated-item-after-method` | `E3` Separate `impl` blocks checked independently | Merge `impl` blocks of one type | `separate_impl_blocks_checked_independently` | |
| `associated-item-after-method` | `E3` Rust `#[cfg(test)]` `fn` | `is_in_rust_inline_test` always false | `inline_test_fn_exempt` | ✓ |
| `statement-after-main-guard` | `E1` Reversed comparison `"__main__" == __name__` | Only match `__name__` on the left | `reversed_main_guard_comparison` (`fail` case) | |
| `statement-after-main-guard` | `E2` Guard `else` branch | Flag statements in `orelse` | `guard_else_branch_exempt` | |
| `statement-after-main-guard` | `E2` Subsequent main guards | Drop the guard filter | `subsequent_main_guard_exempt` | ✓ |
| `statement-after-main-guard` | `E3` Non-guard comparisons on `__name__` | Match any `CmpOp` | `non_main_name_comparisons_exempt` | |
| `statement-after-main-guard` | `E4` Nested guard inside a function | Walk nested function bodies | `nested_main_guard_inside_function_exempt` | |

Rust trait `impl` blocks are also skipped by `constructor-after-method`, `uncolocated-helper` and `private-before-public-function`, but no case can observe it there: trait `impl` methods have no visibility, so they are neither constructors nor public callers. The former cases claiming to test it passed with the skip removed and were deleted.
