# Phase 4: Execution Log — `nullable-collection-return` (`AvoidOptionalOrNoneRule`)

This document records the execution of **Phase 4 (TDD Implementation)** for `nullable-collection-return` as designed in [03_design_plan.md](03_design_plan.md).

> Status: **COMPLETE and validated.**

---

## 1. Task-by-Task Execution Summary

| Task | Scope | Key Implementation Details | Verification |
| :--- | :--- | :--- | :--- |
| **T1 (AST Foundation)** | [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) | - Implemented `ABSTRACT_AND_IMMUTABLE_COLLECTION_CONSTRUCTORS`, `is_non_tuple_collection_constructor`, `is_ellipsis_type_arg`, `unwrap_return_envelope` (`Annotated`, `Awaitable`, `Coroutine`), `collect_union_branches` (`|`, `Optional`, `Union`, `Annotated`), `collection_branch_type_path` (concrete/abstract collections + bare `tuple`/`Tuple` + variadic `tuple[T, ...]`/`Tuple[T, ...]`, excluding fixed-length record tuples), and `collect_nullable_collection_return_types`.<br>- Added parameterized `#[rstest]` unit tests (`test_collect_nullable_collection_return_types`, 12 cases). | `cargo test --lib test_collect_nullable_collection_return_types` |
| **T2 (Rule Implementation)** | [src/code_lint/rules/nullable_collection_return.rs](../../../src/code_lint/rules/nullable_collection_return.rs) | - Implemented `RULE` (`nullable-collection-return`, `RuleTarget::SourceOnly`, `EnforcementMode::RequireExplanation`, `Topic::STATIC_TYPING`, `Precision::Heuristic`, `Consensus::Opinionated`, `ImpactedQuality::Maintainability`).<br>- Reused `extract_function_signatures(file)` and `signature.is_exempt_from_signature_rules()`.<br>- Added 39 `rule_test!` cases (`20` pass, `19` fail) covering concrete and abstract collections, bare/variadic vs fixed-length tuples, mixed unions, async envelopes, signature exemptions (`Protocol`, `ABC`, `@override`, `@overload`, `@abstractmethod`, `@fixture`, `@<fn>.register`, dunders), and explanation comments (`EnforcementMode::RequireExplanation` vs `Ban`). | `cargo test --lib code_lint::rules::nullable_collection_return` |
| **T3 (Catalog Registration)** | [src/code_lint/rules.rs](../../../src/code_lint/rules.rs), [tests/snapshots/cli__list_rules.snap](../../../tests/snapshots/cli__list_rules.snap) | - Registered `nullable_collection_return::RULE` in `CODE_RULES` and updated `--list-rules` snapshot. | `cargo test --test registry --test cli` |
| **T4 (Mutation Verification)** | Per-exemption mutation harness | - Verified all 3 exemption families (`E1` fixed-length record tuple, `E2` mixed union with non-collection branch, `E3` signature exemptions) are `[KILLED]` by unit tests. | `3/3` mutations killed |
