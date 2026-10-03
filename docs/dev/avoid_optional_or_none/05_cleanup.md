# Phase 5: Cleanup — `nullable-collection-return` (`AvoidOptionalOrNoneRule`)

This document records **Phase 5 (Cleanup & Simplification)** for `nullable-collection-return`.

> Status: **COMPLETE and validated.**

---

## 1. Simplifications & Deduplication Applied

| Area | Change | Rationale |
| :--- | :--- | :--- |
| **Reused `PythonFunctionSignature::is_exempt_from_signature_rules`** | Delegated all signature-level exemptions (`Protocol`/`ABC` classes, `@override`/`@overload`/`@abstractmethod`/`@fixture`/`@<fn>.register`, and dunder methods other than `__init__`/`__new__`/`__call__`) to `signature.is_exempt_from_signature_rules()`. | Shares the exact signature contract exemption semantics used by `concrete-collection-return` and `mutable-collection-return` with zero duplication. |
| **Reused `is_concrete_collection_constructor` & `is_std_type_constructor`** | Composed `is_non_tuple_collection_constructor` directly on top of `is_concrete_collection_constructor` and `is_std_type_constructor` in [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs). | Avoids re-listing concrete collection constructors (`list`, `dict`, `set`, `deque`, `defaultdict`, `Counter`, `OrderedDict`) or standard library module qualifiers. |
| **Temporary Artifacts Removed** | Deleted temporary `docs/dev/avoid_optional_or_none/ast_additions.rs` staging file. | Leaves only the canonical Phase 1–7 documentation in `docs/dev/avoid_optional_or_none/`. |

---

## 2. Test Suite & Static Analysis Audit

- All 39 `rule_test!` cases in [src/code_lint/rules/nullable_collection_return.rs](../../../src/code_lint/rules/nullable_collection_return.rs) and 12 `#[rstest]` unit cases in [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) pass.
- Per-exemption mutation verification confirmed all 3 exemption families (`E1`–`E3`) are `[KILLED]` when disabled.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and `cargo doc --no-deps --document-private-items` pass with zero warnings.
