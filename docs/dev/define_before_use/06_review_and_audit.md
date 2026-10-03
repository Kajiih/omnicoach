# Phase 6: Review & Audit — `call-before-definition` (`DefineBeforeUseRule`)

This document records the independent pedantic code/test and user-facing text reviews conducted for `call-before-definition` (`DefineBeforeUseRule`), along with the verification of all findings.

> Status: **COMPLETE and validated.**

---

## 1. Independent Review Setup

Two independent `research` subagents performed read-only pedantic reviews of the working copy:
1. **Code & Test Reviewer** (`a037f54b-74fe-4432-a11b-4bd5222593e5`): audited [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs), [src/code_lint/rules/call_before_definition.rs](../../../src/code_lint/rules/call_before_definition.rs), test coverage, edge cases, and mutation resilience.
2. **User-Facing Text & Documentation Reviewer** (`759a5ea9-0f7d-42b8-b24e-8d40fb820093`): audited `ViolationTemplate`, `RuleDoc`, `--explain` output, taxonomy classification, and `docs/dev/define_before_use/` against [docs/dev/naming_and_message_style_guide.md](../naming_and_message_style_guide.md) and [docs/dev/tag_guide.md](../tag_guide.md).

---

## 2. Findings & Resolutions

| # | Source | Severity | Finding | Resolution |
| :--- | :--- | :--- | :--- | :--- |
| **1** | Code & Test Review | Major | In `extract_local_target_names` ([src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs)), the fallback `_ =>` arm called `extract_from_pattern` instead of recursing with `extract_local_target_names`. For a mixed unpacking assignment such as `first, holder.helper = pair` (where the LHS is a `pattern_list` containing an `attribute`), `extract_from_pattern` recursed into `holder.helper` and extracted both `holder` and `helper` as local bindings, falsely shadowing a sibling function named `helper`. | Fixed `extract_local_target_names` to recurse with `extract_local_target_names(&child, out)` in its `_ =>` arm. Added `first, holder.helper = pair` to `attribute_assignment_does_not_shadow_module_function` in [src/code_lint/rules/call_before_definition.rs](../../../src/code_lint/rules/call_before_definition.rs). |
| **2** | Code & Test Review | Minor | `extract_from_pattern` handled untyped `list_splat_pattern` (`*args`) and `dictionary_splat_pattern` (`**kwargs`), but when a variadic parameter carried a type annotation (`def caller(*helper: Any):`), `tree-sitter-python` wrapped the `list_splat_pattern` inside `typed_parameter`, where `extract_from_pattern` only checked `identifier`. | Extended the `"typed_parameter" | "typed_default_parameter"` arm in `extract_from_pattern` to recurse on `"identifier" | "list_splat_pattern" | "dictionary_splat_pattern"`. Added `run_with_typed_varargs(*helper: Any)` to `shadowed_by_parameter_or_nested_lambda_parameter`. |
| **3** | Code & Test Review | Minor | `ForwardCall` in [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) derived no traits, so it was not `Clone`. | Added `#[derive(Clone)]` on `ForwardCall`. |
| **4** | User-Facing Text Review | Important | `what_it_does` in [src/code_lint/rules/call_before_definition.rs](../../../src/code_lint/rules/call_before_definition.rs) described `@overload` and `@property` exemptions in a way that could be read as exempting any forward call to an `@overload` or `@property` target, whereas `partition_scope_epochs` groups multi-part definitions at their first `def` position (`E4`) so calling an `@overload` group before its first stub is still flagged. | Updated `what_it_does` to state explicitly: *"Multi-part definitions that share a name (`@overload` stubs and their implementation, or a `@property` getter and its setter or deleter) are grouped at the position of their first `def`."* |

---

## 3. Verification Matrix

- **Rule unit tests**: `cargo test --lib code_lint::rules::call_before_definition` — 23 cases (`13` pass, `10` fail) + `RepeatCheck::SameCode` — **PASS**.
- **AST unit tests**: `cargo test --lib test_collect_forward_calls` — **PASS**.
- **Per-exemption mutation check**: `4/4` mutations (`E1` recursion SCC `can_reach`, `E2` `__init__`/`__new__`/`__post_init__` caller exemption, `E3` `collect_local_scope_bindings`, `E4` `@overload`/`@property` first-declaration grouping) — **[KILLED]**.
- **Registry & architecture guardrails**: `cargo test --test registry` and `cargo test --lib architecture` — **PASS**.
