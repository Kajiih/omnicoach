# Phase 6: Review & Audit — Declaration Ordering (`Topic::DECLARATION_ORDER`)

> [!IMPORTANT]
> **Status**: **COMPLETED**

---

## 1. Review Protocol

Two independent read-only review agents audited the entire `Topic::DECLARATION_ORDER` batch (`constructor-after-method`, `private-before-public-method`, `field-after-method`, `associated-item-after-method`, `statement-after-main-guard`) in parallel:
1. **Code & Tests Reviewer**: audited the shared AST extractors ([src/code_lint/ast.rs](../../../src/code_lint/ast.rs), [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs), [src/code_lint/ast/python/classes.rs](../../../src/code_lint/ast/python/classes.rs), [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs)), the 5 rule modules, cross-rule interactions, and test cases.
2. **User-Facing Text & Documentation Reviewer**: audited `ViolationTemplate`, `Classification`, `RuleDoc`, `Example`s, [docs/dev/tag_guide.md](../../tag_guide.md), [docs/dev/naming_and_message_style_guide.md](../../naming_and_message_style_guide.md), [ROADMAP.md](../../../ROADMAP.md), and [01_understand.md](01_understand.md)–[05_cleanup.md](05_cleanup.md).

---

## 2. Code & Tests Review Findings and Resolutions

| # | Severity | Finding | Resolution |
| :--- | :--- | :--- | :--- |
| **C1** | Medium (Bug) | **Doc comments before `#[cfg(test)]` / `#[test]` bypassed `is_in_rust_inline_test`**: In `ra_ap_syntax`, outer doc comments (`/// ...`) are leading token children of an item's `SyntaxNode` before any `ast::Attr` child. `collect_inline_test_ranges_rec` in [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs) started ranges at `first_attribute_start` instead of `node.text_range().start()`, so `file.is_in_rust_inline_test(item.syntax().text_range().start().into())` returned `false` whenever a `#[cfg(test)]` item carried a `///` doc comment. | **Fixed**: `collect_inline_test_ranges_rec` now records `node.text_range().start()..node.text_range().end()`. Added `/// Test-only helper method.` above `#[cfg(test)]` in the `pass` test cases of `constructor_after_method.rs`, `private_before_public_method.rs`, and `associated_item_after_method.rs`. |
| **C2** | Medium (False Positive) | **Rust `new_*` / `try_new_*` associated functions returning an unrelated type were flagged as constructors**: `collect_type_method_scopes` in [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs) did not check whether the function's return type referenced `Self` or `type_name`, so a factory for another type (`pub fn new_request_id() -> u64` inside `impl Server`) was classified as a constructor of `Server`. | **Fixed**: Added `returns_self_type(function, &type_name)` checking that `function.ret_type()` contains a `SELF_TYPE_KW` (`Self`) or `IDENT` (`type_name`) token, and added `pub fn new_request_id() -> u64` to `private_new_helper_and_self_receiver_new_method_exempt` in [src/code_lint/rules/constructor_after_method.rs](../../../src/code_lint/rules/constructor_after_method.rs). |
| **C3** | Low (False Positive) | **Overloaded Python property setters and `@<prop>.getter` were not grouped with the property**: In `collect_class_methods` ([src/code_lint/ast/python/classes.rs](../../../src/code_lint/ast/python/classes.rs)), when a non-overloaded `@property` getter was followed by a private helper and then `@overload` stubs for its setter, `seen.has_overload` was `false`, so the setter's `@overload` stub was emitted as a new public method. | **Fixed**: Updated `continues_overload` to `is_overload || (seen.has_overload && !seen.has_non_overload)` and included `"getter"` alongside `"setter" | "deleter"` in `is_property_accessor`. Updated `public_property_setter_after_private_helper_exempt` in [src/code_lint/rules/private_before_public_method.rs](../../../src/code_lint/rules/private_before_public_method.rs) to cover overloaded property setters. |
| **C4** | Low (Duplication) | **Duplicated `TypeAlias` and `Const` arms in `collect_misplaced_assoc_items`**: Both arms in [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs) repeated the same 10-line `RustAssociatedItemAfterMethod` construction block. | **Fixed**: Extracted `let misplaced_name = match assoc_item { ... };` and pushed once. |

---

## 3. User-Facing Text & Documentation Review Findings and Resolutions

| # | Severity | Finding | Resolution |
| :--- | :--- | :--- | :--- |
| **D1** | High | **Inaccurate method names in `04_execution_log.md` Step 1**: Step 1 listed `LiteralValue::boolean`, `SuppressionTracker::audit_diagnostics`, and `InterceptedCommand::has_arg` instead of the actual preceding methods. | **Fixed**: Updated [04_execution_log.md](04_execution_log.md) to cite `LiteralValue::negated`, `SuppressionTracker::audit`, and `InterceptedCommand::program_base_name`. |
| **D2** | High | **Pending `06_review_and_audit.md` and `07_learn.md` linked from `ROADMAP.md`**: `ROADMAP.md` linked to `07_learn.md` before Phases 6 and 7 were written. | **Fixed**: Created [06_review_and_audit.md](06_review_and_audit.md) and [07_learn.md](07_learn.md). |
| **D3** | High | **Broken relative link to deleted `call_before_definition.rs` in `01_understand.md`**: Line 51 linked to the deleted rule file. | **Fixed**: Replaced with inline backticks `` `src/code_lint/rules/call_before_definition.rs` `` and updated minor stale details (`line 195`, `{function}`, `RuleTarget::SourceOnly`) in [01_understand.md](01_understand.md). |
| **D4** | Medium | **Stale reference to deleted `collect_scope_functions` (`scopes.rs:L736-L779`) in `02_references.md`**: Line 67 pointed to the deleted helper in `scopes.rs`. | **Fixed**: Updated [02_references.md](02_references.md) to point to `collect_class_methods` in [src/code_lint/ast/python/classes.rs](../../../src/code_lint/ast/python/classes.rs). |
| **D5** | Medium | **`associated-item-after-method` template phrasing and `{class}` placeholder documentation**: `suggestion` said `"Move {name} to the top of {class}"`, which on `impl Worker` read like moving the item into `struct Worker` rather than the `impl` block, and `summary` said `"after a method"` instead of ``"after a `fn` item"``. | **Fixed**: Updated `summary` to `"Associated item `{name}` of `{class}` is declared after a `fn` item."`, `suggestion` to `"Move `{name}` to the top of the `{class}` `trait` or `impl` block, before any `fn` items."`, and updated `{class}` in [docs/dev/naming_and_message_style_guide.md](../../naming_and_message_style_guide.md). |
| **D6** | Medium | **Inaccurate `TypedDict` mention in `field-after-method`**: PEP 589 `TypedDict` classes cannot define methods at all (`TypeError` at runtime). | **Fixed**: Replaced `TypedDict` in `rationale` and `why_is_this_bad` of [src/code_lint/rules/field_after_method.rs](../../../src/code_lint/rules/field_after_method.rs) with `@dataclass`, `attrs`, `NamedTuple`, and Pydantic `BaseModel`. |
| **D7** | Low | **Undocumented exemption for duplicate `if __name__ == "__main__":` guards in `statement-after-main-guard`**: `collect_statements_after_main_guard` skips subsequent main guards, which was not mentioned in `what_it_does` or `03_design_plan.md`. | **Fixed**: Documented in [src/code_lint/rules/statement_after_main_guard.rs](../../../src/code_lint/rules/statement_after_main_guard.rs) and [03_design_plan.md](03_design_plan.md). |
