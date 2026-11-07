# Stage E Design: Ownership-Aware ADR 011 Diagnostics and Conflict-Free Anchors in `omnicoach code`

> **Phases 1–3 (Understand, Codebase Audit, Design & Atomic Commit Plan)**
> Continues [12_stage_d_results.md](12_stage_d_results.md) and [05_decisions.md](05_decisions.md).

---

## 1. TL;DR

- **Goal:** Eliminate post-edit ADR 011 declaration-ordering retry loops by making every `omnicoach code` ordering diagnostic state the item's ownership and point to a conflict-free insertion anchor on the first report.
- **Why This Matters (Empirical Evidence from Stage D):**
  - In the Stage D benchmark on **Config & Rule-Selection `[context]` Feature (`flash`)**, agents averaged **`10.7` (Baseline)** to **`12.3` (File / Directory Graph)** post-lint edit retries trying to satisfy ADR 011 (`private-before-public`, `callee-before-caller`, `uncolocated-helper`, `constructor-after-method`).
  - When the agent had an exact, conflict-free target line (`after RuleStatus (ends on line 420)`), it resolved placement in **`0` ordering retries**.
  - Every coding agent already runs `omnicoach code` after editing, so putting the ownership context (`exclusive helper of 'X'` vs. `shared across 'X', 'Y'`) and the conflict-free anchor into `omnicoach code` diagnostics delivers 1-step placement remediation without any extra command.
- **What This Design Changes:**
  1. **Ownership-aware `uncolocated-helper` wording (Defect 1, [05_decisions.md](05_decisions.md) `D60`):** `CallOrderFinding` carries `roots` (`Roots(item)`), and shared helpers get a message that directs them to the trailing helper section instead of a contradictory "move to the helper cluster of `{caller}`".
  2. **Conflict-free anchor scoring in `best_anchor` (`D57`, `L15`):** Candidate slots are scored by `(broken_held, unfixed_own, distance, new_pos)`, so the suggested anchor satisfies all of the moved item's own constraints whenever such a slot exists.
- **Dropped:** `omnicoach graph place` and `omnicoach graph impact` were removed instead of being fixed (§5, `D63`). The defects found in them are the evidence for keeping a single placement engine.

---

## 2. Phase 1: Understand — Problem Statement, Transcript Evidence & Scope

### 2.1 Empirical Motivation from Stage D Trajectory Audits

During Stage D and the follow-up evaluation ([12_stage_d_results.md](12_stage_d_results.md)), we audited the transcripts of the **Config & Rule-Selection `[context]` Feature** trials (`src/rule_selection.rs` and `src/config.rs`) across all conditions.

| Failure Pattern Observed in Agent Transcripts | Observed In | Root Cause in `omnicoach` | Cost Impact |
| :--- | :--- | :--- | :--- |
| **Contradictory `uncolocated-helper` hint on shared helpers:** Agent places `struct RuleSelectionConfig` (used by both `pub fn rule_status` and `pub fn parse_config`) right below `parse_config`. `omnicoach code` fires: *"Private helper `RuleSelectionConfig` is separated from the helper cluster of `parse_config`. Move `RuleSelectionConfig` to the helper cluster of `parse_config` or the end of the scope, for example below `RuleStatus` (ends on line 420)."* | **Config & Rule-Selection `[context]` Feature** (`Baseline`, `File / Directory Graph`, and `Voluntary Query CLI` trials) | `CallOrderFinding` does not distinguish `Ownership::Owner(caller)` from `Ownership::Shared` (`roots = [rule_status, parse_config]`). The static template in `uncolocated_helper.rs` claims the item belongs to `parse_config`'s cluster even when it is already right below `parse_config` and actually belongs to the trailing shared section. | `4`–`12` wasted edit-lint retry turns per run as the agent tries moving adjacent helpers before moving the item to the end of the scope. |
| **Two-move cascades:** Moving an item to fix `private-before-public` lands it above a private caller or type and triggers `callee-before-caller` on the same item, forcing a second move. | Stage D placement tasks ([12_stage_d_results.md](12_stage_d_results.md) §5.3, §6.1) | `best_anchor` scored slots only against the single triggering rule. | One extra edit-lint cycle per cascade. |

#### Context, Analysis & Interpretation

- **Context:** ADR 011 ([`011_colocated_abstraction_ordering.md`](../decisions/011_colocated_abstraction_ordering.md)) enforces four coupled intra-scope invariants:
  1. `private-before-public`: Public entrypoints precede private helpers except in inline helper clusters owned by an earlier entrypoint.
  2. `callee-before-caller`: Higher-level callers and contracts precede lower-level callees (`L(u) <= L(v)`), and type definitions precede callables whose signatures reference them (`SignatureContract`).
  3. `uncolocated-helper`: Private helpers owned by a single entrypoint (`Ownership::Owner(r)`) must sit either in `r`'s inline cluster (`r .. next_pub`) or in the trailing helper section after all public items; shared private helpers (`Ownership::Shared`, `|Roots(h)| >= 2`) must sit in the trailing helper section after all public items.
  4. `constructor-after-method`: Constructors precede non-constructor methods in `impl` and `class` scopes.
- **Analysis:** Because these four rules interact, moving an item requires knowing **two pieces of structural information**:
  1. **Its ownership classification:** Is it a `Public` entrypoint, an `Exclusive` helper of a single root `r`, or a `Shared` helper reached by multiple roots `r1, r2, ...`?
  2. **A conflict-free target slot:** Which declaration `anchor` (and end line) can the item be placed below so that **all four** ADR 011 rules hold simultaneously?
- **Interpretation:** `omnicoach code` exposed only the bare pair `(subject, related)` and a static message template that conflated exclusive and shared helpers, and its anchor ignored the moved item's other constraints. Carrying `roots` on `CallOrderFinding` and scoring anchors against all of the item's own constraints makes `omnicoach code` self-sufficient for post-edit placement remediation.

### 2.2 Scope & Non-Goals

- **In Scope:**
  1. Enrich `CallOrderFinding` in [`call_clusters.rs`](../../../src/code_lint/ast/call_clusters.rs) with `roots: Vec<String>` (`Roots(item)`) so `uncolocated-helper` renders accurate, non-contradictory remediation suggestions for shared vs. exclusive helpers.
  2. Score `best_anchor` candidate slots against all of the moved item's own ADR 011 constraints, tolerating pre-existing lint debt elsewhere in the scope.
- **Out of Scope:**
  - Changing the underlying ADR 011 ordering laws or adding new lint rule codes.
  - A separate placement query outside `omnicoach code` (§5).
  - Cross-file `CodeLintProject` extraction (tracked in [`ROADMAP.md`](../../../ROADMAP.md)).
  - Receiver-typed `x.method()` local variable type inference (tracked in [`ROADMAP.md`](../../../ROADMAP.md)).

---

## 3. Phase 2: Codebase Audit & Root-Cause Analysis

### 3.1 Defect 1: `uncolocated-helper` Conflates Exclusive (`Owner`) and Shared (`Shared`) Helpers

In [`call_clusters.rs`](../../../src/code_lint/ast/call_clusters.rs) and [`uncolocated_helper.rs`](../../../src/code_lint/rules/uncolocated_helper.rs):

- In `generate_placements`:

  ```rust
  match (graph.ownership(subject), last_root, last_public) {
      (Ownership::Unowned, _, Some(last_public)) => placements.push(rule(
          subject,
          Constraint::Below(last_public),
          CallClusterRule::PrivateBeforePublic,
          last_public,
      )),
      (Ownership::Owner(unit), Some(last_root), _) => {
          placements.push(rule(
              subject,
              Constraint::InRegion(Some(unit)),
              CallClusterRule::UncolocatedHelper,
              last_root,
          ));
          placements.push(rule(
              subject,
              Constraint::NotSplit(unit),
              CallClusterRule::UncolocatedHelper,
              unit,
          ));
      }
      (Ownership::Shared, Some(last_root), _) => placements.push(rule(
          subject,
          Constraint::InRegion(None),
          CallClusterRule::UncolocatedHelper,
          last_root,
      )),
      _ => {}
  }
  ```

- The region constraint differs by ownership:
  - For `Ownership::Owner(unit)`: `Constraint::InRegion(Some(unit))` holds either in `unit`'s inline cluster *or* in the trailing helper section (with `Constraint::NotSplit(unit)` preventing the unit's helpers from splitting across both).
  - For `Ownership::Shared`: `Constraint::InRegion(None)` holds **only** in the trailing helper section after all public items (`self.is_trailing(subject)`).
- Before Stage E, `uncolocated_helper.rs` formatted **both** cases with the same static strings:
  - `summary`: ``"Private helper `{function}` is separated from the helper cluster of `{caller}`."``
  - `suggestion`: ``"Move `{function}` to the helper cluster of `{caller}` or the end of the scope, for example below `{anchor}` (ends on line {line})."``
- When `subject` is `Ownership::Shared` (say `RuleSelectionConfig`, reached by `roots = [rule_status, parse_config]`) and sits immediately after `parse_config` (while another public item `RuleStatus` appears later at line 412), `last_root` is `parse_config`. The old diagnostic told the developer/agent:
  > *"Private helper `RuleSelectionConfig` is separated from the helper cluster of `parse_config`. Move `RuleSelectionConfig` to the helper cluster of `parse_config` or the end of the scope, for example below `RuleStatus` (ends on line 420)."*
- This was misleading: `RuleSelectionConfig` was *already* right below `parse_config`, and moving it into `parse_config`'s cluster cannot satisfy `Constraint::InRegion(None)` because it is a **shared helper** used by multiple public roots (`rule_status`, `parse_config`) and must be placed after all public declarations (below `RuleStatus` (ends on line 420)).

### 3.2 Anchor Scoring: Why `best_anchor` Must Score the Moved Item's Own Constraints

`best_anchor` in [`call_clusters.rs`](../../../src/code_lint/ast/call_clusters.rs) tries every single move of the misplaced declaration `from` to a new position `new_pos` and keeps the candidates where the violated placement holds.

- **Before:** Candidates were ranked by `(broken_held, distance, new_pos)`, where `broken_held` counts placements that held before the move and break after it. Nothing penalized a slot that left *another* constraint on `from` violated. Moving a `pub` item above a `priv` item to fix `private-before-public` could land it above a private caller or type and trigger `callee-before-caller` on the same item: a correct-looking first suggestion followed by a second diagnostic and a second move (`L15`).
- **How existing debt must be handled:** A file may already contain violations on unrelated items. Requiring every placement in the scope to hold would reject every slot in such a file. Counting only placements that held before the move (`broken_held`) and only the moved item's own unsatisfied placements (`unfixed_own`) keeps the score independent of untouched debt.

---

## 4. Phase 3: Architectural Design

### 4.1 Enriching `CallOrderFinding` with `roots: Vec<String>` (`Roots(item)`)

In [`call_clusters.rs`](../../../src/code_lint/ast/call_clusters.rs), `CallOrderFinding` carries `pub roots: Vec<String>` (the public entrypoint units that reach `item`):

```rust
pub struct CallOrderFinding<'a> {
    /// Identifier AST node of the misplaced declaration.
    pub name_node: AstNode<'a>,
    /// Name of the misplaced declaration.
    pub item: String,
    /// Name of the declaration or enclosing type the violated placement refers to.
    pub related: String,
    /// Public entrypoint unit names that reach `item` (`Roots(item)`).
    pub roots: Vec<String>,
    /// Why `related` sits above `item` in the abstraction order (`callee-before-caller` only).
    pub reason: Option<OrderingReason>,
    /// Sibling declaration next to which `item` belongs.
    pub anchor: PlacementAnchor,
}
```

#### How Each ADR 011 Rule Uses `CallOrderFinding` to Produce Unambiguous Diagnostics

| Rule | Before Stage E (`summary` / `suggestion`) | After Stage E (`summary` / `suggestion`) |
| :--- | :--- | :--- |
| **`uncolocated-helper`** (exclusive helper, `roots.len() <= 1`) | *Summary:* ``"Private helper `{function}` is separated from the helper cluster of `{caller}`."``<br>*Suggestion:* ``"Move `{function}` to the helper cluster of `{caller}` or the end of the scope, for example below `{anchor}` (ends on line {line})."`` | Unchanged for exclusive helpers (including `Constraint::NotSplit` split-cluster cases), where either `{caller}`'s inline cluster or the trailing section is valid and `below `{anchor}` (ends on line {line})` is the closest conflict-free slot. |
| **`uncolocated-helper`** (shared helper, `roots.len() > 1`) | *Summary:* ``"Private helper `{function}` is separated from the helper cluster of `{caller}`."`` **(Misleading)**<br>*Suggestion:* ``"Move `{function}` to the helper cluster of `{caller}` or the end of the scope, for example below `{anchor}` (ends on line {line})."`` **(Contradictory)** | *Summary:* ``"Private helper `{function}` is shared across `{r1}`, `{r2}` and declared before the trailing helper section at the end of the scope."``<br>*Suggestion:* ``"Move `{function}` to the trailing helper section at the end of the scope (after all public declarations), for example below `{anchor}` (ends on line {line})."`` |
| **`private-before-public`** | *Summary:* ``"Private {kind} `{item}` is declared before `{caller}`."``<br>*Suggestion:* ``"Move `{item}` below `{anchor}` (ends on line {line})."`` | Wording unchanged; the anchor comes from the conflict-free `best_anchor` (§4.2), which accounts for both `private-before-public` and `callee-before-caller`. |
| **`callee-before-caller`** | *Summary:* ``"Lower-level `{callee}` is declared before `{caller}` ({reason})."``<br>*Suggestion:* ``"Move `{callee}` below `{anchor}` (ends on line {line})."`` | Wording unchanged; the anchor comes from the conflict-free `best_anchor` (§4.2). |

The `{relation}` and `{section}` phrases are built by `relation` and `section` in [`uncolocated_helper.rs`](../../../src/code_lint/rules/uncolocated_helper.rs).

#### Context, Analysis & Interpretation of the Diagnostic Design

- **Simplicity & Surgical Changes:** Only `uncolocated-helper` suffered from a template contradiction, because it is the only rule where two distinct `Ownership` variants (`Owner(unit)` and `Shared`) map to the same `CallClusterRule`. For exclusive helpers, `private-before-public`, and `callee-before-caller`, the existing wording is already clear once `best_anchor` supplies a conflict-free target line.
- **No Churn on Exclusive-Helper Tests:** `uncolocated-helper` keeps its phrasing when `roots.len() <= 1` and switches to the shared-helper message when `roots.len() > 1`, so existing exclusive-helper expectations stay valid while shared helpers get an exact explanation (``shared across `rule_status`, `parse_config` and declared before the trailing helper section at the end of the scope`` $\to$ ``move to the trailing helper section at the end of the scope (after all public declarations), for example below `RuleStatus` (ends on line 420)``).

### 4.2 Conflict-Free Anchor Scoring in `best_anchor`

For every candidate position `new_pos` where the violated placement holds, `best_anchor` computes

$$\text{score}(\text{new\_pos}) = \bigl(\text{broken\_held},\; \text{unfixed\_own},\; |\text{new\_pos} - \text{from}|,\; \text{new\_pos}\bigr)$$

and returns the anchor of the lexicographic minimum:

- `broken_held` counts placements that held before the move and break after it. A candidate that breaks a held placement stronger than the one being fixed is rejected.
- `unfixed_own` counts placements on `from` itself that remain violated at `new_pos`. A candidate that leaves a `Hard` placement (Python import- or class-time evaluation order) on `from` violated is rejected.
- Distance and position break ties toward the smallest, earliest move.

Whenever a slot with `broken_held == 0` and `unfixed_own == 0` exists, the first diagnostic points to it, so one move resolves the item. `constructor-after-method` is not scored: its anchor is the related method itself.

---

## 5. Dropped: `graph place` and `graph impact`

The original Stage E plan also fixed three defects in `omnicoach graph place` and `omnicoach graph impact`. Instead, both subcommands were removed, together with the placement simulator in `call_clusters.rs` (`simulate_item_placement`, `HypotheticalItem`, `SimulatedPlacement`, `InsertionSlot`, `evaluate_candidate_slots`) and `impact`'s `render_divergent_edges` in `query.rs` ([05_decisions.md](05_decisions.md) `D63`, superseding `D61`, `D62`, and the `impact` half of `D57`).

The defects themselves are the evidence for having one engine:

| Defect found in the audit | What it shows |
| :--- | :--- |
| **Index-shift corruption in `simulate_item_placement`:** Simulating an existing declaration removed it from the scope without remapping the positional indices stored in the other items' `references` and `evaluated_before`, corrupting every edge to a later item and dropping the item's own references when `--calls` / `--called-by` were omitted. | Re-deriving the scope graph for a hypothetical item is a second model of the file that must be kept consistent with the one `omnicoach code` builds from source. |
| **Divergent slot scoring:** `evaluate_candidate_slots` required every incident placement to hold, while `best_anchor` tolerates pre-existing debt. In a file where an already-misplaced helper referenced the simulated item, `place` returned no valid slot while `omnicoach code` suggested one. | Two scoring functions for the same question drift apart; the fix would have been to make one call the other. |
| **Unscoped `impact --diff` ordering findings:** `render_divergent_edges` reported findings on untouched declarations whose `related` name matched a modified symbol (6 findings on `src/rule_selection.rs` where `omnicoach code --diff` reported none). | `impact --diff` re-implemented `omnicoach code --diff` ordering checks (plus `slice` for callers) and had its own scoping bug. |

Since every agent runs `omnicoach code` after editing, the ownership-aware diagnostics (§4.1) and the conflict-free `best_anchor` (§4.2) deliver the conflict-free anchor in one place. The remaining graph queries are `find` and `slice`.

---

## 6. Verification & Evaluation Plan

1. **Deterministic Unit & Rule Tests (`cargo test`):**
   - **Shared-helper finding:** The `uncolocated-helper` rule test case `shared_helper_stranded_before_trailing_public_method` in [`uncolocated_helper.rs`](../../../src/code_lint/rules/uncolocated_helper.rs) flags a helper reached by two public methods that sits before a later public method; exclusive-helper cases cover the unchanged wording.
   - **Conflict-free anchor:** Case `private_before_public_and_callee_before_caller_selects_conflict_free_anchor` of `test_call_cluster_placement_anchors` in [`call_clusters.rs`](../../../src/code_lint/ast/call_clusters.rs) checks that a `private-before-public` fix is anchored below the private caller, so `callee-before-caller` holds after the same move.
2. **Repository Self-Verification:**
   - Run full `cargo test`, `cargo clippy --all-targets`, and `omnicoach code` across the workspace (`0` warnings, `0` errors).

---

## 7. Atomic Commit Mapping

1. **`fix(call-clusters): name shared-helper roots and pick conflict-free anchors`**
   - Add `pub roots: Vec<String>` (`Roots(item)`) to `CallOrderFinding` in [`call_clusters.rs`](../../../src/code_lint/ast/call_clusters.rs) and emit shared-helper-specific summary and suggestion text in [`uncolocated_helper.rs`](../../../src/code_lint/rules/uncolocated_helper.rs) when `finding.roots.len() > 1`.
   - Score `best_anchor` candidates by `(broken_held, unfixed_own, distance, new_pos)`.
   - Add tests in `call_clusters.rs` and `uncolocated_helper.rs`.

## 8. Summary

| Component | Before Stage E | After Stage E |
| :--- | :--- | :--- |
| **`omnicoach code` `[uncolocated-helper]` on Shared Helpers** | Contradictory message claiming the shared helper belongs to `last_root`'s inline cluster. | Identifies ``Private helper `{fn}` is shared across `{a}`, `{b}` and declared before the trailing helper section at the end of the scope`` and directs it to the trailing helper section (below `{anchor}` (ends on line {line})). |
| **`omnicoach code` Anchors for `private-before-public` / `callee-before-caller`** | Anchor fixed the triggering rule only, which could cause a 2-move cascade. | Anchor satisfies all of the moved item's own constraints whenever such a slot exists, tolerating unrelated pre-existing debt. |
| **Placement and post-edit queries in `omnicoach graph`** | `place` and `impact` duplicated `omnicoach code`'s placement and ordering logic and diverged from it. | Removed; `omnicoach code` is the single ADR 011 placement engine, and `find` / `slice` remain for navigation. |
