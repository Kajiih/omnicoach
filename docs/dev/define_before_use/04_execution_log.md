# Phase 4: Execution Log — `call-before-definition` (`DefineBeforeUseRule`)

This document records the execution of **Phase 4 (TDD Implementation)** for `call-before-definition` as designed in [03_design_plan.md](03_design_plan.md).

> Status: **COMPLETE and validated.**

---

## 1. Task-by-Task Execution Summary

| Task | Scope | Key Implementation Details | Verification |
| :--- | :--- | :--- | :--- |
| **T1 (Taxonomy & AST Foundation)** | [src/rule_declaration/taxonomy.rs](../../../src/rule_declaration/taxonomy.rs), [docs/dev/tag_guide.md](../tag_guide.md), [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) | - Added `Topic::DECLARATION_ORDER` (`declaration-order`) to `taxonomy.rs` and `tag_guide.md`.<br>- Implemented `ForwardCall`, `partition_scope_epochs`, `collect_local_scope_bindings`, `collect_bindings_in_subtree`, `method_receiver_name`, `match_sibling_call`, `can_reach`, `evaluate_epoch`, and `collect_forward_calls` in `src/code_lint/ast/python.rs`.<br>- Added unit test `test_collect_forward_calls_multiple_callees_and_cycles`. | `cargo test --lib test_collect_forward_calls` |
| **T2 (Rule Implementation)** | [src/code_lint/rules/call_before_definition.rs](../../../src/code_lint/rules/call_before_definition.rs) | - Implemented `RULE` (`call-before-definition`, `RuleTarget::SourceOnly`, `Topic::DECLARATION_ORDER`, `Precision::Heuristic`, `Consensus::Opinionated`, `ImpactedQuality::Maintainability`).<br>- Added 23 `rule_test!` cases (`13` pass, `10` fail) covering module and class forward calls, async functions, `@classmethod` (`cls.m()`), comprehensions, direct/mutual/3-way recursion cycles, class constructors (`__init__`, `__new__`, `__post_init__`), `@overload` and `@property` groups, local variable/parameter/import/comprehension/match shadowing, `global` declarations, and non-call references. | `cargo test --lib code_lint::rules::call_before_definition` |
| **T3 (Catalog Registration)** | [src/code_lint/rules.rs](../../../src/code_lint/rules.rs), [tests/snapshots/cli__list_rules.snap](../../../tests/snapshots/cli__list_rules.snap) | - Registered `call_before_definition::RULE` in `CODE_RULES` and updated `--list-rules` snapshot. | `cargo test --test registry --test cli` |
| **T4 (Mutation Verification)** | Per-exemption mutation harness | - Verified all 4 exemption families (`E1` recursion SCC, `E2` class constructors, `E3` local shadowing, `E4` multi-part `@overload`/`@property` grouping at first declaration order) are `[KILLED]` by unit tests. | `4/4` mutations killed |

---

## 2. Deviations & Refinements During Execution

1. **Method Receiver Exclusion in `collect_local_scope_bindings`**:
   - During initial unit test execution, `collect_local_scope_bindings(&part.function_node)` included the method's own receiver parameter (`self` or `cls`) in `initial_bindings`, which caused `match_sibling_call` (`active_bindings.contains(receiver)`) to treat `self`/`cls` as shadowed by a local variable.
   - Fixed by passing `excluded_parameter: Option<&str>` (`receiver.as_deref()`) to `collect_local_scope_bindings` for the outer method while passing `None` for nested functions/lambdas and recording any body reassignments (`self = other`) in `collect_bindings_in_subtree`.
2. **Retained `partition_scope_epochs` for `@overload`, `@property`, and `@singledispatch` Handling**:
   - Kept `partition_scope_epochs` so `@overload` stub-and-implementation groups and `@property` getter/setter/deleter groups merge into a single `LogicalFunction` at the first declaration position (`E4`), while unrelated duplicate definitions (`@singledispatch` `def _` handlers and `RepeatCheck::SameCode` concatenated test scopes) cleanly start a new epoch.
