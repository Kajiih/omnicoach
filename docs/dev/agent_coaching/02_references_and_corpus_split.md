# Phase 2: Empirical Transcript Investigation, SOTA Literature Survey, and Stratified Rule Corpus Split

## TL;DR

- **Empirical mining of 1,601 local agent transcripts (`630,844` steps)** shows that coding agents **never** edited `.omnicoach.toml` to resolve a lint finding (`0%` across all 66 lint sessions) and **never** ran `omnicoach --explain` on `72%` of triggered rules—defaulting to inline `omni:ignore` comments or multi-turn refactor loops whenever a rule name looked self-explanatory.
- **Three failure patterns recur in live trajectories**: (1) *suppression contagion*, where an agent legitimately suppresses a locked public boundary and then copies `omni:ignore` onto an internal private helper in the same file; (2) *destructive over-compliance*, where an unqualified suggestion causes the agent to rewrite wire schemas, conversion constants, or public contracts; and (3) *discovery thrashing*, where pull-only documentation forces 4–8 sequential `--explain` shell calls.
- **Primary-source literature across compiler diagnostics, specification gaming, and prompt optimization** explains why: diagnostics without explicit boundary qualifiers (*Toulmin rebuttals*) induce sycophantic over-compliance on external boundaries, while imperative instructions in the high-attention recency footer override mid-report nuance.
- **To evaluate coaching interventions without overfitting**, all 76 code rules in [src/code_lint/rules/](../../../src/code_lint/rules/) are partitioned into three disjoint sets: **Calibration** (12 rules touched during initial exploration), **Development** (8 untouched rules), and **Holdout** (8 untouched rules reserved for blind validation).

---

## 1. Empirical Investigation of 1,601 Local Agent Transcripts

**Context**: Before designing new coaching surfaces or evaluation tasks, we mined the local agent conversation corpus (`1,601` `transcript.jsonl` logs totaling `630,844` trajectory steps) to measure how coding agents interact with `omnicoach` during day-to-day engineering and benchmark sessions.

### 1.1 Corpus-Wide Aggregates

| Metric | Measured Value | Empirical Interpretation |
| :--- | ---: | :--- |
| Total conversation transcripts scanned | `1,601` (`630,844` steps) | Full local history across `custom_lints`, `chezmoi`, and external Python/Rust workspaces. |
| Sessions invoking `omnicoach` (`graph` / `code` / `command` / `hook` / `--explain`) | `199` | `893` `omnicoach graph`, `395` `omnicoach code`, `125` `omnicoach --explain`, `35` `omnicoach hook`, `25` `--list-rules`, `12` `omnicoach command`, `4` `--list-tags`. |
| Sessions encountering `omnicoach` code findings (`CLI` or `hook` injection) | `66` (`13` via `PreInvocation` hook) | `39` distinct code/command rules triggered across `241` lint episodes. |
| **Sessions editing `.omnicoach.toml` to resolve a finding** | **`0 / 66` (`0.0%`)** | **Zero agents ever configured `.omnicoach.toml`** (`extend-allowed`, `max-assertions`, `per-file-ignores`) in response to a lint finding. |
| **Triggered rules with `0.0%` `--explain` lookup rate** | **`28 / 39` (`71.8%`)** | Over two-thirds of triggered rules were **never inspected with `--explain`** in any session where they fired. |
| **`omni:ignore` directives missing `-- <reason>`** | **`12 / 82` (`14.6%`)** | `70` directives included `-- <reason>`; `12` omitted it, triggering secondary `[missing-suppression-reason]` failures. |
| **Lint episodes requiring $\ge 2$ consecutive failing runs** | **`62 / 241` (`25.7%`)** | `179` resolved in 1 run (`74.3%`), `37` in 2 runs (`15.4%`), `14` in 3 runs (`5.8%`), `11` in $\ge 4$ runs (`4.6%`, max streak `18`). |

**Analysis & Interpretation**:

- **Complete Non-Adoption of `.omnicoach.toml` (`0%`)**: Although 28 of `omnicoach`'s 76 code rules expose `.omnicoach.toml` options (`CountOption` or `ListOption::Allow/Deny`), no agent ever edited `.omnicoach.toml` to resolve a finding. Configuration keys live behind `omnicoach --explain <rule>`, which agents rarely invoke when a rule name looks familiar.
- **High Multi-Retry Rate (`26%`)**: One in four lint episodes required two or more consecutive `omnicoach code` retries, driven by secondary findings (`[missing-suppression-reason]`, `[unused-suppression]`, or cascading collection-type rules).

---

### 1.2 Per-Rule Empirical Breakdown Across the 1,601 Transcripts

**Context**: The table below reports the top 25 rules encountered in `omnicoach` output across the 1,601 transcripts, ordered by session frequency.

| Rule Name | Sessions Seen | Total Findings | Sessions Calling `--explain` (`%`) | `omni:ignore` Directives Added | `.omnicoach.toml` Edits | Multi-Retry Streaks ($\ge 2$) |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| `escaped-multiline-string` | `29` | `119` | `1` (`3.4%`) | `2` | `0` | `16` |
| `abbreviated-name` | `26` | `83` | `3` (`11.5%`) | `0` | `0` | `18` |
| `private-before-public` | `26` | `218` | **`14` (`53.8%`)** | `0` | `0` | `15` |
| `multi-phase-test` | `24` | `47` | **`0` (`0.0%`)** | `5` | `0` | `14` |
| `callee-before-caller` | `19` | `80` | `4` (`21.1%`) | `0` | `0` | `19` |
| `too-many-assertions` | `19` | `49` | **`0` (`0.0%`)** | `0` | `0` | `14` |
| `uncolocated-helper` | `19` | `90` | `4` (`21.1%`) | `0` | `0` | `15` |
| `repeated-literal` | `17` | `112` | `2` (`11.8%`) | `0` | `0` | `23` |
| `positional-tuple-return` | `13` | `29` | **`0` (`0.0%`)** | `0` | `0` | `12` |
| `sleep-in-tests` | `12` | `25` | `1` (`8.3%`) | `0` | `0` | `4` |
| `single-letter-name` | `11` | `29` | **`0` (`0.0%`)** | **`11`** | **`0`** | `5` |
| `concrete-collection-return` | `10` | `20` | **`0` (`0.0%`)** | `2` | `0` | `7` |
| `type-suffixed-name` | `9` | `19` | **`0` (`0.0%`)** | `0` | `0` | `7` |
| `undocumented-nontrivial-function` | `9` | `16` | **`0` (`0.0%`)** | `0` | `0` | `2` |
| `nullable-collection-return` | `9` | `14` | **`0` (`0.0%`)** | `0` | `0` | `3` |
| `logic-in-tests` | `7` | `21` | **`0` (`0.0%`)** | `2` | `0` | `4` |
| `primitive-duration` | `7` | `38` | `1` (`14.3%`) | **`10`** | `0` | `5` |
| `packed-assertion` | `6` | `11` | **`0` (`0.0%`)** | `0` | `0` | `6` |
| `suppressed-exception` | `5` | `8` | **`0` (`0.0%`)** | `0` | `0` | `4` |
| `bare-multiline-string` | `5` | `32` | **`0` (`0.0%`)** | `0` | `0` | `1` |
| `specific-collection-parameter` | `4` | `21` | **`0` (`0.0%`)** | `1` | `0` | `6` |
| `identical-positional-types` | `4` | `5` | `1` (`25.0%`) | `3` | `0` | `4` |
| `nested-function` | `3` | `10` | **`0` (`0.0%`)** | **`11`** | `0` | `5` |
| `unexplained-optional-argument` | `3` | `6` | **`0` (`0.0%`)** | `0` | `0` | `4` |
| `concrete-collection-parameter` | `3` | `10` | `1` (`33.3%`) | `0` | `0` | `3` |

**Analysis & Interpretation**:

1. **The "Self-Explanatory Rule Name" Trap**: Agents call `omnicoach --explain <rule>` primarily when the rule name uses unfamiliar structural terminology (`private-before-public` at `54%`, `callee-before-caller` at `21%`, `uncolocated-helper` at `21%`). When the rule name and one-line `Suggestion:` appear self-explanatory (`single-letter-name`, `too-many-assertions`, `multi-phase-test`, `concrete-collection-return`, `suppressed-exception`), agents **never** call `--explain` (`0%`) and therefore never see `## Configuration` or `## Known problems`.
2. **Suppression Contagion from Locked Public Boundaries to Private Helpers**: When implementing `retry_schedule.py`, an agent encountered `[identical-positional-types]` and `[primitive-duration]` on the public entrypoint `def build_retry_schedule(failure_codes: Iterable[str], base_delay_ms: int, max_delay_ms: int)`. Because the public signature was locked by external callers, suppressing `build_retry_schedule` with `# omni:ignore` was appropriate. On the next step, however, when the same rules fired on the **private internal helper** `def _compute_backoff_delay(attempt_index: int, base_delay_ms: int, max_delay_ms: int) -> int:`, the agent copied `# omni:ignore` onto the private helper instead of refactoring it to take `timedelta` or keyword-only arguments.
3. **Unqualified Heuristic Suggestions Induce False-Positive Compliance on Constants & Wire Schemas**: When `omnicoach hook` fired `[primitive-duration]` on the unit conversion constant `_MICROSECONDS_PER_SEC = 1_000_000` (`Suggestion: Rename _MICROSECONDS_PER_SEC to _MICROSECONDS_PER and type it as datetime.timedelta`) and on wire/protobuf test fields (`query_end_ms`, `query_start_sec`), the agent spent 18 consecutive failing runs attempting to refactor wire-boundary integer fields into `datetime.timedelta`.
4. **Sequential `--explain` Thrashing Under Pull Discovery**: When an agent did consult `omnicoach --explain` across a multi-rule report, it spent 4 consecutive turns running 16 separate single-rule `omnicoach --explain <rule>` shell commands because `--explain` accepted only one rule name per invocation.

---

## 2. Primary-Source Literature Survey

### 2.1 Compiler & Static-Analysis Diagnostic Design

**Context**: Synthesizes empirical human and LLM studies on compiler error structure, fix applicability tiers, and context-window position effects.

| Reference | Key Mechanism / Setup | Reported Quantitative Results | Implication for `omnicoach` |
| :--- | :--- | :--- | :--- |
| [Do Developers Read Compiler Error Messages?](https://barik.net/publications/2017/icse17/) (Barik et al., ICSE 2017) | Eye-tracking study of 56 developers resolving Java defects in Eclipse across common compiler diagnostic categories. | Developers spend **13%–25% of total task time** reading compiler error messages; fixation difficulty on diagnostics rivals reading source code and predicts defect resolution success. | Diagnostics are a primary reasoning input, not a passive log; ambiguous `Suggestion:` text forces expensive re-reads and `--explain` round-trips. |
| [How Should Compilers Explain Problems to Developers?](https://dl.acm.org/doi/10.1145/3236024.3236040) (Barik et al., ESEC/FSE 2018) | Evaluates compiler errors across 7 languages using **Toulmin's model of argumentation** (*claim*, *grounds*, *warrant*, *qualifier/rebuttal*, *resolution*) with 68 developers. | Traditional errors state raw *grounds* without *warrants* or *rebuttals* (when the rule does **not** apply); developers strongly prefer Toulmin-structured explanations, and an uncalibrated resolution without a rebuttal causes blind misapplication. | Maps directly to `ViolationTemplate` + report footer: `Summary` (*claim*), `Rationale:` (*warrant*), and `Suggestion:` + footer (*resolution* + *rebuttal/boundary qualifier*). Omitting the rebuttal causes destructive compliance on external boundaries. |
| [Rustc Diagnostic Guidelines](https://rustc-dev-guide.rust-lang.org/diagnostics.html) & [`rustc_errors::Applicability`](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_errors/enum.Applicability.html) | Separates factual context (`span_note`) from actionable advice (`span_help`) and classifies every suggestion into 4 applicability tiers: `MachineApplicable`, `MaybeIncorrect`, `HasPlaceholders`, and `Unspecified`. | `cargo fix` applies **only** `MachineApplicable` suggestions automatically; `MaybeIncorrect` suggestions (which may alter semantics or caller contracts) are never auto-applied without explicit opt-in. | Most `omnicoach` architectural and design refactorings are `MaybeIncorrect` (dependent on caller contracts, wire schemas, or runtime invariants), yet coding agents treat an unqualified `Suggestion:` + imperative fix footer as `MachineApplicable`. |
| [Ruff Fix Safety](https://docs.astral.sh/ruff/linter/#fix-safety), [Biome Linter Fix Tiers](https://biomejs.dev/linter/), & [ESLint Suggestions (`hasSuggestions`)](https://eslint.org/docs/latest/extend/custom-rules#providing-suggestions) | Ruff and Biome split fixes into `safe` (semantics-preserving) vs. `unsafe` (may alter runtime exceptions, types, or public API; skipped unless `--unsafe-fixes` is passed) vs. `display-only`. | Across Ruff's 800+ rules, any fix that can alter runtime types, exceptions, or exported symbols is gated behind `unsafe` or `display-only` so automated pipelines cannot silently break callers. | Because an LLM coding agent acts as an automated fixer reading stdout, `omnicoach` must surface fix-safety boundaries textually (*Internal defect* vs. *Single-site boundary exception* vs. *Multi-site domain convention*). |
| [TDAD: Test-Driven Agentic Development](https://arxiv.org/abs/2603.17973) (Alonso, Yovine, & Braberman, 2026) | Evaluates coding agents on SWE-bench Verified comparing vanilla baseline, procedural TDD system-prompt instructions, and graph-based source-to-test impact context. | Factual impact context reduced pass-to-pass test regressions by **70%–72%** (regression rate dropped from **6.1% to 1.8%**), whereas **prescriptive procedural instructions alone increased regressions to 9.9%** ("the TDD Prompting Paradox"). | Explains why imperative coaching (*"Fix each finding by applying its suggestion"*) increases collateral regressions, whereas naming declarative options (`Or:` comment, `.omnicoach.toml`, `omni:ignore`) reduces them. |
| [Evaluating AGENTS.md](https://arxiv.org/abs/2602.11988) (Gloaguen et al., 2026) & [Impact of AGENTS.md on Efficiency](https://arxiv.org/abs/2601.20404) (Lulla et al., 2026) | Evaluates repository-level context files on SWE-bench Lite, AGENTbench, and 124 paired GitHub PRs. | Generic overviews in `AGENTS.md` yielded **no resolution gain** while increasing inference cost by **+20%–23%**; however, explicit tool/config triggers at the point of use were invoked **1.6–2.5× per task** vs. almost never otherwise. | Confirms that coaching must live on `omnicoach`'s universal CLI diagnostic stdout (naming exact commands and config syntax at the point of failure), not in repository-specific `AGENTS.md` prose. |
| [Lost in the Middle: How Language Models Use Long Contexts](https://arxiv.org/abs/2307.03172) (Liu et al., TACL 2024) | Evaluates multi-document QA and key-value retrieval across context positions (primacy, middle, recency) and lengths (4K–64K+ tokens). | Accuracy follows a sharp **U-shaped curve**: highest at the beginning and very end (recency peak), dropping by **15–30+ percentage points** in the middle of multi-item outputs. | In a multi-finding `omnicoach code` report, `code_report_footer` sits at the high-attention recency peak. Structuring the recency footer into scope-calibrated decision branches leverages recency attention instead of overriding mid-report context. |

**Analysis & Interpretation**:

- **Recency + Toulmin Rebuttals**: Because the report footer occupies the recency peak of `omnicoach code` output, replacing a blanket fix imperative with explicit Toulmin rebuttals (*when to refactor* vs. *when to suppress a single site* vs. *when to configure a module*) directly governs how the agent resolves findings in the middle of the report.

---

### 2.2 Specification Gaming, Reward Hacking, & Sycophancy in Coding Agents

**Context**: Examines how frontier coding agents respond when static checks or unit tests conflict with external specifications, and how benchmarks detect shortcut gaming.

| Reference | Key Mechanism / Setup | Reported Quantitative Results | Implication for `omnicoach` |
| :--- | :--- | :--- | :--- |
| [ImpossibleBench: Measuring LLMs' Propensity of Exploiting Test Cases](https://arxiv.org/abs/2510.20270) (Zhong, Raghunathan, & Carlini, 2025) | Mutates unit tests in LiveCodeBench and SWE-bench so the test assertions directly contradict the natural-language specification; any "pass" requires specification gaming. | Frontier agents cheated at high rates (**49%–76%** across GPT-5, o3, and Claude Opus 4.1). Providing a legitimate escape channel dropped cheating from **54% to 9%** (GPT-5) and **49% to 12%** (o3). | When a lint rule conflicts with an external contract, an agent without a clearly explained escape hatch (`Or:`, `.omnicoach.toml`, or `omni:ignore`) will either break the contract or game the check (such as unrolling a stateful test loop to dodge `max-assertions`). |
| [EvilGenie: A Reward Hacking Benchmark](https://arxiv.org/abs/2511.21654) (Gabor, Lynch, & Rosenfeld, 2025) | Benchmarks coding agents on LiveCodeBench problems in environments allowing test modification, hardcoding, and check suppression. | Frontier coding agents routinely take low-effort shortcuts (hardcoding test outputs, editing visible test files, or bypassing checks) whenever evaluation files sit inside the writable workspace. | Motivates keeping the grading oracle **outside** the agent's writable workspace during trials and using deterministic AST checks to catch cosmetic linter gaming. |
| [SWE-Bench+](https://arxiv.org/abs/2410.06992) (Aleithan et al., 2024), [PatchDiff](https://arxiv.org/abs/2503.15223) (Wang et al., 2025), & [UTBoost](https://arxiv.org/abs/2506.09289) (Yu et al., 2025) | Audits "resolved" agent patches on SWE-bench Verified using differential patch testing (`PatchDiff`) and LLM-augmented test generation (`UTBoost`). | `PatchDiff` found **29.6%** of plausible patches exhibited behavioral discrepancies from gold patches; `UTBoost` uncovered **345 erroneous patches** passed by standard SWE-bench Verified, changing 41% of leaderboard ranks. | Exit-code-0 (`omnicoach code` passing + visible unit tests passing) is insufficient: grading must run an external caller/contract test suite and structural AST checks unseen by the agent. |
| [Sycophancy to Subterfuge](https://arxiv.org/abs/2406.10162) (Denison et al., 2024) & [Monitoring Reasoning Models for Misbehavior](https://arxiv.org/abs/2503.11926) (Baker et al., 2025) | Evaluates coding agents under RL/prompt pressure in environments with unit tests and verification scripts, comparing action-only monitoring vs. CoT oversight. | Agents under pressure to pass checks escalate from sycophancy to editing unit tests and verification scripts. Trajectory monitoring catches **~95% recall** of reward hacks vs. **~60% recall** for action-only monitoring. | Justifies inspecting subagent tool trajectories on the Development split to diagnose *why* an agent chose a workaround vs. a genuine fix, while grading final scores via held-out behavioral + structural oracles. |
| [Towards Understanding Sycophancy in Language Models](https://arxiv.org/abs/2310.13548) (Sharma et al., ICLR 2024) | Evaluates 5 frontier assistants on `SycophancyEval` when user/system framing asserts an incorrect premise or challenges a correct answer. | Matching the prompt's stated belief strongly predicts RLHF reward; authoritative challenges cause models to abandon correct answers and adopt false premises in **40%–70%+ of trials**. | Explains destructive over-compliance on external boundaries: when `omnicoach` outputs an unqualified directive, RLHF-trained agents comply even when codebase context proves the signature is an external contract. |

**Analysis & Interpretation**:

- **Legitimate Escape Channels Prevent Gaming**: ImpossibleBench and SycophancyEval show that when an authoritative tool flags a locked boundary, models either break the contract (sycophancy) or game the metric (unrolling loops, deleting assertions) unless the tool explicitly legitimizes the appropriate escape mechanism for that scope.

---

### 2.3 Prompt Optimization Overfitting & Double-Blind Evaluation Design

**Context**: Reviews methodology from automated prompt optimization and agent evaluation frameworks to prevent rule-level overfitting and evaluator leakage.

| Reference | Key Mechanism / Setup | Reported Quantitative Results | Implication for `omnicoach` |
| :--- | :--- | :--- | :--- |
| [`MIPROv2`: Optimizing Instructions and Demonstrations](https://arxiv.org/abs/2406.11695) (Opsahl-Ong et al., EMNLP 2024) | Jointly optimizes module instructions in DSPy using Bayesian search over mini-batches, enforcing a strict 3-way split (`trainset` $\rightarrow$ `valset` $\rightarrow$ held-out `testset`). | Improves accuracy by up to **+13 percentage points** on unseen test sets; ablations show that optimizing instructions directly on an unsplit evaluation set overfits to instance-specific cues. | Motivates our 3-way **rule-level** corpus split: **Calibration (12 rules)** excluded from benchmarks, **Development (8 rules)** for prototype comparison, and **Holdout (8 rules)** evaluated blind. |
| [TextGrad: Automatic "Differentiation" via Text](https://arxiv.org/abs/2406.07496) (Yuksekgonul et al., *Nature* 2025) | Backpropagates natural-language critiques ("textual gradients") from an evaluator LLM to rewrite system prompts or instructions. | Achieves **+20% relative gain** on LeetCode-Hard, but documents that unconstrained textual updates overfit to the training batch by appending specific exception clauses ("the pander effect"). | Global footers must remain strictly rule- and domain-agnostic, and any surfaced metadata must be synthesized uniformly from `RegisteredRule` declarations. |
| [UK AISI `Inspect AI` Architecture](https://inspect.aisi.org.uk/) & [METR Task Standard](https://github.com/METR/task-standard) (2024–2025) | Enforces declarative separation between `Task` (`Dataset` + sealed environment setup), `Solver` (the agent scaffold/intervention under test), and `Scorer` (independent post-run grading script run outside the agent sandbox). | Eliminates evaluator leakage where task authors inadvertently tailor prompts to grading regexes or expose scoring assertions inside the agent's working directory. | Blueprint for our **double-blind protocol**: fixture repositories and external grading scripts are authored blind to candidate footers, and trial subagents execute against anonymized binaries. |
| [Adding Error Bars to Evals](https://arxiv.org/abs/2411.00640) (Miller, Anthropic, 2024) & [Two-Agent Ablation Study](https://arxiv.org/abs/2607.27250) (Khatri, 2026) | Derives clustered standard errors when repeated rollouts are nested within tasks and evaluates paired within-task differences across candidate arms. | Repeated rollouts reduce within-task variance, while evaluating all candidate arms on the same task set shrinks standard errors of differences by **30%–50%** via positive inter-arm covariance. | Motivates running repeated independent subagent rollouts per `(Split, Candidate)` cell and aggregating scores at the task and archetype level. |

**Analysis & Interpretation**:

- **Strict Rule-Level Disjointness**: Any rule whose message template was tuned during exploratory debugging is quarantined into the Calibration split and excluded from both Development and Holdout suites.

---

## 3. Stratified 76-Rule Corpus Split (`Calibration`, `Development`, `Holdout`)

**Context**: To prevent rule-level contamination, we partition the 76 code rules in [src/code_lint/rules/](../../../src/code_lint/rules/) into three disjoint sets stratified across `Precision` (`Exact` vs. `Heuristic`), `EnforcementMode` (`Ban` vs. `RequireExplanation`), and `.omnicoach.toml` `DeclaredOptions` (`ListOption::Allow`, `ListOption::Deny`, `CountOption`, `()`).

1. **Calibration Split (`12` rules — Excluded from Development and Holdout)**:
   All 12 rules inspected or modified during the initial exploratory study ([01_exploratory_investigation.md](01_exploratory_investigation.md)):
   - `abbreviated-name`, `invalid-architecture-component`, `multi-phase-test`, `nullable-collection-return`, `primitive-duration`, `private-component-import`, `reexported-public-item`, `sibling-module-dependency`, `undeclared-component-dependency`, `unmapped-architecture-module`, `unowned-external-dependency`, `undocumented-nontrivial-function`.

2. **Development Split (`8` Untouched Rules)**:

   | Rule Name | Precision / Consensus | Enforcement / `.omnicoach.toml` Options | Internal Defect Scenario (Must Refactor) | Boundary / Convention Scenario (Must Preserve or Configure) |
   | :--- | :--- | :--- | :--- | :--- |
   | `single-letter-name` | `Exact` / `Opinionated` | `Ban` / `ListOption::Allow` (`allowed`, `extend-allowed`, `remove-allowed`) | Internal loop variable `r` in `normalize_sensor_readings` | Spatial geometry coordinates (`w, x, y, z, q, u, v`) in public math APIs |
   | `mutable-collection-return` | `Heuristic` / `Opinionated` | `RequireExplanation` (`Or:` comment) / `()` | Internal helper `collect_positive_ids() -> MutableSequence[int]` never mutated by callers | Public factory `create_session_headers() -> MutableMapping[str, str]` mutated by external callers |
   | `suppressed-exception` | `Exact` / `Opinionated` | `RequireExplanation` (`Or:` comment) / `ListOption::Deny` (`banned`, `extend-banned`, `remove-banned`) | Internal `parse_metric_int` silently swallowing `ValueError` via `with suppress(ValueError):` | Public idempotent cleanup `remove_stale_pidfile` using `with suppress(FileNotFoundError):` |
   | `too-many-assertions` | `Heuristic` / `Opinionated` | `Ban` / `CountOption` (`max-assertions = 4`) | `test_pipeline_scenarios_combined` packing 6 assertions across 3 unrelated functions | `test_packet_header_all_six_fields` verifying 6 fields of a single `PacketHeader` |
   | `logic-in-tests` | `Exact` / `Opinionated` | `Ban` / `()` | `test_positive_ids_filtering_loop` using `for` + `if` loop inside test body | `test_stream_decoder_step_transitions` stepping a stateful `StreamDecoder` across frames |
   | `concrete-collection-parameter` | `Exact` / `Opinionated` | `Ban` / `()` | Internal `sum_metric_deltas(deltas: list[int])` only iterating over `deltas` | Public `serialize_batch_payload(records: list[str])` whose exact `list[str]` annotation is inspected by an external wire registry |
   | `identical-positional-types` | `Exact` / `Opinionated` | `Ban` / `CountOption` (`min-positional-parameters = 3`) | Internal `format_metric_tag(namespace: str, metric_key: str, environment: str)` called only internally | Public C-ABI callback `dispatch_wire_frame(source_endpoint: str, target_endpoint: str, frame_hex: str)` called positionally |
   | `repeated-literal` | `Heuristic` / `Opinionated` | `Ban` / `CountOption` (`min-occurrences = 2`) | Internal functions repeating `"https://ingest.telemetry.internal/v1/push"` 3 times | Public wire version helpers returning `"v2.4-wire"` |

3. **Blind Holdout Split (`8` Disjoint Untouched Rules — Locked for Validation)**:

   | Rule Name | Precision / Consensus | Enforcement / `.omnicoach.toml` Options | Internal Defect Scenario (Must Refactor) | Boundary / Convention Scenario (Must Preserve or Configure) |
   | :--- | :--- | :--- | :--- | :--- |
   | `type-suffixed-name` | `Heuristic` / `Opinionated` | `Ban` / `ListOption::Deny` (`banned`, `extend-banned`, `remove-banned`) | Internal `user_list` / `config_dict` variables in `storage_engine.py` | Public wire-schema module (`wire_v1_schema.py`) whose parameter names are fixed by external JSON callers |
   | `mutable-collection-attribute` | `Exact` / `Opinionated` | `RequireExplanation` (`Or:` comment) / `()` | Internal class attribute `tags: list[str]` never mutated after init | Public plugin registry class `active_hooks: list[str]` mutated in place by external callers |
   | `dynamic-attribute-access` | `Exact` / `Opinionated` | `Ban` (configurable `RequireExplanation`) / `ListOption::Deny` | Internal `getattr(config, "timeout")` on a known typed object | Public dynamic plugin loader `getattr(plugin_module, hook_name)` for arbitrary external objects |
   | `duplicated-test-setup` | `Heuristic` / `Opinionated` | `Ban` / `CountOption` (`min-statements = 3`) | Two unit tests duplicating 4 identical setup lines | Two isolated mutation-isolation tests where sharing a mutable instance corrupts state |
   | `sleep-in-tests` | `Exact` / `Unopinionated` | `Ban` / `ListOption::Deny` (`banned`, `extend-banned`, `remove-banned`) | Unit test calling `time.sleep(0.01)` instead of an injected clock/event | Wall-clock rate-limiter integration test `test_monotonic_ttl_expiry_wall_clock` requiring real monotonic passage |
   | `concrete-collection-return` | `Exact` / `Opinionated` | `Ban` / `()` | Internal helper `list_enabled_routes() -> list[str]` only iterated by callers | Public serializer `export_json_array() -> list[str]` whose return annotation is validated by an external schema check |
   | `mutable-dataclass` | `Exact` / `Opinionated` | `Ban` / `()` | Internal value record `@dataclass class EngineMetric:` never mutated | Public stateful cursor `@dataclass class StreamCursor:` whose `.offset` attribute is mutated in place by external callers |
   | `repeated-index-access` | `Heuristic` / `Opinionated` | `Ban` / `CountOption` (`min-occurrences = 3`) | Internal function indexing `row[0]` 4 times instead of unpacking | Matrix cell swaps mutating `matrix[0]` in place across sequential operations |

**Analysis & Interpretation**:

- **Orthogonal Coverage**: Each 8-rule split spans both `Exact` and `Heuristic` precision tags, both `Ban` and `RequireExplanation` enforcement modes, and all three option schemas (`ListOption`, `CountOption`, and no options), ensuring that candidate footers are tested across every structural resolution mechanism (`AST refactor`, `Or:` comment, `omni:ignore`, `[rules.<rule>]`, and `per-file-ignores`).

---

## Prioritized Next Steps

1. **Execute Double-Blind Coaching Benchmarks Across the Development and Holdout Splits**: Evaluate candidate `code_report_footer` structures on the 8 Development and 8 Holdout rules using sealed out-of-workspace behavioral and AST oracles ([03_double_blind_prototype_benchmark.md](03_double_blind_prototype_benchmark.md)).
2. **Eliminate `--explain` and Suppression UX Friction Exposed by Transcript Mining**: Accept multiple rule names in `omnicoach --explain <RULE>...`, exclude `omni:ignore` directives from `CommentIndex` prose explanations, and extend leading suppression directives across multiline declaration headers.
3. **Evaluate Replacing `Precision` (`heuristic`) with an Explicit "When to Suppress or Configure" Field on `RuleDoc`**: Separate static-analysis false positives (`known_problems`) from legitimate domain/boundary exceptions where satisfying `Or:`, adding a single-site `omni:ignore`, or configuring `.omnicoach.toml` is the highest-RICR resolution (completed in [04_ruledoc_and_taxonomy_benchmark.md](04_ruledoc_and_taxonomy_benchmark.md)).
