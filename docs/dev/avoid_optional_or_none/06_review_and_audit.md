# Phase 6: Review & Audit — `nullable-collection-return` (`AvoidOptionalOrNoneRule`)

This document records the independent pedantic code/test and user-facing text reviews conducted for `nullable-collection-return` (`AvoidOptionalOrNoneRule`), along with the verification of all findings.

> Status: **COMPLETE and validated.**

---

## 1. Independent Review Setup

Two independent `research` subagents performed read-only pedantic reviews of the working copy:
1. **Code & Test Reviewer** (`a037f54b-74fe-4432-a11b-4bd5222593e5`): audited [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs), [src/code_lint/rules/nullable_collection_return.rs](../../../src/code_lint/rules/nullable_collection_return.rs), test coverage, edge cases, and mutation resilience.
2. **User-Facing Text & Documentation Reviewer** (`759a5ea9-0f7d-42b8-b24e-8d40fb820093`): audited `ViolationTemplate`, `RuleDoc`, `--explain` output, taxonomy classification, and `docs/dev/avoid_optional_or_none/` against [docs/dev/naming_and_message_style_guide.md](../naming_and_message_style_guide.md) and [docs/dev/tag_guide.md](../tag_guide.md).

---

## 2. Findings & Resolutions

| # | Source | Severity | Finding | Resolution |
| :--- | :--- | :--- | :--- | :--- |
| **1** | Code & Test Review | None | All AST helpers (`collect_nullable_collection_return_types`, `unwrap_return_envelope`, `collect_union_branches`, `collection_branch_type_path`) and all 32 `rule_test!` cases in [src/code_lint/rules/nullable_collection_return.rs](../../../src/code_lint/rules/nullable_collection_return.rs) were verified with no issues found. | No code changes required. |
| **2** | User-Facing Text Review | Minor | [ROADMAP.md](../../../ROADMAP.md) listed the 7 existing signature & attribute collection type rules on line 76 without `nullable-collection-return`, and omitted `AvoidOptionalOrNoneRule` from the `*Not pursued*` summary list. | Updated [ROADMAP.md](../../../ROADMAP.md) to include `nullable-collection-return` (`docs/dev/avoid_optional_or_none/`) in the signature & attribute collection type rules list and added `AvoidOptionalOrNoneRule` Check 1 (`Optional[T]` syntax covered by Ruff `UP045`) and scalar/attribute/parameter exclusions to `*Not pursued*`. |

---

## 3. Verification Matrix

- **Rule unit tests**: `cargo test --lib code_lint::rules::nullable_collection_return` — 32 cases (`17` pass, `15` fail) + `RepeatCheck::SameCode` — **PASS**.
- **AST unit tests**: `cargo test --lib test_collect_nullable_collection_return_types` — **PASS**.
- **Per-exemption mutation check**: `3/3` mutations (`E1` fixed-length record `tuple` exemption, `E2` mixed non-collection union exemption, `E3` `is_exempt_from_signature_rules` signature exemptions) — **[KILLED]**.
- **Registry & architecture guardrails**: `cargo test --test registry` and `cargo test --lib architecture` — **PASS**.
