# Phase 3–4: Double-Blind Prototype Implementation, Pilot & Unpaired Multi-Archetype Benchmark

## TL;DR

- **Unpaired Multi-Archetype Benchmark Winner (`Scope-Calibrated 3-Branch + Schema Hint`, promoted to [src/rule_catalog.rs](../../../src/rule_catalog.rs))**:
  1. **Raises resolution accuracy from `97%` (`Rule-Agnostic Prose Baseline`) to `100%` across 16 blind trials (`256` evaluated items)**:
     - Under the `Rule-Agnostic Prose Baseline` and the `Uncalibrated 3-Branch Footer`, blind subagents committed **Destructive Compliance** on the stateful `StreamDecoder` transition test—unrolling a 4-step stateful transition loop into 4 flat assertions just to fall under `max-assertions = 4`. Under the `Rule-Agnostic Prose Baseline`, a Holdout run also committed **Suppression Clutter** on `src/wire_v1_schema.py`, scattering 8 duplicate `# omni:ignore [type-suffixed-name]` comments across the file instead of configuring `per-file-ignores` in `.omnicoach.toml`.
     - Explicitly calibrating the three resolution branches by **scope**—**Internal defect (default)** vs. **Single-site boundary exception** (forbidding repo-wide `.omnicoach.toml` relaxation and threshold dodges) vs. **Multi-site module or domain convention (5+ sites)** (`per-file-ignores` or `[rules.<rule>]` in `.omnicoach.toml`)—achieved **`100%` RICR alignment** across both Development and Holdout splits.
  2. **Halves reasoning token cost (`-50%` overall, `-68%` on Development) and saves 2–3 discovery turns on Holdout**:
     - `Scope-Calibrated 3-Branch + Schema Hint` reduced mean output tokens from **`23,352` to `11,645` (`-50%`)** and mean agent steps from `35` to `32`. It also outperformed `Scope-Calibrated 3-Branch Footer` (without the inline schema hint) on Holdout (`12,946` vs. `16,047` tokens, `31` vs. `35` steps) because `omnicoach --explain <rule>` renders `[rules.<rule>]` options but omits top-level `per-file-ignores` syntax—forcing agents without the inline hint to spend 2–3 extra turns running `omnicoach --help` and `omnicoach code --help`.
  3. **Validates the CLI UX Fixes and Surfaces Three Follow-Ups**:
     - After landing multi-rule `omnicoach --explain <RULE>...`, excluding `omni:`/`omnicoach:` directives from `CommentIndex` prose explanations, and extending suppression coverage across multiline declaration headers, `--explain` CLI errors dropped from **`40%` to `0%`** and `[unused-suppression]` retries dropped from **`70%` to `6%`**.
     - Live trajectories exposed two remaining linter UX follow-ups—a **two-hop parameter widening cascade** (`[concrete-collection-parameter]` suggesting `Sequence` $\to$ `[specific-collection-parameter]` suggesting `Iterable` in `88%` of Development trials) and **function-header `omni:ignore` vs. body-span test rules** (`[logic-in-tests]`)—plus a structural documentation improvement: **replacing the `Precision` (`heuristic`) tag with an explicit `RuleDoc` field stating when it is idiomatic to suppress or configure a rule**.

---

## 1. Competing Coaching Prototypes (Paired Pilot)

**Context**: To test which diagnostic footer structure and metadata level best guides coding agents without overfitting to specific rules, we first evaluated five variants of `code_report_footer` in [src/rule_catalog.rs](../../../src/rule_catalog.rs) across the 8 Development and 8 Holdout rules from [02_references_and_corpus_split.md](02_references_and_corpus_split.md).

| Prototype Cohort | Decision Framing & Metadata Surfaced | Primary Hypothesis Tested |
| :--- | :--- | :--- |
| **Heuristic-Only Baseline** | Unconditional fix imperative (*"Fix each finding by applying its suggestion"*) + suppression note gated strictly on `Precision::Heuristic`. | Tests whether restricting suppression guidance to "heuristic false positives" causes reasoning stalls or contract breakage on `Precision::Exact` rules. |
| **Rule-Agnostic Prose Footer** | Two-paragraph prose note (`FIX_NOTE` + `SUPPRESSION_NOTE` gated on `Precision::Heuristic \|\| Consensus::Opinionated`) naming `.omnicoach.toml`, `Or:`, `omni:ignore`, and issue reporting. | Tests whether expanding suppression/config guidance to `Consensus::Opinionated` rules eliminates `Precision::Exact` boundary stalls and whether prose pointers suffice for `.omnicoach.toml` adoption. |
| **Structured 3-Branch Footer** | Explicit 3-branch Toulmin decision list separating **Internal defect (default)**, **Project or domain convention** (`.omnicoach.toml`), and **External caller, wire, or runtime boundary** (`Or:` / `omni:ignore`). | Tests whether visually separating the three resolution paths increases `.omnicoach.toml` inspection or reduces reasoning tokens compared to prose paragraphs. |
| **Prose Footer + All Config Keys** | `Rule-Agnostic Prose Footer` + auto-synthesized `Configurable in .omnicoach.toml:` line listing every `CountOption` and `ListOption` key for reported rules. | Tests whether surfacing numeric thresholds (`max-assertions`, `min-occurrences`) tempts agents to inflate thresholds in `.omnicoach.toml` instead of refactoring internal defects. |
| **Structured 3-Branch + Allowlist Keys** | `Structured 3-Branch Footer` + inline synthesis of symbol-scoped `ListKind::Allow` (`extend-allowed`) keys inside the **Project or domain convention** branch. | Tests whether selectively surfacing symbol-scoped allowlist keys (`extend-allowed`) unlocks `.omnicoach.toml` adoption while keeping numeric thresholds unadvertised in stdout. |

---

## 2. Double-Blind Pilot Protocol & Results

### 2.1 Isolation Protocol

To prevent evaluator leakage and rule-level overfitting, every trial enforced four boundaries:

1. **Zero Contamination Across Splits**: None of the 12 calibration rules from [02_references_and_corpus_split.md](02_references_and_corpus_split.md) appeared in the Development or Holdout suites.
2. **Blinded Anonymized Binaries**: Each subagent received an isolated workspace copy and an anonymized `omnicoach` binary path, with no access to `omnicoach` source code, `AGENTS.md`, or sibling trials.
3. **Zero Prompt Hints on Holdout**: Holdout modules (`internal_engine.py`, `external_bridge.py`, `test_engine_suite.py`) contained zero module docstrings or comments hinting at which symbols were internal vs. external, forcing agents to infer contracts purely from codebase usage and `omnicoach` output.
4. **Out-of-Workspace Behavioral + AST Grading**: Sealed grading scripts (`verify_oracle.py`) kept outside the workspace verified both AST refactoring on internal defects and exact signature, type-hint, and mutation preservation on external boundaries.

### 2.2 Development & Holdout Pilot Results

**Context**: Evaluates two independent blind subagent runs per prototype on the Paired Development Suite (`16` findings per run across 8 rules) and the unhinted Paired Holdout Suite (`16` findings per run across 8 disjoint rules).

| Prototype Cohort | Dev Defect Refactor | Dev Boundary Preservation | Dev `.omnicoach.toml` Created | Holdout Defect Refactor | Holdout Boundary Preservation | Holdout `--explain` Calls (Median) | Holdout Output Tokens (Median) | Holdout Token Delta | Holdout Wall Time (Median) |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Heuristic-Only Baseline** | `100%` | `100%` | `0%` | `100%` | `100%` | `8.5` | `27,487` | — | `9m 47s` |
| **Rule-Agnostic Prose Footer** | `100%` | `100%` | `0%` | `100%` | `100%` | `5.0` | `16,083` | **`-41%`** | `5m 52s` |
| **Structured 3-Branch Footer** | `100%` | `100%` | `0%` | `100%` | `100%` | `3.0` | **`14,115`** | **`-49%`** | **`5m 20s`** |
| **Prose Footer + All Config Keys** | `100%` | `100%` | `0%` | `100%` | `100%` | `2.0` | `16,071` | **`-42%`** | `6m 01s` |
| **Structured 3-Branch + Allowlist Keys** | `100%` | `100%` | **`100%`** | `100%` | `100%` | `3.0` | `18,339` | **`-33%`** | `5m 26s` |

**Analysis & Interpretation**:

- **Root Cause of the 27,487-Token Reasoning Stall Under `Heuristic-Only Baseline`**: The baseline footer stated: *"Some findings above come from heuristic rules. If a finding is a false positive and applying its suggestion would make the code worse, suppress it with an inline comment (`# omni:ignore [rule-name] -- <reason>`)."* On the unhinted Holdout suite, subagents ran `--explain` on all 8 rules (`8.5` median calls) and saw `Precision: exact` on `concrete-collection-return`, `mutable-dataclass`, `dynamic-attribute-access`, and `sleep-in-tests`. Because the baseline authorized `omni:ignore` only for "heuristic false positives," subagents burned over `20,000` reasoning tokens trying to invent wrapper workarounds before conceding that `omni:ignore` was the only way to preserve `external_bridge.py`'s public contract.
- **Why Framing Around Opinionated Defaults & Boundaries Cut Holdout Tokens by 33%–49%**: Explicitly framing `omni:ignore` and `Or:` around *opinionated defaults and external caller, wire, or runtime boundaries* removed the contradiction between `Precision: exact` and legitimate boundary suppression.

---

## 3. CLI UX Friction Exposed by Pilot Trajectories & Fixes Landed

Inspecting all 20 pilot transcripts revealed two concrete CLI usability traps that accounted for **100% of extra `omnicoach code` retry cycles** (`70%` of trials required 3 runs instead of 2) and **100% of failed `--explain` invocations** (`40%` of Holdout trials):

1. **Multi-Rule `--explain` Rejection (`exit code 2`)**:
   - *Symptom*: When an `omnicoach code` report flagged 8 distinct rules at once, subagents naturally batched their lookup into `omnicoach --explain rule-a rule-b ...`, which failed with `error: unrecognized subcommand` because `DiscoveryArgs::explain` in [src/rule_catalog.rs](../../../src/rule_catalog.rs) accepted only a single `Option<String>`.
   - *Fix Landed*: Changed `DiscoveryArgs::explain` to `Vec<String>` (`num_args = 1..`), rendering multiple rules joined by `\n---\n`.
2. **`[unused-suppression]` Traps on `require-explanation` (`Or:`) Rules and Multiline Declaration Headers**:
   - *Symptom A (`omni:ignore` counted as an `Or:` prose explanation)*: When an agent wrote `# omni:ignore [mutable-collection-attribute] -- public plugin registry mutated in place` on a `require-explanation` rule, `CommentIndex::has_explanation_for_span` treated the $\ge 3$-word directive as a valid prose explanation. The rule then emitted no diagnostic, causing the post-pass suppression tracker in [src/code_lint/suppression.rs](../../../src/code_lint/suppression.rs) to report `[unused-suppression]`.
   - *Symptom B (Multiline signature headers)*: Placing `# omni:ignore [type-suffixed-name] -- ...` above a multi-line `def` / `fn` header attached the directive to the `def` start line, missing parameter or return-type diagnostics on wrapped lines of the same signature.
   - *Fixes Landed*: Excluded `omni:` / `omnicoach:` directives from prose explanation matching in [src/code_lint/semantic/comments.rs](../../../src/code_lint/semantic/comments.rs) (while still counting reasons after third-party directives like `# noqa: F401 -- reason`), and extended leading standalone `omni:ignore` directives across the full declaration header (`statement_header_end_lines` in [src/code_lint/ast/statement_spans.rs](../../../src/code_lint/ast/statement_spans.rs)).

---

## 4. Methodological Lessons: Paired Ceiling Effect & Metric Redesign

1. **Why 100% Binary Pass Rates Happened in the Pilot (Contrastive Pairing)**:
   Placing 8 internal defects (`internal_engine.py`) and 8 external boundaries (`external_bridge.py`) for the *exact same 8 rules* side-by-side inside a 3-file workspace allowed frontier models to spot the 1-to-1 structural symmetry between the two files, even with all module comments stripped.
2. **Why Raw `omni:ignore` Reduction Is a Flawed Metric (Config Over-Broadening vs. Surgical Suppression)**:
   Minimizing inline `omni:ignore` count rewards **Config Over-Broadening**: relaxing a rule repo-wide in `.omnicoach.toml` to silence a **single localized boundary site** silently disables the rule across every other module in the repository. For a single localized boundary exception, an inline `# omni:ignore [<rule>] -- <reason>` (or `Or:` comment) is the highest-RICR, most surgical resolution; `.omnicoach.toml` is only the right RICR choice when a convention spans **multiple functions or modules across a domain package** (where repeating 5+ identical inline `omni:ignore` comments creates clutter).

---

## 5. Unpaired Multi-Archetype Double-Blind Benchmark

### 5.1 Unpaired Task Archetypes & Resolution-Mechanism RICR Scoring

**Context**: Following the CLI UX fixes in §3, we redesigned the benchmark so that each rule in the **Development Suite** (`8` rules, `16` scored items across `32` initial findings) and the **Holdout Suite** (`8` disjoint rules, `16` scored items across `24` initial findings, with zero module-role comments) appears in an **unpaired** role matched to one of four ground-truth archetypes:

| Task Archetype | Ground-Truth Scenario | Required RICR Resolution (`score = 1.0`) | Penalized Anti-Patterns (`score = 0.0`) |
| :--- | :--- | :--- | :--- |
| **Unpaired Internal Defect** (`7` Dev, `7` Holdout) | Genuine internal code or test defect in a module or test suite (`telemetry_pipeline.py`, `storage_engine.py`, `test_*.py`). | Idiomatic AST refactor preserving unit/caller behavior without any `omni:ignore` or `.omnicoach.toml` relaxation. | **Lazy Suppression** (adding `omni:ignore`), **Config Over-Broadening** (disabling/relaxing in `.omnicoach.toml`), or **Threshold Dodging** (deleting assertions). |
| **Single-Site Boundary Exception** (`6` Dev, `6` Holdout) | One public SDK/wire/callback entrypoint or stateful transition loop test where refactoring breaks external callers or test semantics. | Surgical site-scoped escape (`Or:` comment, `@dataclass(frozen=False)`, or single-site `omni:ignore [<rule>] -- <reason>`). | **Destructive Compliance** (breaking signature, return type, mutability, or unrolling a stateful transition loop to dodge `max-assertions`) or **Config Over-Broadening** (weakening `.omnicoach.toml` repo-wide for 1 site). |
| **Multi-Site Domain Convention** (`1` Dev, `1` Holdout) | A dedicated domain module (`spatial_geometry.py` with 7 `x, y, z, w, q, u, v` functions; `wire_v1_schema.py` with 8 wire-protocol `*_list`/`*_dict`/`*_str`/`*_int` functions). | Module- or symbol-scoped `.omnicoach.toml` configuration (`extend-allowed = [...]` or `per-file-ignores = { "src/wire_v1_schema.py" = ["type-suffixed-name"] }`) while keeping internal defects in other modules flagged. | **Suppression Clutter** (scattering 7–8 duplicate `# omni:ignore` directives across the domain module) or **Destructive Compliance** (renaming public wire-protocol parameters). |
| **Intra-Module Mixed Polarity** (`2` Dev, `2` Holdout) | A single module where both a locked public entrypoint and an internal private helper trigger the **exact same rule** (`repeated-literal` on Development, `repeated-index-access` on Holdout). | Preserve/suppress the locked public entrypoint **and** refactor the private internal helper within the same file. | **Suppression Contagion** (copying `omni:ignore` onto the private helper) or **Destructive Compliance** (breaking the public entrypoint). |

---

### 5.2 Competing Universal Footer Candidates & Aggregate Results

**Context**: Evaluates four universal footer candidates across two independent blind subagent runs on the Unpaired Development Suite and two independent blind subagent runs on the unhinted Unpaired Holdout Suite (`16` trials $\times$ `16` items = `256` graded evaluations):

1. **`Rule-Agnostic Prose Baseline`**: Two-paragraph prose `FIX_NOTE` + `SUPPRESSION_NOTE` + `--explain <rule>` pointer.
2. **`Uncalibrated 3-Branch Footer`**: 3-branch bulleted footer (*Internal defect*, *Project or domain convention*, *External caller, wire, or runtime boundary*) without explicit single-site vs. multi-site scope counts.
3. **`Scope-Calibrated 3-Branch Footer`**: 3-branch bulleted footer explicitly contrasting **Single-site boundary exception** (*"suppress that site only ... do not weaken `.omnicoach.toml` repo-wide for a single site"*) against **Multi-site module or domain convention (5+ sites across a domain module)** (*"Configure `[rules.<rule>]` or `per-file-ignores` in `.omnicoach.toml`"*), plus `--explain <rule>...`.
4. **`Scope-Calibrated 3-Branch + Schema Hint`** (promoted to [src/rule_catalog.rs](../../../src/rule_catalog.rs)): Adds inline `per-file-ignores = { "path.py" = ["<rule>"] }` and `[rules.<rule>]` syntax inside the multi-site branch.

| Candidate Cohort | Overall RICR (`256` items) | Dev RICR | Holdout RICR | Unpaired Internal Defect | Single-Site Boundary | Multi-Site Convention | Intra-Module Mixed | Mean Output Tokens (All / Dev / Holdout) | Token Delta vs. Baseline | Mean Steps |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Rule-Agnostic Prose Baseline** | `96.9%` | `96.9%` | `96.9%` | `100%` | `95.8%` | `75.0%` | `100%` | `23,352` (`32,672` / `14,033`) | — | `34.8` |
| **Uncalibrated 3-Branch Footer** | `98.4%` | `96.9%` | `100%` | `100%` | `95.8%` | `100%` | `100%` | `16,530` (`17,790` / `15,270`) | `-29.2%` | `32.5` |
| **Scope-Calibrated 3-Branch Footer** | **`100%`** | **`100%`** | **`100%`** | **`100%`** | **`100%`** | **`100%`** | **`100%`** | `13,329` (`10,611` / `16,047`) | `-42.9%` | `34.8` |
| **Scope-Calibrated 3-Branch + Schema Hint** | **`100%`** | **`100%`** | **`100%`** | **`100%`** | **`100%`** | **`100%`** | **`100%`** | **`11,645` (`10,344` / `12,946`)** | **`-50.1%`** | **`31.8`** |

**Analysis & Interpretation**:

1. **Eliminating Stateful-Loop Unrolling on Single-Site Boundaries (`95.8%` $\to$ `100%`)**:
   - In `test_stream_decoder_step_transitions`, a 4-step `for frame, expected_count in steps:` loop advances a stateful `StreamDecoder` across sequential frames. Under both `Rule-Agnostic Prose Baseline` and `Uncalibrated 3-Branch Footer`, a blind subagent noticed that unrolling the 4-iteration loop into 4 flat `assert decoder.feed(frame) == expected` lines eliminated the `for` statement while landing right at `max-assertions = 4` (`4 <= 4`), destroying the table-driven transition loop to dodge `[logic-in-tests]` and `[too-many-assertions]`.
   - Explicitly naming *"Single-site boundary exception (locked public/wire signature or stateful test): Satisfy `Or:` if shown, or suppress that site only with `omni:ignore [<rule>] -- <reason>`"* in the scope-calibrated footers stopped loop-unrolling threshold gaming across all runs (`100%`).
2. **Eliminating Multi-Site Suppression Clutter (`75.0%` $\to$ `100%`)**:
   - On Holdout, `src/wire_v1_schema.py` contains 8 wire-protocol functions (`decode_packet_list`, `encode_header_dict`, etc.) whose parameter names are fixed by external JSON callers. Under `Rule-Agnostic Prose Baseline`, the agent scattered 8 repetitive `# omni:ignore [type-suffixed-name]` comments across `wire_v1_schema.py` instead of configuring `.omnicoach.toml`. Every structured 3-branch candidate configured `.omnicoach.toml` (`100%`).
3. **Why Inline Schema Hints Cut Holdout Tokens by an Additional 19% Over `Scope-Calibrated 3-Branch Footer` (`12,946` vs. `16,047` tokens)**:
   - When `Scope-Calibrated 3-Branch Footer` agents on Holdout ran `omnicoach --explain type-suffixed-name`, the `## Configuration` section showed `[rules.type-suffixed-name]` (`enforcement-mode`), which has no allowlist option, and omitted the top-level `per-file-ignores` table syntax. Those agents spent 2–3 extra turns running `omnicoach --help` and `omnicoach code --help` to discover how `per-file-ignores` is formatted in `.omnicoach.toml`. Including `per-file-ignores = { "path.py" = ["<rule>"] }` directly in the multi-site branch eliminated those extra discovery turns.

---

### 5.3 Representative Trace Comparison (Baseline Failure vs. Scope-Calibrated Winner)

**Context**: Compares the trajectory of the `Rule-Agnostic Prose Baseline` against `Scope-Calibrated 3-Branch + Schema Hint` on the two tasks that separated the cohorts (`test_stream_decoder_step_transitions` on Development and `src/wire_v1_schema.py` on Holdout).

| Task & Split | Cohort | Resolution Chosen by Blind Agent | Oracle Verdict | Output Tokens | Agent Steps |
| :--- | :--- | :--- | :--- | ---: | ---: |
| **Stateful `StreamDecoder` Transition Test** (`Development`, `Single-Site Boundary`) | `Rule-Agnostic Prose Baseline` | Unrolled 4-step `for frame, expected in steps:` transition loop into 4 flat `assert` lines to fall under `max-assertions = 4` (`Destructive Compliance`). | **FAIL (`0.0`)** | `33,838` | `38` |
| **Stateful `StreamDecoder` Transition Test** (`Development`, `Single-Site Boundary`) | `Scope-Calibrated 3-Branch + Schema Hint` | Preserved the stateful `for` loop and added surgical `# omni:ignore [logic-in-tests]` with explanation (`Single-Site Boundary Exception`). | **PASS (`1.0`)** | `9,581` | `33` |
| **Wire Protocol Schema Module (`8` sites)** (`Holdout`, `Multi-Site Convention`) | `Rule-Agnostic Prose Baseline` | Added 8 duplicate `# omni:ignore [type-suffixed-name]` directives across `src/wire_v1_schema.py` (`Suppression Clutter`). | **FAIL (`0.0`)** | `14,508` | `29` |
| **Wire Protocol Schema Module (`8` sites)** (`Holdout`, `Multi-Site Convention`) | `Scope-Calibrated 3-Branch + Schema Hint` | Added `per-file-ignores = { "src/wire_v1_schema.py" = ["type-suffixed-name"] }` to `.omnicoach.toml` in 1 step while refactoring `storage_engine.py`. | **PASS (`1.0`)** | `12,081` | `29` |

**Analysis & Interpretation**:

- **Scope Calibration + Concrete Syntax**: Naming the scope threshold (`1 site` vs. `5+ sites across a domain module`) tells the agent *which* mechanism is idiomatic, while the inline TOML snippet (`per-file-ignores = { "path.py" = ["<rule>"] }`) tells the agent *how* to write it without extra CLI discovery hops.

---

## Prioritized Next Steps

1. **Eliminate the Two-Hop Parameter Widening Cascade (`[concrete-collection-parameter]` $\to$ `[specific-collection-parameter]`)**: In `88%` of Development trials, when `[concrete-collection-parameter]` flagged `def summarize_batch_payloads(payloads: list[str])` with `Suggestion: Replace list in payloads with collections.abc.Sequence...`, the agent changed `list[str]` to `Sequence[str]`, which immediately triggered `[specific-collection-parameter]` (`Replace Sequence with Iterable`) on the next `omnicoach code` run. Have `concrete-collection-parameter` recommend the narrowest read-only abstraction required by the function body (or name `Iterable` vs. `Sequence` by capability in its suggestion).
2. **Align Function-Header `omni:ignore` Scope with Body-Span Test Rules (`[logic-in-tests]`, `[sleep-in-tests]`)**: Placing `# omni:ignore [logic-in-tests]` above `def test_...` currently triggers `[unused-suppression]` because `logic-in-tests` anchors its span on the inner `for`/`if` statement inside the test body rather than on the `def` header (unlike `too-many-assertions`, which anchors on `def`). Evaluate allowing test-scoped rules (`Topic::TESTING`) to match a suppression directive on the enclosing test function header.
3. **Evaluate Replacing the `Precision` (`exact` / `heuristic`) Tag with an Explicit "When to Suppress or Configure" Field on `RuleDoc`**: Both benchmark phases showed that `Precision: exact` vs. `heuristic` conflates parser/single-file false positives (already covered by `RuleDoc.known_problems`) with legitimate domain or boundary exceptions (where even a syntactically `exact` rule should be satisfied via `Or:`, suppressed at a single site with `omni:ignore`, or configured in `.omnicoach.toml`). Adding a dedicated `RuleDoc` field that explicitly lists non-exhaustive boundary and domain cases where suppression or configuration is highest-RICR will give `omnicoach --explain <rule>` targeted, per-rule boundary guidance (completed in [04_ruledoc_and_taxonomy_benchmark.md](04_ruledoc_and_taxonomy_benchmark.md)).
4. **Surface Top-Level `per-file-ignores` in `omnicoach --explain <rule>`**: Include a one-line `per-file-ignores` example inside `## Configuration` when rendering `omnicoach --explain <rule>` so pull-discovery users see both per-rule options and path-scoped ignores.
