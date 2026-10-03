# Phase 5: Cleanup — Signature & Attribute Collection Type Rules

This document records **Phase 5 (Cleanup & Simplification)** for the 7 Python collection type annotation rules in [04_execution_log.md](04_execution_log.md).

> Status: **COMPLETE — Ready for User Validation before Phase 6 (Review & Audit)**

---

## 1. Simplifications & Deduplication Applied

| Area | Change | Rationale |
| :--- | :--- | :--- |
| **Function Exemption Methods on `PythonFunctionSignature`** | Added `PythonFunctionSignature::is_exempt_from_signature_rules(&self)` and `PythonFunctionSignature::is_exempt_from_body_usage_rules(&self)` in [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) and removed 5 duplicate `fn is_exempt_function` helpers across `concrete_collection_parameter.rs`, `concrete_collection_return.rs`, `mutable_collection_parameter.rs`, `mutable_collection_return.rs`, and `specific_collection_parameter.rs`. | Eliminates 5 identical helper definitions across rule modules and centralizes signature vs. body-usage exemption semantics on `PythonFunctionSignature`. |
| **Narrowed `ast::python` Public Surface** | Changed `is_concrete_collection_constructor`, `collect_type_constructors`, `is_exempt_dunder_method`, `has_exempt_signature_decorator`, `is_in_protocol_or_abc_class`, and `is_stub_function_body` from `pub fn` to module-private `fn` in [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs). | Keeps internal AST predicates private to `ast::python` while exposing only the high-level domain queries (`collect_concrete_collection_types`, `collect_mutable_collection_types`, `collect_specific_collection_types`, `has_unaliased_collections_abc_set_import`, `is_parameter_mutated_or_escaping`, `analyze_parameter_collection_capability`, `collect_locally_mutated_return_functions`, `collect_public_class_attributes`). |
| **Removed Unused State in `CapabilityTracker`** | Removed `reference_count: usize` from `CapabilityTracker` in [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs). | `reference_count` was incremented in `record_reference_capability` (`+= 1` suppressed `dead_code` warnings) but never read when computing `ParameterCollectionCapability`. |

---

## 2. Test Suite Audit

- Verified all 7 rule test suites use `crate::test_utils::rule_test!` with one behavior per named case, zero redundant multi-diagnostic test scaffolds, and 100% pass/fail coverage of Matrices A–F.
- Verified `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and `cargo doc --no-deps --document-private-items` pass with zero warnings.
