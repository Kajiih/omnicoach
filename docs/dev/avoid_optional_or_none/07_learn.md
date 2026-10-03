# Phase 7: Learn — `nullable-collection-return` (`AvoidOptionalOrNoneRule`)

This document captures the reusable lessons from designing, implementing, and auditing `nullable-collection-return` (`AvoidOptionalOrNoneRule`).

> Status: **COMPLETE.**

---

## 1. Key Takeaways

1. **Narrowing Overbroad Type-Annotation Bans to High-Consensus Contracts**:
   - The legacy the legacy linter `AvoidOptionalOrNoneRule` flagged every `| None` annotation on parameters, attributes, and scalar return types (`int | None`, `User | None`), which forced Sentinel/Null-Object boilerplate across ordinary Python APIs and overlapped with Ruff `UP045` (`Optional[T]` → `T | None`). Narrowing the rule to **nullable collection return annotations** (`Sequence[T] | None`, `list[T] | None`, `Mapping[K, V] | None`, `Optional[Sequence[T]]`) aligned the rule with Effective Java Item 54 ("Return empty collections or arrays, not nulls") and the existing `signature_collection_types` family.
2. **Use `EnforcementMode::RequireExplanation` When Sentinel Semantics Can Be Intentional**:
   - Even for collection return types, `None` occasionally distinguishes "not queried / cache miss / uninitialized" from "queried and empty (`()`)". Declaring `EnforcementMode::RequireExplanation` as the rule's default `enforcement_mode` (configurable to `Ban`) allows callers with genuine tri-state semantics to document why `None` is returned in an adjacent comment without needing `omni:ignore`.
3. **Require Every Non-`None` Union Branch to Be a Collection**:
   - `collect_union_branches` flattens `A | B | None`, `Optional[A]`, and `Union[A, B, None]` into a branch list and a `has_none` flag. `collect_nullable_collection_return_types` returns an empty list unless `has_none` is set and every branch is a collection type (`collection_branch_type_path`), so mixed unions like `list[int] | int | None` are skipped. For `Sequence[str] | Mapping[str, int] | None`, it returns `Sequence` and `Mapping` once each, in source order, and the rule joins them into the `{token}` placeholder.
