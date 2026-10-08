# Phase 4: Execution Log — Declaration Ordering (`Topic::DECLARATION_ORDER`)

> [!IMPORTANT]
> **Status**: **COMPLETED**

---

## 1. Execution Summary

All 7 implementation steps from [03_design_plan.md](03_design_plan.md) were executed and verified:

1. **Step 1 — Self-Dogfooding Reorder (`D6`)**:
   - Reordered the 3 private inherent `impl` methods in Omni's own repository so they follow all `pub` / `pub(crate)` methods in their enclosing `impl` blocks:
     - `LiteralValue::float` in [src/code_lint/ast.rs](../../../src/code_lint/ast.rs) moved below `LiteralValue::negated`.
     - `SuppressionTracker::parse_comment_text` in [src/code_lint/suppression.rs](../../../src/code_lint/suppression.rs) moved below `SuppressionTracker::audit`.
     - `InterceptedCommand::parse_single` in [src/command_lint/command.rs](../../../src/command_lint/command.rs) moved below `InterceptedCommand::program_base_name`.

2. **Step 2 — Retire `call-before-definition` (`D1`)**:
   - Deleted `src/code_lint/rules/call_before_definition.rs` and `docs/dev/define_before_use/`.
   - Removed the orphaned `PythonFunctionScope`, `PythonFunctionDef`, `PythonLocalCall`, `collect_function_scopes`, and `partition_scope_epochs` helpers from [src/code_lint/ast/python/scopes.rs](../../../src/code_lint/ast/python/scopes.rs), its re-exports from [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs), and its unit tests.

3. **Step 3 — Shared Cross-Language Type-Method Scope Extractor (`collect_type_method_scopes`)**:
   - Added `MethodVisibility`, `TypeMethod`, `TypeMethodScope`, and `collect_type_method_scopes` to [src/code_lint/ast.rs](../../../src/code_lint/ast.rs).
   - Implemented the Python extractor in [src/code_lint/ast/python/classes.rs](../../../src/code_lint/ast/python/classes.rs) (`collect_type_method_scopes`, `collect_class_methods`, `python_method_visibility`, `PYTHON_CONSTRUCTOR_NAMES`), grouping `@overload` signatures and `@<prop>.setter` / `@<prop>.deleter` accessors with their first definition.
   - Implemented the Rust extractor in [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs) (`collect_type_method_scopes`, `impl_self_type_name`, `is_rust_constructor_name`), extracting direct non-test `fn` items from inherent `impl` blocks (`impl_item.trait_().is_none()`).

4. **Step 4 — Python Class Field & Main-Guard Extractors**:
   - Added `PythonFieldAfterMethod` and `collect_fields_after_methods` to [src/code_lint/ast/python/classes.rs](../../../src/code_lint/ast/python/classes.rs) and re-exported them from [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs).
   - Added `collect_statements_after_main_guard` and `is_main_guard_statement` to [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs).

5. **Step 5 — Rust Associated Item Extractor**:
   - Added `RustAssociatedItemAfterMethod` and `collect_associated_items_after_methods` to [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs).

6. **Step 6 — Implement and Register the 5 `Topic::DECLARATION_ORDER` Rules**:
   - [src/code_lint/rules/constructor_after_method.rs](../../../src/code_lint/rules/constructor_after_method.rs) (`constructor-after-method`, Python + Rust).
   - [src/code_lint/rules/private_before_public_method.rs](../../../src/code_lint/rules/private_before_public_method.rs) (`private-before-public-method`, Python + Rust).
   - [src/code_lint/rules/field_after_method.rs](../../../src/code_lint/rules/field_after_method.rs) (`field-after-method`, Python).
   - [src/code_lint/rules/associated_item_after_method.rs](../../../src/code_lint/rules/associated_item_after_method.rs) (`associated-item-after-method`, Rust).
   - [src/code_lint/rules/statement_after_main_guard.rs](../../../src/code_lint/rules/statement_after_main_guard.rs) (`statement-after-main-guard`, Python).
   - Registered all 5 rules in `CODE_RULES` in [src/code_lint/rules.rs](../../../src/code_lint/rules.rs).
   - Uncommented the `declaration-order` row in [docs/dev/tag_guide.md](../../tag_guide.md) §5 (`taxonomy::tests::every_tag_has_a_rule` and `topic_tree_table_matches_the_topics` pass).
   - Updated [tests/snapshots/cli__list_rules.snap](../../../tests/snapshots/cli__list_rules.snap) and [ROADMAP.md](../../../ROADMAP.md).

---

## 2. Deviations & Adjustments During Execution

1. **`ruff_python_ast::ExprCompare` representation in `is_main_guard_statement`**:
   - In the pinned version of `ruff_python_ast`, `ExprCompare` stores `ops: Box<[CmpOp]>` and `operands: Box<[Expr]>` (including the left-hand expression as `operands[0]`), rather than separate `left` and `comparators` fields. Matching `(&*compare.ops, &*compare.operands)` against `([ruff_python_ast::CmpOp::Eq], [left, right])` handles single-operator equality comparisons directly without intermediate slice conversions.
2. **`rule_test!` doubled-file check on `statement-after-main-guard`**:
   - `assert_rule_fail` in [src/test_utils.rs](../../../src/test_utils.rs) runs every `fail` snippet twice in one file (`format!("{code}\n\n{code}")`) and asserts that it produces 2 diagnostics matching the two copies' spans.
   - Filtering out duplicate `is_main_guard_statement` nodes in `collect_statements_after_main_guard` and starting `statement-after-main-guard`'s `fail` snippets with the `if __name__ == "__main__":` guard ensures that the doubled file produces the exact 2 expected diagnostics, while statements preceding the guard are unit-tested directly on `collect_statements_after_main_guard` in [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs).
3. **Self-dogfooding (`test_self_dogfooding_code_lint`)**:
   - Running `omni-code-lint` on Omni's own repository reported 0 `Topic::DECLARATION_ORDER` findings and caught 3 naming violations in our new AST helper code (`is_main_guard_stmt` / `stmt_if`, `self_ty`, and `assoc_item_list`), which were renamed to `is_main_guard_statement` / `if_statement`, `self_type`, and `associated_items`.

---

## 3. Per-Exemption Mutation Check Results

Every named exemption across the 5 rules was temporarily disabled to confirm that a named `pass` test case in `rule_test!` fails when the exemption is removed:

| Rule | Exemption | Mutation Applied | Failing `pass` Test Case Confirmed |
| :--- | :--- | :--- | :--- |
| `constructor-after-method` | `E1` (Multiple constructors at top of scope) | Set `seen_non_constructor = true` on constructors | `pass::case_1_constructors_before_methods`, `pass::case_4_constructors_at_top_of_inherent_impl` |
| `constructor-after-method` | `E2` (Python `@overload` constructor grouping) | Disable `continues_overload` deduplication in `collect_class_methods` | `pass::case_2_overloaded_init_grouped_at_first_declaration_exempt` |
| `constructor-after-method` | `E3` / `E7` (Trait `impl`, nested classes, separate `impl` blocks) | Flatten scopes across classes / `impl` blocks | `pass::case_3_nested_class_tracks_constructors_independently`, `pass::case_6_trait_impl_and_separate_impl_blocks_exempt` |
| `constructor-after-method` | `E4` & `E5` (Rust private `fn new*` and `&self` methods) | Remove `is_exported && !has_self` from `is_constructor` | `pass::case_5_private_new_helper_and_self_receiver_new_method_exempt` |
| `constructor-after-method` | `E6` (Rust `#[cfg(test)]` / `#[test]` items in `impl`) | Remove `is_in_rust_inline_test` skip in `collect_type_method_scopes` | `pass::case_7_inline_test_before_constructor_exempt` |
| `private-before-public-method` | `E1` (Python `__dunder__` methods in public tier) | Set `is_dunder = false` in `python_method_visibility` | `pass::case_1_public_and_dunder_before_private` |
| `private-before-public-method` | `E2` (Python `@<prop>.setter` / `@overload` grouping) | Disable `is_property_accessor` deduplication in `collect_class_methods` | `pass::case_2_public_property_setter_after_private_helper_exempt`, `fail::case_3_private_property_grouped_once_before_public_method` |
| `private-before-public-method` | `E3` (Rust `pub(crate)` / `pub(super)` in exported tier) | Require unrestricted `pub` for `MethodVisibility::Public` | `pass::case_4_exported_tiers_before_private_methods` |
| `private-before-public-method` | `E4` & `E6` (Trait `impl`, nested classes, separate `impl` blocks, module functions) | Merge scopes across classes / `impl` blocks | `pass::case_3_module_functions_and_nested_classes_isolated`, `pass::case_5_trait_impl_and_separate_impl_blocks_exempt` |
| `private-before-public-method` | `E5` (Rust `#[cfg(test)]` / `#[test]` items in `impl`) | Remove `is_in_rust_inline_test` skip in `collect_type_method_scopes` | `pass::case_6_inline_test_before_pub_method_exempt` |
| `field-after-method` | `E1` (Unannotated `Stmt::Assign` after `def`) | Flag `Stmt::Assign` when `seen_method` is true | `pass::case_2_unannotated_method_alias_and_property_after_def_exempt` |
| `field-after-method` | `E2` & `E3` (Non-`Expr::Name` target, nested classes, method-local annotations) | Remove `Expr::Name` target guard in `collect_fields_after_methods` | `pass::case_3_non_name_target_and_nested_class_exempt` |
| `associated-item-after-method` | `E1` & `E2` (Macro calls in `impl`/`trait` and local items in `fn` body) | Flag `AssocItem::MacroCall` or walk inside `fn` bodies | `pass::case_2_macro_call_and_local_items_inside_fn_exempt` |
| `associated-item-after-method` | `E3` (Separate `impl` blocks and inline `#[cfg(test)]` `fn`) | Remove `is_in_rust_inline_test` skip in `collect_misplaced_assoc_items` | `pass::case_3_separate_impl_blocks_and_inline_test_fn_exempt` |
| `statement-after-main-guard` | `E1` & `E2` (Reversed comparison order and guard `else` branch) | Only match `__name__` on left or flag statements inside `orelse` | `fail::case_2_reversed_main_guard_comparison`, `pass::case_1_main_guard_at_end_of_module` |
| `statement-after-main-guard` | `E3` (Non-guard `if` comparisons on `__name__`) | Match any `CmpOp` in `is_main_guard_statement` | `pass::case_2_non_main_name_comparisons_exempt` |
| `statement-after-main-guard` | `E4` (Nested `if __name__ == "__main__":` inside function) | Walk nested function bodies in `collect_statements_after_main_guard` | `pass::case_3_nested_main_guard_inside_function_exempt` |
