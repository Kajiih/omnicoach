# Phase 5: Cleanup — `fake-without-protocol` (`FakeMustInheritProtocolRule`)

This document records **Phase 5 (Cleanup & Simplification)** for `fake-without-protocol`.

> Status: **COMPLETE and validated.**

---

## 1. Simplifications & Deduplication Applied

| Area | Change | Rationale |
| :--- | :--- | :--- |
| **Method Placement on `PythonClassInfo` & `PythonBaseClass`** | Implemented `unsubscripted_name` and `is_contract_base` directly on `PythonBaseClass` and `is_fake_class_name` and `has_contract_base` on `PythonClassInfo` in [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs). | Keeps [src/code_lint/rules/fake_without_protocol.rs](../../../src/code_lint/rules/fake_without_protocol.rs) `check_file` to a 5-line declarative iterator pipeline (`extract_classes(file).into_iter().filter(...).map(...)`). |
| **Temporary Artifacts Removed** | Deleted temporary `docs/dev/fake_must_inherit_protocol/ast_additions.rs` staging file after merging into `src/code_lint/ast/python.rs`. | Leaves only the canonical Phase 1–7 documentation in `docs/dev/fake_must_inherit_protocol/`. |

---

## 2. Test Suite & Static Analysis Audit

- All 23 `rule_test!` cases in [src/code_lint/rules/fake_without_protocol.rs](../../../src/code_lint/rules/fake_without_protocol.rs) and 17 `#[rstest]` unit cases in [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) test one distinct behavior per case.
- Per-exemption mutation verification confirmed both `E1` and `E2` exemptions are `[KILLED]` when disabled.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and `cargo doc --no-deps --document-private-items` pass with zero warnings.
