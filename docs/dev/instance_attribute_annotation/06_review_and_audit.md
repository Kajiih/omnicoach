# Phase 6: Review & Audit — `inline-public-attribute-annotation` (`InstanceAttributeAnnotationRule`)

This document records the independent pedantic code/test and user-facing text reviews conducted for `inline-public-attribute-annotation` (`InstanceAttributeAnnotationRule`), along with the verification of all findings.

> Status: **COMPLETE and validated.**

---

## 1. Independent Review Setup

Two independent `research` subagents performed read-only pedantic reviews of the working copy:
1. **Code & Test Reviewer** (`a037f54b-74fe-4432-a11b-4bd5222593e5`): audited [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs), [src/code_lint/rules/inline_public_attribute_annotation.rs](../../../src/code_lint/rules/inline_public_attribute_annotation.rs), test coverage, edge cases, and mutation resilience.
2. **User-Facing Text & Documentation Reviewer** (`759a5ea9-0f7d-42b8-b24e-8d40fb820093`): audited `ViolationTemplate`, `RuleDoc`, `--explain` output, taxonomy classification, and `docs/dev/instance_attribute_annotation/` against [docs/dev/naming_and_message_style_guide.md](../naming_and_message_style_guide.md) and [docs/dev/tag_guide.md](../tag_guide.md).

---

## 2. Findings & Resolutions

| # | Source | Severity | Finding | Resolution |
| :--- | :--- | :--- | :--- | :--- |
| **1** | Code & Test Review | Minor | `PythonInlinePublicAttributeAnnotation` in [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) derived no traits, so it was not `Clone`, unlike peer AST structs in `python.rs`. | Added `#[derive(Clone)]` on `PythonInlinePublicAttributeAnnotation`. |
| **2** | User-Facing Text Review | Minor | `TEMPLATE.suggestion` read `"Move `{name}: {expression}` to the body of `{class}` and assign `self.{name}` without a type annotation in `{function}`."`, which is slightly awkward when the flagged statement is a bare annotation (`self.timeout: int` without `= value`). | Updated `TEMPLATE.suggestion` in [src/code_lint/rules/inline_public_attribute_annotation.rs](../../../src/code_lint/rules/inline_public_attribute_annotation.rs) to `"Move `{name}: {expression}` to the body of `{class}` and keep only the unannotated `self.{name}` assignment in `{function}`."` |

---

## 3. Verification Matrix

- **Rule unit tests**: `cargo test --lib code_lint::rules::inline_public_attribute_annotation` — 19 cases (`10` pass, `9` fail) + `RepeatCheck::SameCode` — **PASS**.
- **AST unit tests**: `cargo test --lib test_collect_inline_public_attribute_annotations` — **PASS**.
- **Per-exemption mutation check**: `5/5` mutations (`E1` private `_` prefix, `E2` bare `Final` exemption, `E3` `@staticmethod`/`@classmethod`, `E4` non-`self` receiver, `E5` nested function/class/lambda scope boundary) — **[KILLED]**.
- **Registry & architecture guardrails**: `cargo test --test registry` and `cargo test --lib architecture` — **PASS**.
