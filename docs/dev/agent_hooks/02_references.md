# Phase 2 — References & Empirical Evaluation: Agent Lifecycle Hook Policies

## 1. External & Architectural References

### 1.1 Antigravity Agent Lifecycle Hook Protocol

Antigravity executes external hook commands configured in `~/.gemini/config/hooks.json` during the agent event loop:

- **`PreToolUse`**: Invoked synchronously before a tool call executes.
  - *Input*: JSON on `stdin` containing `workspacePaths`, `artifactDirectoryPath`, and `toolCall` (`name` and `args`).
  - *Output*: Blank stdout (`""`) allows the tool call (`NoObjection`). A JSON object `{"decision":"deny","reason":"..."}` rejects the tool call and returns `reason` as an error response to the agent.
  - *Crucial Wire-Protocol Constraint*: `PreToolUse` **cannot return informational text or warnings to the model** while allowing the tool call. A tool call is either allowed or denied.
- **`PreInvocation`**: Invoked before the model starts a new reasoning turn.
  - *Input*: JSON on `stdin` containing `artifactDirectoryPath`.
  - *Output*: Blank stdout (`""`) executes no injection. A JSON object `{"injectSteps":[{"ephemeralMessage":"..."}]}` injects one or more ephemeral context messages into the model's prompt for that turn without polluting conversation history.

### 1.2 State of the Art: Differential Static Analysis & Diff Scoping

Industrial linting frameworks (Reviewdog, SonarQube "Clean as You Code", GitHub PR checks, Rust Clippy diff integrations) face the fundamental tension between:

- **Line-filtered linting**: Only reporting issues where `diagnostic.line \in modified_lines`.
- **Differential (Net-New) linting**: Reporting issues present in the modified file that did not exist in the baseline commit/file:
  $$\text{Reported} = \text{Diagnostics}(F_{\text{after}}) \setminus \text{Diagnostics}(F_{\text{before}})$$
Line-filtered linting fails when edits introduce non-local effects (e.g., duplicate literals, cyclic imports, or public/private declaration reordering). Whole-file linting fails in existing codebases by nagging the author about pre-existing legacy violations.

### 1.3 Internal Architecture & ADR Conformance

- [ADR 006: Architectural DAG and Conformance Checking](../decisions/006_architectural_dag_and_conformance.md): Defines the strict acyclic dependency structure of `custom_lints`.
- [ADR 011: Colocated Abstraction Ordering](../decisions/011_colocated_abstraction_ordering.md): Governs top-down ordering within modules.
- [ADR 012: Module Placement by Abstraction Level](../decisions/012_module_placement_by_abstraction.md): Places `Hook` as an application-edge orchestration component depending on `CodeLintRunner`, `CommandLintRunner`, `CommandLintVcs`, `RuleSelection`, `RuleCatalog`, and `Diagnostic`.

---

## 2. Study 1: Offline Corpus Replay Across 1,158 Real Agent Sessions

To determine the empirical baseline of agent tool calls, file edit patterns, and violation rates, we conducted an offline replay analysis across all historical sessions stored on this machine:

- **Sessions Analyzed**: **1,158** conversation transcripts (`~/.config/agent/brain/*/.system_generated/logs/transcript.jsonl`).
- **Total Tool Calls Replayed**: **199,857**.
- **File Edit Trajectories Reconstructed**: **1,668** unique editing sequences across Python (`.py`) and Rust (`.rs`).
- **Commands Evaluated**: **56,804** `run_command` invocations.

### 2.1 Toolset Distribution & Usage Patterns

```text
Total Tool Calls Replayed: 199,857
├── view_file:                 69,261  (34.7%)
├── run_command:               56,804  (28.4%)
├── replace_file_content:      20,088  (10.1%)
├── manage_task:               13,617  (6.8%)
├── code_search:                9,182  (4.6%)
├── grep_search:                6,739  (3.4%)
├── write_to_file:              5,806  (2.9%)
├── schedule:                   3,985  (2.0%)
├── read_file:                  3,808  (1.9%)
├── ask_question:               1,688  (0.8%)
├── multi_replace_file_content: 1,404  (0.7%)
├── edit_file:                  1,310  (0.7%)
└── write_file:                   446  (0.2%)
```

#### Key Empirical Finding: Toolset Split

- **Variant A (`write_to_file` + `replace_file_content` + `multi_replace_file_content`)**: **27,298 calls (94.0%)** of all file modifications.
- **Variant B (`write_file` + `edit_file`)**: **1,756 calls (6.0%)** of file modifications.
- **VCS Usage**: `jj` was the #1 command binary invoked across all sessions (**over 18,000 calls** including binary and path invocations), confirming that Jujutsu working-copy safety guardrails (`[edit-of-described-commit]`) have massive daily impact.

### 2.2 Directory Zone Analysis: Where Do Edits Occur?

Every file edit was categorized into one of four directory zones:

| Directory Zone | Edits Recorded | % of Edits | Lint Trigger Rate | Top Rules Triggered | Average Payload Size |
| :--- | :---: | :---: | :---: | :--- | :---: |
| **Zone 1: Agent Brain** (`~/.config/agent/brain/...`) | 6,157 | 21.4% | **75.6%** | `repeated-literal` (2,304), `single-letter-name` (505), `undocumented-nontrivial-function` (67) | 4,993 bytes |
| **Zone 2: `/tmp` & Scratch** (`/tmp/...`) | 55 | 0.2% | **87.5%** | `repeated-literal` (17), `single-letter-name` (12), `dynamic-attribute-access` (10) | 3,658 bytes |
| **Zone 3: Opt-In Repos** (`custom_lints` with `.omnicoach.toml`) | 3,817 | 13.3% | **31.9%** | `repeated-literal` (114), `single-letter-name` (48), `bare-multiline-string` (32), `abbreviated-name` (15) | 4,572 bytes |
| **Zone 4: Other Workspaces** (shared monorepos, dotfiles, repos without `.omnicoach.toml`) | 22,838 | 65.1% | **36.1%** | `repeated-literal` (4,550), `single-letter-name` (448), `abbreviated-name` (150), `private-before-public` (135) | 6,907 bytes |

#### Findings on Directory Scope

1. **Brain Exclusion is Essential**: Without excluding `agent_owned_dir`, **75.6% of internal agent operations** would trigger false warnings.
2. **Scratchpad Noise in `/tmp`**: 87.5% of one-off scripts written to `/tmp` violate strict formatting/docstring rules. Confining file linting to `workspace_paths` removes this noise entirely.
3. **Non-Opt-In Workspaces (e.g., shared monorepos, dotfiles)**: Whole-file or line-only linting in repositories without `.omnicoach.toml` triggers warnings on 36.1% of edits. Rather than disabling the hook in repositories where checking in `.omnicoach.toml` is prohibited (such as shared team directories), combining net-new differential scoping (`D2`) with ancestor `.omnicoach.toml` discovery (`D5`) and a planned user-level global fallback config (`~/.config/omnicoach/config.toml`) suppresses pre-existing legacy debt while preserving coaching on new edits.

### 2.3 The Multi-Step Edit Trajectory Test: Hard-Block vs. Ephemeral Coaching

In real programming sessions, code generation is an iterative sequence of partial edits ($s_1 \to s_2 \to \dots \to s_k$). We evaluated 150 multi-step edit sequences ($k \ge 2$) from real sessions to measure the impact of hard-blocking:

| Metric | Measured Value | Consequence of Hard-Blocking (`deny`) |
| :--- | :---: | :--- |
| **Transient Intermediate Violation Rate** | **74.7%** (112 of 150 sequences) | **In ~3 out of 4 multi-step tasks, hard-blocking creates a FALSE BLOCK.** An intermediate edit introduced a violation that was naturally resolved by a subsequent edit in the same turn or sequence. |
| **Top Transient Rules** | `single-letter-name` (141), `repeated-literal` (125), `private-before-public` (64), `undocumented-nontrivial-function` (63), `abbreviated-name` (49) | Intermediate refactorings temporarily order helper functions incorrectly or leave variables untyped while drafting. |
| **Discarded Payload Cost per Block** | **4,572 – 6,907 bytes (~1,150 – 1,750 tokens)** | A hard `deny` throws away the entire file payload, forcing the model into an expensive, brittle full-file rewrite. |

### 2.4 Diff Scoping Across 823 Historical Edit Transitions

Comparing modified-line filtering against whole-file linting across 823 transitions:

- **Pre-existing debt suppressed**: **43,420 violations** on unmodified lines were suppressed by `modified_lines`.
- **Non-local spillovers dropped by line-only filter**: **16,871 violations** introduced by an edit were anchored on unmodified lines.

### 2.5 Wall-Clock Latency Microbenchmarks

Measured over 30 iterations per scenario using the `omnicoach` release binary:

| Scenario | $p_{50}$ Latency | $p_{95}$ Latency | Max Latency | Impact on Agent Loop |
| :--- | :---: | :---: | :---: | :--- |
| **1. No-Op Tool Call** (`search_web`, `manage_task`) | **8.2 ms** | 13.4 ms | 13.8 ms | Imperceptible (<15 ms) |
| **2. Safe Shell Command** (`cargo test`) | **9.3 ms** | 13.8 ms | 19.9 ms | Imperceptible |
| **3. VCS Shell Command in `.jj` Repo** (`jj status`) | **9.5 ms** | 19.6 ms | 22.8 ms | Imperceptible (<25 ms) |
| **4. File Edit on Small File** (~80 LOC, `event.rs`) | **11.8 ms** | 21.1 ms | 21.6 ms | Imperceptible |
| **5. File Edit on Large File** (~880 LOC, `rule_selection.rs`) | **68.0 ms** | 105.2 ms | 107.2 ms | Smooth (<110 ms) |
| **6. `PreInvocation` Queue Drain** | **7.5 ms** | 26.0 ms | 38.1 ms | Imperceptible |

---

## 3. Study 2: Live 7-Arm Multi-Agent Prototype Benchmark

### 3.1 Experimental Setup

To evaluate how real LLM agents behave under different hook policies, we spawned 7 autonomous subagents (`benchmark-coder`) in parallel. Each agent operated in an isolated Git repository with an identical two-part Python coding challenge:

1. **Module 1 (`billing_ledger.py` + `test_billing_ledger.py`)**:
   - Contains 3 pre-existing legacy violations on lines 1–20 (`single-letter-name`, `abbreviated-name`, `mutable-default-arg`) that must be left untouched.
   - Contains a non-local line-42 trap: line 42 has `STATUS_SETTLED = "settled"`. When extending the module with payment reconciliation, introducing `"settled"` on line 80 triggers `[repeated-literal]` anchored on unmodified line 42.
2. **Module 2 (`retry_schedule.py` + `test_retry_schedule.py`)**:
   - Greenfield implementation of exponential backoff with jitter and error classification.
   - The user prompt mandated the exact function and class signatures:
     `RetryStep(attempt: int, delay_ms: int, reason: str)` and `calculate_delay(attempt: int, base_delay_ms: int, max_delay_ms: int) -> int`.
   - These parameter names deliberately trigger heuristic/opinionated rules: `[primitive-duration]` (prefer `datetime.timedelta`) and `[identical-positional-types]`.
   - The agent must either fix the violation or apply `# omni:ignore [<rule>] -- <reason>` without altering the requested public API signature.

### 3.2 Quantitative Results Across All 7 Benchmark Arms

| Arm | Configuration Architecture | Wall-Clock Time | Model Turns | Total Steps | Input Tokens | Output Tokens | File Edits | Denied Edits | `code --diff` Violations | Whole-Repo Net Violations | Preserved Requested API Signature? |
| :--- | :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| **Arm 1** | **Control (`no_hook`)** | 259.0 s | 31 | 67 | 208,074 | 39,495 | 4 | 0 | **4** | **14** | Yes |
| **Arm 2** | **End-of-Task Explicit CLI (`omnicoach code --diff`)** | 518.0 s (+100%) | 62 (+100%) | 144 | 275,527 | 40,069 | 4 (+46 shell) | 0 | **0** | **0** | Yes (with `# omni:ignore`) |
| **Arm 3** | **Hard-Block File Edits (`PreToolUse` `deny`)** | 588.0 s (+127%) | **85** (+174%) | **179** | **388,566** (+87%) | **75,542** (+91%) | **48** (12x) | **24** | **0** | **1** | **NO** (Broke signature to escape linter) |
| **Arm 4** | **Ephemeral Whole-File (`PreInvocation` All Lines)** | 249.0 s | 32 | 80 | 177,415 | 48,919 | 11 | 0 | **0** | 10 (Legacy noise diluted focus) | Yes |
| **Arm 5** | **Ephemeral Modified-Lines (Current Hook)** | 297.0 s | 37 | 85 | 179,291 | 47,938 | 12 | 0 | **0** | 7 (Missed line 42 spillover) | Yes |
| **Arm 6** | **Ephemeral Net-New (`modified_lines` $\cup$ Net-New Diagnostics)** | 270.0 s | 33 | 78 | 165,594 | 44,265 | 12 | 0 | **0** | 7 (Caught line 42 spillover) | Yes |
| **Arm 7** | **Optimal (Net-New Scoping + Guidance Footer)** | **195.0 s** (-25%) | **27** (-13%) | **62** | **151,132** (-27%) | **32,428** (-18%) | **7** | **0** | **0** | **5** (Cleanest, fastest completion) | **Yes** |

---

### 3.3 Four Decisive Behavioral & Causal Findings

#### 1. Hard-Blocking File Edits Paralyzes Agents and Degrades API Quality

- Arm 3 suffered **24 rejected edits** and burned **85 turns** and **464,108 tokens** (2.5x more than Arm 7).
- When `PreToolUse` hard-blocked the edit under `[primitive-duration]` and `[identical-positional-types]`, the model was trapped: the prompt strictly demanded `delay_ms: int`, but the linter denied the edit.
- Unable to bypass the block, the agent in Arm 3 mutated the public API (replacing `int` with `timedelta` and adding keyword-only `*` markers) and even attempted to edit `.omnicoach.toml` to delete the rule.
- **Conclusion**: Hard-blocking on heuristic code rules breaks developer intent and destroys API contracts.

#### 2. End-of-Task Explicit Lint Gates Double the Rework Cycle

- Arm 2 achieved 0 violations, but took **518.0 seconds** (more than 2.6x longer than Arm 7).
- Discovering all lint findings only at the end forced a complete second pass of file modifications, shell edits, and test reruns. In-flight ephemeral feedback eliminates late-stage rework by guiding the agent while it drafts code.

#### 3. Net-New Diagnostic Scoping Closes the Non-Local Gap

- In Arm 5 (modified lines only), extracting `STATUS_SETTLED = "settled"` caused `[repeated-literal]` on unmodified line 42. Arm 5 filtered by `modified_lines`, ignored line 42, and left the violation unresolved.
- In Arms 6 and 7, net-new differential scoping alerted the agent to the line-42 finding on the next turn. The agent extracted the constant cleanly without ever touching lines 1–20.

#### 4. The Actionable Guidance Footer Prevents Web-Search Thrashing

- In Arm 5, upon seeing `[primitive-duration]`, the agent executed 7 `search_web` queries searching for `# noqa` or suppression directives.
- In Arm 7, the guidance footer explicitly stated:
  `suppress it with omni:ignore [<rule>] -- <reason> rather than working around the linter and degrading the design.`
- The agent immediately added:
  `# omni:ignore [primitive-duration] -- Delay in milliseconds is required by the RetryStep specification`
  resulting in **0 violations in `retry_schedule.py` on the very first attempt**, finishing in 195.0 seconds and 27 turns.
