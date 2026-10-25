# Phase 5 & 6 — Clean up, Multi-Reviewer Audit & Triage Report

## 1. Overview

Following `/phased-engineering-workflow`, this document records the Phase 5 (Clean up) and Phase 6 (Independent Multi-Reviewer Audit) activities for the `omnicoach hook` hardening project.

Two independent read-only review agents evaluated the uncommitted changes in parallel across strict architectural boundaries:

1. **Area 1 Auditor (`e5adb921`)**: Hook Evaluation Core, Differential Scoping, `RuleCatalog` & Architectural DAG.
2. **Area 2 Auditor (`cebe5cb8`)**: Antigravity Wire Adapter, `WarningQueue` Persistence, CLI Integration Tests & Engineering Documentation.

---

## 2. Phase 5: Clean Up Summary

1. **Dead Test Boilerplate Pruned (`src/hook/runner.rs`)**:
   - In `evaluate_pre_tool_use_skips_file_outside_workspace_paths`, `evaluate_pre_tool_use_reports_net_new_spillover_on_unmodified_line_and_suppresses_legacy_debt`, and `evaluate_pre_tool_use_does_not_unmask_unmodified_violation_when_same_violation_added_above`, removed 18 lines of unused `FakeVcsClient` closures and invoked `evaluate_pre_tool_use(&event)` directly, testing the module's public entrypoint.
2. **Double Allocation & Code Duplication Removed (`src/hook/warning_queue.rs`)**:
   - Unified warning parsing, deduplication, and `MAX_QUEUED_WARNINGS` capacity capping across `push` and `drain` into `collect_unique_warnings(existing_jsonl: &str, new_warnings: &[String]) -> Vec<String>`. Eliminated a secondary `HashSet<String>` allocation and deduplicated capacity-capping logic.
3. **ADR 011 Colocation Ordering (`src/hook/runner.rs`)**:
   - Reordered helpers in `src/hook/runner.rs` so that private helpers exclusive to `lint_proposed_edit` (`is_within_workspace`, `filter_relevant_diagnostics`, `format_code_warning`) immediately follow their single caller, followed by shared helpers (`resolve_in_workspace`, `resolve_project_root`, `normalize_path`, `load_hook_config`).

---

## 3. Phase 6: Independent Multi-Reviewer Audit Triage Table

| ID | Area / Boundary | Finding Description | Severity | Verdict | Rationale & Remediation |
| :--- | :--- | :--- | :--- | :---: | :--- |
| **A1-1** | Area 1 (Core) | `RuleSelection => [Diff, ...]` in `src/architecture.rs` serves a single item (`discover_project_root`). | Minor / RICR | **Reject** | `discover_project_root` was placed in `rule_selection.rs` in earlier commit `72f5aa24721a` on `main`, and is slated to be reused for CLI ancestor and global fallback config resolution per `ROADMAP.md:218-221`. Per Phase 5 rules, structures with a near-term roadmap use are preserved. |
| **A1-2** | Area 1 (Core) | Redundant `raw_path.as_os_str().is_empty()` check in `resolve_in_workspace`. | Minor / RICR | **Defer** | Pre-existing code on `main`; per User Rule 3 (*Surgical Changes*), adjacent working code is not refactored without necessity. |
| **A1-3** | Area 1 (Core) | Avoid `String` clones in `baseline_counts` and document modified-vs-unmodified invariant in `filter_relevant_diagnostics`. | Minor / RICR | **Fix** | **Applied (doc comment)**: Added doc comment explaining why modified-line diagnostics bypass `baseline_counts` without decrementing it; retained `HashMap<(RuleName, String), usize>` because `&mut HashMap<(RuleName, &str), usize>` is invariant over the `&str` lifetime across the `.filter(|diagnostic| ...)` closure (`E0521`). |
| **A1-4** | Area 1 (Core) | `resolve_project_root` walks above `workspace_paths` when the workspace has no VCS/config marker. | Minor / RICR | **Defer** | Handled deliberately in the dedicated Global Fallback Config step (`ROADMAP.md`), where ancestor walking vs workspace boundaries will be unified across CLI and Hook. |
| **A1-5** | Area 1 (Core) | Declaration order of `lint_proposed_edit` exclusive vs shared helpers in `src/hook/runner.rs`. | Minor / RICR | **Fix** | **Applied**: Grouped `lint_proposed_edit` exclusive helpers immediately after their caller, before shared helpers ([ADR 011](../decisions/011_colocated_abstraction_ordering.md)). |
| **A1-6** | Area 1 (Core) | Unused `FakeVcsClient` closures in `FileEdit` unit tests. | Minor / RICR | **Fix** | **Applied**: Switched all 3 new file-edit tests to call `evaluate_pre_tool_use(&event)` directly. |
| **A1-7** | Area 1 (Core) | `code_report_footer_for_rules` signature should accept `impl IntoIterator<Item = &'a str>`. | Nit | **Fix** | **Applied**: Updated signature to `rule_names: impl IntoIterator<Item = &'a str>`, removing `mut` and improving idiomatic flexibility. |
| **A1-8** | Area 1 (Core) | ADR 006 component count update. | Nit / Out-of-scope | **Reject** | `Hook` was already present in `src/architecture.rs` on `main`; no new component was introduced in this change. |
| **A2-M1** | Area 2 (Protocol & Docs) | Stale pre-decision text in `02_references.md:76` contradicts validated decision `D5`. | Minor / RICR | **Fix** | **Applied**: Updated `02_references.md:76` to document that non-opt-in workspaces (monorepos, dotfiles) are supported via net-new differential scoping (`D2`) and ancestor config discovery (`D5`). |
| **A2-M2** | Area 2 (Protocol & Docs) | Broken relative links in `04_execution_log.md`, machine-local path, drifted test name, and present-tense framing in `01_understand.md`. | Minor / RICR | **Fix** | **Applied**: Fixed relative ADR links to `../decisions/`, replaced machine-local path with plain skill name, corrected cited test name, and framed §2 in `01_understand.md` as baseline architecture prior to hardening. |
| **A2-M3** | Area 2 (Protocol & Docs) | Duplicate deduplication, capacity-capping, and double `HashSet` allocation in `src/hook/warning_queue.rs`. | Minor / RICR | **Fix** | **Applied**: Consolidated into `collect_unique_warnings`. |
| **A2-M4** | Area 2 (Protocol & Docs) | Implicit string format coupling between `format_code_warning` and `extract_rule_name`. | Minor / RICR | **Fix** | **Applied**: Documented the cross-module `<path>:<line>:<col>: [<rule>] <summary>` layout contract on both `format_code_warning` and `extract_rule_name`. |
| **A2-M5** | Area 2 (Protocol & Docs) | Weak assertions in `serialize_pre_invocation_injects_warnings_as_one_step` and single-batch test in `push_caps_queue_at_max_queued_warnings`. | Minor / RICR | **Fix** | **Applied**: Asserted both files appear in the ephemeral message and split queue push into two sequential batches (`0..40`, `40..70`). |
| **A2-N1** | Area 2 (Protocol & Docs) | Stale doc comment on `ToolCallOutcome::warnings`. | Nit | **Fix** | **Applied**: Updated doc comment to reflect net-new differential scoping beyond modified lines. |
| **A2-N2** | Area 2 (Protocol & Docs) | Optional wording tweak in `03_design.md` CUJ 1. | Nit | **Fix** | **Applied**: Changed "in an opt-in workspace" to "in an active workspace". |

---

## 4. Verification Post-Remediation

Following all remediations, the full verification suite was rerun:

- `cargo fmt --check`: Clean (0 diffs).
- `cargo clippy --all-targets -- -D warnings`: Clean (0 warnings, 0 errors).
- `cargo test`: All 2,465 tests pass (0 failures).
- `cargo run -- code`: All checks passed (0 diagnostics).
