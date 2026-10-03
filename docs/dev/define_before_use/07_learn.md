# Phase 7: Learn — `call-before-definition` (`DefineBeforeUseRule`)

This document captures the reusable lessons from designing, implementing, and auditing `call-before-definition` (`DefineBeforeUseRule`).

> Status: **COMPLETE.**

---

## 1. Key Takeaways

1. **The Epoch Flush Exists for `RepeatCheck::SameCode` and Hides Forward Calls**:
   - The epoch flush exists so `RepeatCheck::SameCode` passes (design D7 in [03_design_plan.md](03_design_plan.md)). `rule_test!` runs each `fail` case as `{code}\n{code}` in one module. Merging all same-named functions of a scope into one `LogicalFunction` broke that check: per-(caller, callee) deduplication collapsed the two copies into one finding. The flush has a cost: it hides forward calls across any unrelated same-name redefinition, `@singledispatch` `def _` handlers included. The fix is tracked in [python_ast_consolidation/01_understand.md](../python_ast_consolidation/01_understand.md) and in the [ROADMAP.md](../../../ROADMAP.md) item on the `rule_test!` repeat check.
2. **Never Reuse General Pattern Extractors on Assignment LHS Without Filtering Attribute/Subscript Leaves**:
   - In `tree-sitter-python`, unpacking assignments (`first, holder.helper = pair`) represent the LHS as a `pattern_list` containing both `identifier` and `attribute`/`subscript` nodes. A recursive helper that delegates non-leaf nodes to a general pattern extractor (`extract_from_pattern`) will walk inside `holder.helper` and extract `helper` as if it were a bound variable name. Dedicated LHS binding extraction (`extract_local_target_names`) must recurse on itself and return early on `attribute` and `subscript` nodes at every depth.
3. **Exclude the Method's Own Receiver Parameter from Local Shadowing Sets**:
   - When checking `self.helper()` or `cls.helper()` inside a method, the method's first parameter (`self` or `cls`) is bound in the method header. Excluding that receiver parameter from `initial_bindings` while still recording any body reassignments (`self = other`) ensures `self.helper()` is recognized as a sibling method call unless `self` is actually rebound in the body.
