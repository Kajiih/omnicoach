# Phase 4 — Execution Log: Agent Lifecycle Hook Implementation

## 1. Overview & Verification Summary

This document records the Phase 4 execution of the `omnicoach hook` improvements designed in [`03_design.md`](./03_design.md) and governed by `/phased-engineering-workflow`.

All changes were implemented using Test-Driven Development (Audit $\to$ RED $\to$ GREEN $\to$ Verify), following all architectural DAG boundaries ([ADR 006](../decisions/006_architectural_dag_and_conformance.md)), colocation ordering rules ([ADR 011](../decisions/011_colocated_abstraction_ordering.md)), and component placement ([ADR 012](../decisions/012_module_placement_by_abstraction.md)).

### Quality Gates Status: **ALL GREEN**

- `cargo fmt --check`: Clean (0 diffs).
- `cargo clippy --all-targets -- -D warnings`: Clean (0 warnings, 0 errors).
- `cargo test`:
  - `src/lib.rs`: 2,393 passed; 0 failed.
  - `tests/architecture_conformance.rs`: 11 passed; 0 failed.
  - `tests/cli.rs`: 34 passed; 0 failed.
  - `tests/registry.rs`: 19 passed; 0 failed.
  - `Doc-tests omnicoach`: 8 passed; 0 failed.
  - **Total**: 2,465 tests passing, 0 failures.
- `cargo run -- code` (Self-dogfooding lint): **All checks passed!** (0 diagnostics).

---

## 2. Completed TDD Tasks

### Task T1: Expose `RuleCatalog` Footer Helper & Update Architectural DAG

- **Files Modified**:
  - `src/architecture.rs`: Added `RuleCatalog` to `Hook` allowed dependencies (`Hook => [CodeLintRunner, CommandLintRunner, CommandLintVcs, RuleSelection, RuleCatalog, Diagnostic]`).
  - `src/rule_catalog.rs`: Added `pub fn code_report_footer_for_rules<'a>(binary: &str, mut rule_names: impl Iterator<Item = &'a str>) -> String` and refactored `code_report_footer` to delegate to it.
- **Tests Added/Verified**:
  - `rule_catalog::tests::code_report_footer_includes_note_only_for_heuristic_or_opinionated_rules`
  - `tests/architecture_conformance.rs` (11 tests passed).

### Task T2: Bounded `WarningQueue` Capacity

- **Files Modified**:
  - `src/hook/warning_queue.rs`:
    - Defined `const MAX_QUEUED_WARNINGS: usize = 64;`.
    - Enforced retention of the 64 most recent unique warnings under file locking.
    - Added explicit `.truncate(false)` on open to avoid implicit truncation clippy warnings while reading prior contents.
- **Tests Added/Verified**:
  - `hook::warning_queue::tests::push_caps_queue_at_max_queued_warnings`: Verified pushing 70 unique warnings leaves exactly the 64 newest entries.

### Task T3: Workspace Boundary, Lazy Net-New Scoping, and Enriched Warning Formatting

- **Files Modified**:
  - `src/hook/runner.rs`:
    - Implemented `is_within_workspace` checking target paths against `event.workspace_paths` and rejecting paths outside workspace or inside `agent_owned_dir`.
    - Implemented lazy baseline diagnostics extraction and `filter_relevant_diagnostics`:
      - Violations on modified lines (`virtual_file.modified_lines`) are always reported without decrementing baseline counts.
      - Violations on unmodified lines are matched against pre-edit baseline by multiset count of `(diagnostic.rule_name, diagnostic.message.summary)` (line-shift invariant).
      - Pre-existing debt on untouched lines is 100% suppressed; net-new non-local spillovers are correctly detected and warned.
    - Updated `format_code_warning` to include `Rationale: ...`, `Suggestion: ...`, and optional `Or: {hint}`.
- **Tests Added/Verified**:
  - `hook::runner::tests::evaluate_pre_tool_use_skips_file_outside_workspace_paths`
  - `hook::runner::tests::evaluate_pre_tool_use_reports_net_new_spillover_on_unmodified_line_and_suppresses_legacy_debt`
  - `hook::runner::tests::evaluate_pre_tool_use_does_not_unmask_unmodified_violation_when_same_violation_added_above`
  - `hook::runner::tests::format_code_warning_includes_rationale_suggestion_and_optional_hint`

### Task T4: Conditional Guidance Footer in `PreInvocation` & Integration Tests

- **Files Modified**:
  - `src/hook/antigravity.rs`:
    - Updated `serialize_pre_invocation` to extract rule names via `extract_rule_name` and append `code_report_footer_for_rules("omnicoach", ...)`.
    - Conditionally includes `SUPPRESSION_NOTE` (`omni:ignore [<rule>] -- <reason>`) only when at least one reported warning corresponds to a `heuristic` or `opinionated` rule in `RuleCatalog`.
    - Always includes `FIX_NOTE` and `--explain` command pointer.
  - `tests/cli.rs`:
    - Updated `test_hook_queues_code_lint_warning_and_drains_on_pre_invocation` to verify presence of `Rationale:`, `Suggestion:`, `omni:ignore`, and `omnicoach --explain`.
- **Tests Added/Verified**:
  - `hook::antigravity::tests::serialize_pre_invocation_injects_warnings_as_one_step`
  - `hook::antigravity::tests::serialize_pre_invocation_includes_suppression_note_for_heuristic_rule`
  - `tests/cli.rs::test_hook_queues_code_lint_warning_and_drains_on_pre_invocation`

---

## 3. Deviations & Surgical Adaptations During Execution

1. **`clippy::format_push_string`**:
   - Replaced `warning.push_str(&format!(...))` in `format_code_warning` with `let _ = write!(warning, "\n  Or: {hint}");` importing `std::fmt::Write as _`.
2. **`clippy::suspicious_open_options`**:
   - Added `.truncate(false)` to `OpenOptions::new().create(true).truncate(false).read(true).write(true)` in `WarningQueue::push` to explicitly indicate that existing contents are read before truncation.
3. **`omnicoach code [packed-assertion]`**:
   - Replaced compound `&&` in `tests/cli.rs` with slice subtraction `missing_snippets` to ensure atomic failure reporting and stay under the 4-assertion limit of `[too-many-assertions]`.
4. **`single-letter-name` in test fixture**:
   - Replaced variable `x` (exempted by default in `single-letter-name` allowlist `["i", "j", "x", "f"]`) with `k` in `evaluate_pre_tool_use_does_not_unmask_unmodified_violation_when_same_violation_added_above`.
