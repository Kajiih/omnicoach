# Phase 5: Cleanup — `call-before-definition` (`DefineBeforeUseRule`)

This document records **Phase 5 (Cleanup & Simplification)** for `call-before-definition`.

> Status: **COMPLETE and validated.**

---

## 1. Simplifications & Deduplication Applied

| Area | Change | Rationale |
| :--- | :--- | :--- |
| **Scoped `partition_scope_epochs` for `@overload`, `@property`, and `@singledispatch`** | Grouped `@overload` stub-and-implementation pairs and `@property` getter/setter/deleter pairs into a single `LogicalFunction` at the first declaration position (`E4`), while splitting unrelated same-name redefinitions (`@singledispatch` `def _` handlers and `RepeatCheck::SameCode` concatenated test blocks) into separate epochs. | Preserves both `@overload`/`@property` first-declaration grouping and independent epoch evaluation when a name is genuinely redefined in the same scope. |
| **Reused Existing `ast::python` Pattern & Import Extractors** | Reused `extract_from_pattern`, `extract_from_import`, `extract_decorators_raw`, `extract_parameters_raw`, and `decorated_definition` in [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs), while adding `extract_local_target_names` so attribute/subscript assignment targets (`self.helper = 1`, `first, holder.helper = pair`) do not shadow sibling functions. | Avoids duplicating destructuring pattern or import binding extraction while preventing false-negative local shadowing on attribute writes. |
| **Temporary Artifacts Removed** | Deleted temporary `docs/dev/define_before_use/ast_additions.rs` staging file. | Leaves only the canonical Phase 1–7 documentation in `docs/dev/define_before_use/`. |

---

## 2. Test Suite & Static Analysis Audit

- All 23 `rule_test!` cases in [src/code_lint/rules/call_before_definition.rs](../../../src/code_lint/rules/call_before_definition.rs) and the AST unit test in [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) pass.
- Per-exemption mutation verification confirmed all 4 exemption families (`E1`–`E4`) are `[KILLED]` when disabled.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and `cargo doc --no-deps --document-private-items` pass with zero warnings.
