# Agent Coaching & Diagnostic Wording: Exploratory Investigation and Methodology Post-Mortem

## TL;DR

- **Root cause of lint-driven RICR regressions**: When an `omnicoach` rule's `Suggestion:` prescribes an unconditional syntactic edit (for example, *"or remove `pub use ...`"*, *"Extract the shared helper into the root module"*, or *"Split each arrange-act-assert phase into its own `test_*` function"*) without stating the boundary conditions under which that edit is invalid, coding agents treat the literal `Suggestion:` text as a mandatory instruction and degrade code quality (RICR) to silence the finding.
- **What we tested**:
  1. **Experiment 1 ($2 \times 2$ Internal Architecture Trial, 4 subagent runs)**: Crossed diagnostic `Suggestion:` wording with repository-local `AGENTS.md` instructions on `CodeLintProject` facade preservation and `CodeLintAst` submodule layering. Boundary-conditional `Suggestion:` templates raised facade and layer preservation from `0%` to `100%` with zero tool-call overhead (`17` vs `20` calls), whereas `AGENTS.md` instructions paired with baseline diagnostics caused a **`3.5x` wall-clock slowdown (`647 s` vs `183 s`)** due to instruction conflict.
  2. **Experiment 2 (External-Repository Multi-Rule Trial, 3 binary variants $\times$ 2 rounds = 6 subagent runs)**: Tested 3 compiled `omnicoach` binaries (`Baseline`, `Footer Only`, `Footer + Boundary-Conditional Templates`) across 5 tasks (17 findings across 10 rules) in a standalone Rust + Python repository (`ext_service`), with and without source-level hint comments. In the unhinted round, `Baseline` attempted to launder a sibling import through the parent facade (`use super::Invoice;`), was blocked by [`resolve_intra_component_reexports`](../../../src/code_lint/project/rust/architecture.rs), and spent `48,961` output/thinking tokens (`15m 41s`, 14 `--explain` calls) recovering; `Footer + Boundary-Conditional Templates` solved all 17 findings in 1 edit pass (`16,133` tokens, `7m 05s`).
- **Methodological mistakes made during this exploratory investigation**:
  1. **Repository-local instruction leakage (`AGENTS.md`)**: Experiment 1 initially tested adding coaching rules to `custom_lints/AGENTS.md`, which provides zero benefit to agents running `omnicoach` in external repositories.
  2. **Overfitting the candidate footer to the benchmark tasks ("teaching to the test")**: In Experiment 2, the candidate `FIX_NOTE` and `SUPPRESSION_NOTE` were drafted alongside the 5 benchmark tasks and explicitly listed the exact 5 traps in those tasks (*"dropping assertions, widening visibility (`pub`), exposing internal modules, or breaking public/serialization contracts"* and *"external wire/API field, a distinct `None` state, a multi-step state transition test"*). That confounded the `Footer Only` condition, measuring task-example memorization rather than zero-shot generalization.
  3. **No train/holdout split across rules**: Every rule in Experiment 2 was either named in the overfitted footer or had its `ViolationTemplate::suggestion` edited for the experiment; no held-out rules were tested.
  4. **Unblinded single-sample ($N = 1$) cells**: Both the prompt interventions and the synthetic benchmark fixture were authored in the same session with $N = 1$ run per cell.
- **Resolution & Next Steps**: We replaced `FIX_NOTE` and `SUPPRESSION_NOTE` in [src/rule_catalog.rs](../../../src/rule_catalog.rs) with a rule-agnostic footer (including guidance to report false positives or misleading suggestions/documentation to `omnicoach`), kept rule-specific boundary conditions inside each rule's own `ViolationTemplate::suggestion`, and added a dedicated research item to [ROADMAP.md](../../../ROADMAP.md) to design a double-blind, held-out agent evaluation methodology before further tuning coaching prompts.

---

## 1. Problem Statement: Why Coding Agents Game or Degrade Code Under Lint Pressure

When an AI coding agent runs `omnicoach code` (or receives queued warnings from `omnicoach hook`), three signals appear in its context window:

1. **Per-finding `ViolationTemplate` lines**: `Summary`, `Rationale:`, `Suggestion:`, and optional `Or:` comment hints.
2. **Report footer notes** ([src/rule_catalog.rs](../../../src/rule_catalog.rs)): `FIX_NOTE` (printed on every plain report) and `SUPPRESSION_NOTE` (printed when any reported rule has `Precision::Heuristic` or `Consensus::Opinionated`).
3. **On-demand rule documentation**: `omnicoach --explain <rule>`, which agents only invoke when the inline diagnostic leaves a question unanswered.

We observed two distinct failure modes when agents resolved `omnicoach` findings:

- **Destructive Compliance on Boundary Exceptions**: When `FIX_NOTE` states *"Note: Fix each finding by applying its suggestion."* and a rule's `Suggestion:` prescribes an unconditional transformation (such as removing `pub use` from a component root, splitting a stateful multi-step lifecycle test into separate functions, or replacing a wire-schema `u64` field with `Duration`), the agent executes the destructive transformation even when it breaks external callers, facade encapsulation, or test coverage.
- **Exploratory Thrashing When Mechanism Syntax Is Omitted**: When a rule flags an architectural violation (`sibling-module-dependency` or `reexported-public-item`) whose `Suggestion:` recommends an invalid local workaround (*"Extract the shared helper into the root module"*) instead of naming the declarative mechanism (`modules: { ... }` / `submodules`), the agent first tries a syntactic workaround (such as rewriting `use super::invoice::Invoice;` to `use super::Invoice;`), fails when `omnicoach` catches the facade-laundered import, and then spends 5–14 discovery tool calls (`--help`, `--list-rules`, `--explain`) searching for the configuration syntax.

---

## 2. Exploratory Experiments & Quantitative Results

### 2.1 Experiment 1: $2 \times 2$ Internal Architecture Trial (Diagnostic Wording $\times$ `AGENTS.md`)

**Context**: Before recursive submodule DAGs (`modules: { ... }`) were added, splitting `CodeLintProject` into 4 top-level components triggered `unmapped-architecture-module` and `reexported-public-item` on [src/code_lint/project.rs](../../../src/code_lint/project.rs), causing an agent to delete `pub use` from `project.rs` and rewrite ~40 caller imports across the crate to `crate::code_lint::project::context::ProjectContext`.

We tested 4 isolated subagents across two independent variables on two scenarios (`CodeLintProject` facade re-exports and `CodeLintAst` internal submodule layering):

- **Diagnostic `Suggestion:` Wording**: Baseline production templates vs. boundary-conditional templates warning against leaking internal layer paths or collapsing layered submodules into a monolithic root.
- **Repository-Local Guidelines (`AGENTS.md`)**: Baseline `AGENTS.md` vs. adding explicit self-dogfooding and facade-preservation rules in `AGENTS.md`.

| Condition | Preserves `project::*` Facade? | Preserves `ast` Submodule Layers? | Diagnoses Linter Engine Limitation? | Planner Turns | Tool Calls | Wall-Clock Time |
| :--- | :--- | :--- | :--- | ---: | ---: | ---: |
| **Baseline Diagnostics + Baseline `AGENTS.md`** | **No (`0%`)** — deletes `pub use`, leaks `project::context::*` | **No (`0%`)** — collapses layers into 14 flat root `mod`s | **No (`0%`)** | 7 | 20 | `83 s` |
| **Boundary-Conditional Diagnostics Only** | **Yes (`100%`)** — keeps single `CodeLintProject` facade | **Yes (`100%`)** — keeps 4-layer submodule structure | **Partial (`50%`)** | 10 | 17 | `183 s` |
| **Self-Dogfooding `AGENTS.md` Only** | **Yes (`100%`)** | **Yes (`100%`)** | **Yes (`100%`)** | 20 | 45 | `647 s` (`3.5x` slower) |
| **Combined Diagnostics + `AGENTS.md`** | **Yes (`100%`)** | **Yes (`100%`)** | **Yes (`100%`)** | 8 | 17 | `222 s` |

**Analysis**: Updating the inline `Suggestion:` text flipped architectural preservation from `0%` to `100%` without extra tool calls (`17` vs `20`), whereas leaving the misleading `Suggestion:` text in place and contradicting it in `AGENTS.md` forced the agent to spend `45` tool calls and `647 s` reconciling the conflict.

### 2.2 Experiment 2: External-Repository Multi-Rule Trial (`3 Variants` $\times$ `2 Rounds`)

**Context**: Because changes to `custom_lints/AGENTS.md` do not help agents running `omnicoach` in external repositories, we constructed a standalone Rust + Python repository (`ext_service`, no `AGENTS.md`) with **5 tasks and 17 lint findings across 10 rules**:

1. **Webhook Wire Schema vs. Internal Duration (`src/webhook.rs`)**: `primitive-duration` (3) + `abbreviated-name` (1) on internal `retry_delay` (genuine `Duration` refactor) vs. serialized `WebhookPayload { timeout_ms: u64, msg: String }` (external JSON wire contract).
2. **Tri-State Cache Lookup (`cache.py`)**: `concrete-collection-return` (1) + `nullable-collection-return` (1) on `get_cached_permissions(...) -> list[str] | None` (narrow `list[str]` $\rightarrow$ `Sequence[str]`, but preserve `| None` because `None` cache miss $\neq$ `()` empty permissions).
3. **Stateful Order Lifecycle Test (`checkout.py`, `test_checkout.py`)**: `undocumented-nontrivial-function` (1) + `multi-phase-test` (1) + `too-many-assertions` (1) on `calculate_tiered_receipt`, stateful `Order` transition test (`authorize()` $\rightarrow$ assert `"authorized"` $\rightarrow$ `capture()` $\rightarrow$ assert `"captured"`), and 5 field assertions on `Receipt`.
4. **Macro-Before-Public Declaration Order (`src/codec.rs`)**: `private-before-public` (2) + `type-suffixed-name` (4) where `macro_rules! decode_header_field` must remain above `pub fn decode_frame` while private helpers move below it without widening visibility to `pub`.
5. **Intra-Component Submodule Facade (`src/architecture.rs`, `src/billing*`)**: `reexported-public-item` (2) + `sibling-module-dependency` (1) on `Billing` facade (`mod invoice; mod ledger; pub use ...` + `modules: { invoice => [], ledger => [invoice] }`).

We compared 3 compiled `omnicoach` binaries across **Hinted Source Comments** and **Unhinted Source Files**:

| Condition | 10-Check RICR & Mutation Score | Stateful `multi-phase-test` Outcome | `Billing` Facade & `modules` Outcome | Planner Turns | Output + Thinking Tokens | Wasted Edit Cycles | Wall-Clock Time |
| :--- | :---: | :--- | :--- | ---: | ---: | ---: | ---: |
| **Hinted — `Baseline`** | **9 / 10** (`90%`) | **Failed** — split stateful `Order` lifecycle test into 2 functions with duplicated setup | Passed (copied `modules:` from macro arm in `architecture.rs`) | 6 | `17,679` | 0 | `5m 36s` |
| **Hinted — `Footer Only` (Overfitted)** | **10 / 10** (`100%`) | **Passed** (`omni:ignore`) | Passed | 6 | `16,581` | 0 | `5m 14s` |
| **Hinted — `Footer + Boundary-Conditional Templates`** | **10 / 10** (`100%`) | **Passed** (`omni:ignore`) | Passed | **5** | **`13,187`** | 0 | **`4m 55s`** |
| **Unhinted — `Baseline`** | **10 / 10** (after retry) | Passed (ran `--explain multi-phase-test`) | **Tried facade laundering (`use super::Invoice;`)**, blocked by [`resolve_intra_component_reexports`](../../../src/code_lint/project/rust/architecture.rs), then ran 14 `--explain` calls | 12 | `48,961` | **1 broken edit** | `15m 41s` |
| **Unhinted — `Footer Only` (Overfitted)** | **10 / 10** (`100%`) | Passed | Spent 5 extra CLI discovery calls (`graph`, `--help`, `--list-rules`, `--explain`) to find `modules: { ... }` | 9 | `19,607` | 0 | `7m 47s` |
| **Unhinted — `Footer + Boundary-Conditional Templates`** | **10 / 10** (`100%`) | **Passed** | **Solved in 1 edit pass** (`modules: { ... }` named in `Suggestion:`) | **7** | **`16,133`** (`-67%`) | **0** | **`7m 05s`** (`-55%`) |

**Analysis**: Naming the declarative mechanism (`modules: { ... }`) and private `mod` encapsulation directly inside the architecture rules' `Suggestion:` lines eliminated both the invalid `use super::Invoice;` edit attempt and the 5–14 exploratory CLI discovery calls in the unhinted round, cutting output/thinking tokens by `67%` (`16,133` vs `48,961`) and wall-clock time by `55%` (`7m 05s` vs `15m 41s`).

---

## 3. Methodological Post-Mortem: Mistakes Made During Benchmarking

Critically auditing our own experimental setup exposed four methodological flaws that must be corrected in future agent-coaching benchmarks:

### Mistake 1: Testing Repository-Local `AGENTS.md` Rules Instead of Universal Tool Surfaces

In Experiment 1, the second factor modified `custom_lints/AGENTS.md`. While useful for self-dogfooding inside `custom_lints`, repository-local `AGENTS.md` instructions do not ship with the `omnicoach` binary and cannot help agents in external repositories or on rules not covered by that repo's `AGENTS.md`.

### Mistake 2: Overfitting the Global Footer to the Benchmark Fixture ("Teaching to the Test")

In Experiment 2, we drafted the candidate `FIX_NOTE` and `SUPPRESSION_NOTE` in the same step as the 5 benchmark tasks:

- The candidate `FIX_NOTE` enumerated *"dropping assertions, widening visibility (`pub`), exposing internal modules, or breaking public/serialization contracts"* (matching the exact traps in the stateful lifecycle test, macro declaration-order, billing facade, and webhook wire-schema tasks).
- The candidate `SUPPRESSION_NOTE` enumerated *"an external wire/API field, a distinct `None` state, a multi-step state transition test"* (matching the exact exceptions in the webhook, cache lookup, and lifecycle test tasks).

Because the global footer enumerated the exact 5 traps in the benchmark fixture, the `Footer Only` condition was not a valid test of a general footer—it tested whether an agent obeys an explicit cheat sheet for the 5 benchmark tasks. Furthermore, hardcoding a list of rule-specific exceptions inside `FIX_NOTE` / `SUPPRESSION_NOTE` violates separation of concerns: rule-specific boundary conditions belong in each rule's `ViolationTemplate::suggestion` and `--explain <rule>`, while the global footer must remain concise and rule-agnostic.

### Mistake 3: No Train / Holdout Split Across Rules

Every rule triggered in Experiment 2 was either named in the overfitted footer or had its `ViolationTemplate::suggestion` directly edited in `Footer + Boundary-Conditional Templates`. We did not include a **held-out set of rules** (rules whose `ViolationTemplate` was untouched and whose domain was not mentioned in the footer, such as `boolean-parameter-default`, `magic-number`, `single-method-class`, or `try-except-in-loop`) to test whether the general footer transfers to unseen rules.

### Mistake 4: Unblinded Fixture Construction and $N = 1$ Sample Size per Cell

The same agent session designed the prompt intervention, constructed the synthetic 5-task fixture, and knew the 10-check evaluation oracle in advance, running $N = 1$ subagent per condition. Because LLM tool-use trajectories have run-to-run variance (for instance, in the unhinted round `Baseline` ran `--explain multi-phase-test` on Turn 2 whereas in the hinted round `Baseline` did not), $N = 1$ cannot distinguish a reliable behavioral shift from sampling noise on individual tasks.

---

## 4. Shipped Non-Overfitted Footer & Separation of Concerns

To eliminate the overfitting identified in §3 while preserving the clarity gains from boundary-conditional rule templates:

1. **Rule-agnostic `FIX_NOTE` and `SUPPRESSION_NOTE`** in [src/rule_catalog.rs](../../../src/rule_catalog.rs#L63-L79):
   - `FIX_NOTE` states the universal principle (fix the underlying design issue rather than reshaping code around thresholds or patterns, and never degrade behavior, encapsulation, or external contracts to silence a finding) without listing benchmark-specific traps.
   - `SUPPRESSION_NOTE` states the universal configuration and suppression workflow (`.omnicoach.toml`, `Or:` explanation comments, `omni:ignore [<rule>] -- <reason>`, and reporting false positives or misleading suggestions/documentation to `omnicoach`).
2. **Rule-specific boundary conditions in `ViolationTemplate::suggestion`**:
   - Rules whose default refactoring advice has a common architectural or domain boundary (`primitive-duration`, `nullable-collection-return`, `abbreviated-name`, `multi-phase-test`, and the 7 `Topic::ARCHITECTURE` rules) state that boundary condition and the exact configuration/declaration syntax (`modules: { ... }`, `remove-banned`, boundary conversion) directly in their `Suggestion:` line.

---

## 5. Next Steps

Follow the two-phase evaluation plan tracked in [ROADMAP.md](../../../ROADMAP.md#4-cli-ux--reporting) (*Universal Agent Coaching & Double-Blind Diagnostic Prompt Benchmarking*):

1. **Phase 1 (Methodology Design)**: Establish a train/holdout split across rules and target repositories, decouple benchmark fixture construction from prompt authoring (double-blind protocol), define bidirectional error metrics (destructive compliance rate vs. false suppression rate), and size $N$ for statistical power across model tiers.
2. **Phase 2 (Held-Out Benchmark)**: Evaluate any future changes to `FIX_NOTE`, `SUPPRESSION_NOTE`, `ViolationTemplate`, `--explain`, and `omnicoach hook` payloads against held-out rules and external repositories before shipping.
