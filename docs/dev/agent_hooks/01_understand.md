# Phase 1 — Understand: Agent Lifecycle Hook Policies & Differential Coaching

## 1. Problem Statement

`omnicoach hook antigravity` (`src/hook/{antigravity,runner,virtual_edit,warning_queue,event}.rs`) intercepts agent lifecycle events before execution (`PreToolUse`) and before model reasoning turns (`PreInvocation`). The goal is to provide guardrails against destructive shell operations (such as modifying described commits in Jujutsu) and to provide in-flight code quality coaching on file modifications without stalling or paralyzing autonomous agents.

While the initial implementation proved that command and file-edit interception could be unified under an agent-neutral evaluation core, five critical policy and scoping questions needed rigorous empirical investigation before hardening the hook for daily production use:

1. **Hard-blocking (`"decision": "deny"`) vs. asynchronous coaching (`PreInvocation` `ephemeralMessage`) for file edits**:
   When an agent's proposed file edit introduces code lint violations, should `PreToolUse` reject the edit with `deny`, or allow the edit to land and deliver the warnings in the next model turn via `ephemeralMessage`?
2. **In-flight coaching vs. end-of-task explicit verification (`omnicoach code --diff`)**:
   Does in-flight feedback improve developer speed, token consumption, and code quality, or is an explicit verification pass at the end of a task sufficient?
3. **Diagnostic scoping for file modifications (`modified_lines` vs. whole file vs. net-new)**:
   How should violations be filtered so that agents are never nagged about pre-existing legacy debt, yet are immediately notified when their edits introduce non-local spillovers on unmodified lines (e.g., duplicated strings triggering `repeated-literal` elsewhere in the file)?
4. **Trigger boundaries and directory exclusions**:
   Where should file-edit linting trigger? Specifically, how should internal agent scratchpads (`agent_owned_dir`), temporary debug scripts (`/tmp/...`), non-opt-in repositories (workspaces without `.omnicoach.toml`), and opt-in projects be isolated?
5. **Actionable context and suppression guidance**:
   What information must accompany ephemeral warnings so that models can resolve violations or legitimately suppress heuristic rules without entering web-search loops or breaking required API signatures?

---

## 2. Baseline Architecture & Identified Gaps (Prior to Hardening)

The baseline hook implementation was structured across five modules in `src/hook/`:

- `event.rs`: Neutral representation of tool calls (`ToolCallPayload`) and decisions (`PreToolDecision`).
- `antigravity.rs`: Wire-protocol adapter for Antigravity JSON payloads (`PreToolUse`, `PreInvocation`).
- `virtual_edit.rs`: Applies proposed edits (`Overwrite`, `Append`, `Replace`) in memory against disk content to compute the post-edit content and `modified_lines`.
- `runner.rs`: Dispatches commands to `command_lint` and file edits to `code_lint`.
- `warning_queue.rs`: Session-scoped, file-locked JSON-lines queue in the conversation's scratch directory for persisting warnings between `PreToolUse` and `PreInvocation`.

Our empirical studies (detailed in [02_references.md](02_references.md)) revealed four major functional and architectural gaps in the baseline:

### Gap F1: Line-Only Filtering Missed Non-Local Net-New Violations (`src/hook/runner.rs`)

`lint_proposed_edit` initially filtered diagnostics strictly by checking whether the diagnostic's line fell within `virtual_file.modified_lines`:

```rust
lint_file(relative_path, &virtual_file.content, &config)
    .iter()
    .filter(|diagnostic| {
        virtual_file
            .modified_lines
            .contains(&diagnostic.location.line)
    })
    ...
```

While this suppressed pre-existing legacy debt on unmodified lines, it **silently dropped violations introduced by the edit that were anchored on unmodified lines**. For example:

- Adding a second occurrence of a string literal on line 80 caused `[repeated-literal]` to report on line 42 (the first occurrence). Because line 42 was not modified in the edit, the warning was suppressed.
- Reordering or adding caller functions could trigger `[callee-before-caller]` or `[private-before-public]` on unmodified functions.
In our historical replay of 1,158 sessions, line-only filtering dropped **16,871 net-new non-local violations**. In our live multi-agent benchmark, Arm 5 (modified-lines only) missed a non-local `repeated-literal` violation on line 42 that Arm 6 and Arm 7 caught.

### Gap F2: Ephemeral Warnings Omitted Rationale, Hints, and the Catalog Guidance Footer (`src/hook/runner.rs`, `src/hook/antigravity.rs`)

`format_code_warning` initially formatted only:

```text
<path>:<line>:<col>: [<rule>] <summary>
suggestion: <suggestion>
```

It omitted `diagnostic.message.rationale` and `diagnostic.message.explanation_hint`. Furthermore, `serialize_pre_invocation` wrapped warnings in a generic header without the `RuleCatalog` guidance footer (`FIX_NOTE`, `SUPPRESSION_NOTE`, and `explain_footer`).

When an agent hit an opinionated or heuristic rule on a prompt-mandated signature (e.g., `[primitive-duration]` on a required `delay_ms: int` parameter):

- Without suppression syntax guidance (`omni:ignore [<rule>] -- <reason>`), the agent did not know how to suppress the rule.
- In our live benchmark (Arm 5), this caused the agent to execute **7 `search_web` queries** trying to guess `# noqa` syntax.
- In Arm 3 (hard-block), the agent broke the required API signature to escape the linter.

### Gap F3: Unbounded Directory Scope Outside Workspaces and in Non-Opt-In Repositories (`src/hook/runner.rs`)

`lint_proposed_edit` checked `event.agent_owned_dir`, but did not verify whether the target file resided within `event.workspace_paths`. Consequently:

- Edits to `/tmp` (throwaway reproduction and scratch scripts) were linted, generating an **87.5% warning trigger rate** on temporary code.
- Edits in repositories without an `.omnicoach.toml` fell back to `Config::default()`, generating a **36.1% warning trigger rate** across external codebases (e.g., shared monorepos, dotfiles) whose conventions differ from `omnicoach` defaults.

### Gap F4: Unbounded `WarningQueue` Growth (`src/hook/warning_queue.rs`)

`WarningQueue::push` appended JSON lines indefinitely. If an agent environment did not configure or invoke `PreInvocation`, the queue file grew monotonically on disk across long sessions.

---

## 3. Goals & Non-Goals

### Goals

- **G1 (Zero False Blocking)**: Ensure `PreToolUse` never hard-blocks file edits. All code lint feedback must be delivered asynchronously via `PreInvocation` ephemeral messages, allowing multi-step refactoring workflows to complete without interruption.
- **G2 (Net-New Differential Scoping)**: Upgrade diagnostic filtering to report any diagnostic on a modified line *plus* any diagnostic on an unmodified line that did not exist prior to the edit (`modified_lines` $\cup$ Net-New).
- **G3 (Actionable Guidance & Suppression)**: Provide self-contained warnings containing `rationale`, `suggestion`, `explanation_hint`, and the standard `RuleCatalog` guidance footer (`FIX_NOTE`, `SUPPRESSION_NOTE` with `omni:ignore [<rule>] -- <reason>`, and `explain_footer`).
- **G4 (Strict Workspace Boundaries)**: Restrict file-edit linting to files inside `workspace_paths` (skipping `/tmp` and out-of-workspace scratchpads in addition to `agent_owned_dir`). Support running across all workspaces (including shared team monorepos) using the nearest ancestor `.omnicoach.toml` (e.g., in `experimental/users/<user>/...`) or falling back to `Config::default()`, while tracking Global Fallback Config (`~/.config/omnicoach/config.toml`) in `ROADMAP.md`.
- **G5 (Bounded Queue Storage)**: Enforce a fixed capacity (`MAX_QUEUED_WARNINGS = 64`) on `WarningQueue`.

### Non-Goals

- **NG1 (Interactive Prompts During `PreToolUse`)**: The hook cannot prompt the user interactively during tool execution; decisions must be fully deterministic and machine-readable.
- **NG2 (Whole-Repository AST Indexing in Hook)**: The hook must not perform whole-repo indexing or expensive cross-file symbol resolution. All checks must complete within $\le 50\text{ ms}$ ($p_{50}$).
- **NG3 (Replacing CLI Verification)**: Ephemeral in-flight coaching does not eliminate the need for end-of-task verification (`omnicoach code --diff` or CI checks); it accelerates the inner loop so end-of-task verification passes on the first try.

---

## 4. Key Decisions Backed by Empirical Data

The following decisions (`D1`–`D7`) are grounded directly in the offline corpus replay (1,158 sessions, 199,857 tool calls) and the live 7-arm multi-agent prototype benchmark documented in [02_references.md](02_references.md):

| Decision | Policy Choice | Empirical Justification |
| :--- | :--- | :--- |
| **D1: Deny Policy** | Hard-block (`Deny`) for `run_command` only; `NoObjection` with queued warnings for file edits. | In the offline replay, 74.7% of multi-step edit sequences had transient intermediate violations; hard-blocking burned 85 turns and 464k tokens in Arm 3 and broke public API signatures. In contrast, shell commands (`jj edit`) have irreversible working-copy side effects and zero false positives. |
| **D2: Diagnostic Scoping** | Lazy Net-New Differential Scoping: report if on `modified_lines` OR net-new in `after` compared to `before`. | Line-only filtering missed 16,871 non-local violations in historical replay and missed line 42 in Arm 5. Net-new differential scoping caught all non-local spillovers in Arms 6 and 7 while suppressing 43,420 pre-existing legacy violations. |
| **D3: Warning Formatting** | Include `rationale`, `explanation_hint`, and the `RuleCatalog` guidance footer in `PreInvocation`. | In Arm 5, missing suppression syntax caused 7 wasted `search_web` calls. In Arm 7, including the footer enabled clean suppression of heuristic rules on required signatures, achieving the fastest completion (195s) and lowest tokens (183k). |
| **D4: Workspace Boundary** | Skip file linting if `!event.workspace_paths.is_empty()` and target path is outside all workspace roots. | 87.5% of edits to `/tmp` (scratchpads/reproduction scripts) triggered lint warnings. Confining checks to `workspace_paths` eliminates scratchpad noise completely. |
| **D5: Config Scope** | Support running in all `workspace_paths` (including shared team monorepos) using nearest ancestor `.omnicoach.toml` (e.g. `experimental/users/<user>/...`) or falling back to `Config::default()`. Global Fallback Config (`~/.config/omnicoach/config.toml`) tracked in `ROADMAP.md`. | Allows coaching in team monorepos where checking in `.omnicoach.toml` is prohibited, protected from legacy noise by net-new differential scoping (D2) and workspace boundaries (D4). |
| **D6: Toolset Compatibility** | Support both Antigravity tool variants without assuming "modern" vs. "legacy". | Variant A (`write_to_file`, `replace_file_content`, `multi_replace_file_content`) accounted for 94.0% of real tool calls (27,298 calls); Variant B (`write_file`, `edit_file`) accounted for 6.0% (1,756 calls). Both must be first-class. |
| **D7: Queue Bounding** | Cap `WarningQueue` at `MAX_QUEUED_WARNINGS = 64` entries. | Prevents runaway disk growth if `PreInvocation` is omitted from `hooks.json` or fails to drain. |

---

## 5. Validated User Decisions

The open questions were resolved and validated with the user before Phase 4 execution:

1. **Q1 / A1 — Config Scope in Large Monorepos (`D5`)**:
   - Resolved: Keep file-edit linting enabled across all active `workspace_paths` (including shared team monorepos) using nearest ancestor `.omnicoach.toml` or `Config::default()`.
   - Follow-up: Track **Global Fallback Config (`$XDG_CONFIG_HOME/omnicoach/config.toml` / `~/.config/omnicoach/config.toml`)** in `ROADMAP.md` for a dedicated step so user-level default rules and `per-file-ignores` can be managed in `chezmoi`.
2. **Q2 / A2 — Conditional Guidance Footer in `PreInvocation` (`D3`)**:
   - Resolved: Include `SUPPRESSION_NOTE` conditionally in `PreInvocation` only when at least one reported rule is `heuristic` or `opinionated` (matching `omnicoach code` via `RuleCatalog`).
3. **Q3 / A3 — Net-New Diagnostic Identity Matching (`D2`)**:
   - Resolved: Match pre-existing diagnostics on unmodified lines by multiset count of `(diagnostic.rule_name, diagnostic.message.summary)` (line-insensitive) so line shifts from insertions/deletions above an unmodified line never cause false positives.
