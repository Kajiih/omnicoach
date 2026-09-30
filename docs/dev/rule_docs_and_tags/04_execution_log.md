# Phase 4: Execution Log — Tag Taxonomy (T1)

This log records the Phase 4 tasks of the T1 exploration cycle, following the plan in [03_design_plan.md §6](03_design_plan.md).

**Status**: Complete. Tasks 1–6 done (E1, E2, checkpoint 3 → D23–D29, five prototypes compared in §6, tag guide in [tag_guide.md](../tag_guide.md), checkpoint 6 → D30–D38). Next: **Phase 5 (Clean up)**.

**Decisions from checkpoint 3**:
- **D23 — Opinion definition**: *opinionated* means reasonable people disagree (DEF2), applied with the DEF4 test: "name one ordinary situation where the flagged code is correct and appropriate". More values than opinionated/unopinionated may be added if a useful one exists (Q23).
- **D24 — Topic tree**: the merged tree of §2.3 is accepted. Old tag names are dropped; no aliases or backward compatibility (A1).
- **D25 — Single-rule topics are legitimate**: a topic's rule count is not an admission criterion. A topic is admitted when it names a distinct subject, passes the all-and-some test under its parent, and has a scope note. (Supersedes the "≥2 rules or a named candidate" rule used in E2.)
- **D26 — Side findings**: S1–S7 were added to [ROADMAP](../../../ROADMAP.md) under "Rule Defects".
- **D27 — Quality facet on trial** (Q23): P0 carries a single-valued quality facet (`reliability` / `maintainability`, DEF1 with reviewer A's boundary). It is kept or dropped after the prototypes.
- **D28 — Labels use the standard term where one exists** (Q24, working labels, final at checkpoint 6):
  - *Detection facet → "Precision"*. It is the established static-analysis term for "how many reports are true positives": CodeQL query metadata `@precision` (`low`…`very-high`, "estimated proportion of results that are true positives"). The alternative "confidence" (Semgrep, Pylint) was already judged vague. Values stay `exact` / `heuristic`.
  - *Opinion facet → "Consensus"*. No linter names this axis: eslint-plugin-unicorn encodes it as the `unopinionated` value of `meta.docs.recommended`, Clippy as the `restriction` group, typescript-eslint as the `stylistic` config. Values stay `opinionated` / `unopinionated` (unicorn precedent).
- **D29 — Five prototypes** (Q25): P0 (decided design) plus P1 flat topics + presets, P2 group-side membership, P3 label expressions, P4 polyhierarchy. All share one CUJ harness and the 26-rule corpus (plan in [03 §5–§6](03_design_plan.md)).


---

## 1. E1: Opinion facet definitions and detection (task 1)

Two independent read-only reviewers classified all 26 rules under four candidate definitions of "opinionated", and as exact or heuristic. Neither saw the other's output or the current `tags()`.

| Reviewer | Conversation |
|---|---|
| A | `7a9a715e-f382-494f-a19d-be901b453001` |
| B | `f1a0102a-1e9e-4484-9079-19fff4bf28f7` |

### 1.1 Counts

In each cell, the number before the slash is from reviewer A and the number after it is from reviewer B.

| Definition | Not opinionated | Opinionated | Share opinionated |
|---|---|---|---|
| DEF1: no nameable failure mode | 16 / 16 | 10 / 10 | 38% |
| DEF2: reasonable people disagree | 5 / 5 | 21 / 21 | 81% |
| DEF3: goes beyond an external baseline | 2 / 1 | 24 / 25 | 92–96% |
| DEF4: restricts a legitimate construct | 5 / 4 | 21 / 22 | 81–85% |

### 1.2 Agreement

- **DEF2: identical sets.** Both reviewers found exactly five rules not opinionated:
  - `no-logging-error-in-except`
  - `no-sleep-in-tests`
  - `unused-suppression`
  - `unknown-suppression-rule`
  - `blanket-suppression`
- **DEF4: agree on 25 of 26 rules.** A's five not-opinionated rules are the DEF2 set. B's four are the same minus `no-logging-error-in-except`, because the rule also flags `exc_info=True`, which is legitimate code (see §3).
- **DEF1: same counts, different sets.** Each reviewer had to add an unstated boundary for "maintenance hazard", and A said openly that "without this line every rule qualifies". Only A and B share that boundary, so the matching counts do not show agreement.
- **DEF3: close to universal by construction.** Omni exists to cover what the default Ruff and Clippy sets don't. Ruff 0.16 also changed its default set two months ago, so which rules count as default could not be verified.
- **Detection: agree on 24 of 26 rules (19 exact, 7 heuristic for each reviewer).**
  - Both found heuristic: `banned-abbreviations`, `no-hungarian-notation`, `prefer-timedelta-over-seconds`, `prefer-tuple-unpacking`, `max-test-assertions`, `no-env-in-functions`.
  - Disagreements (both reviewers marked these low confidence):
    - `no-unstructured-task-creation`: B says heuristic, A says exact.
    - `no-edits-on-described-commits`: A says heuristic, B says exact.
  - Working definition (A's, which resolves both disagreements): **heuristic means the rule uses a proxy that can flag correct code.** A threshold that can only cause missed findings does not make a rule heuristic.

### 1.3 The U1 test: what `ignore = ["heuristic", "opinionated"]` leaves

| Definition | Rules left | Verdict |
|---|---|---|
| DEF1 | 12–13, including `no-mocks-in-tests`, `no-typing-cast`, `enforce-frozen-slots-dataclass` | ❌ Leaves rules that are highly contested |
| DEF2 | the 5 above | ✅ A set nearly everyone accepts |
| DEF3 | 1–2 | ❌ Nearly empty, and it even drops `no-sleep-in-tests` |
| DEF4 | 4–5 | ✅ Same as DEF2 on this corpus |

**On `enforce-frozen-slots-dataclass`** (the user doubted it is opinionated):
- Both reviewers rated it opinionated under DEF2 and DEF4, with high confidence, because mutable dataclasses are ordinary Python.
- It does have a real failure mode (accidental mutation, and silent typo attributes without slots), which is why it counts as not opinionated under DEF1.
- So the user's intuition matches DEF1. DEF1 fails U1, because "has a failure mode" is not the same as "nearly everyone accepts it".

### 1.4 The reviewers' recommendations

| | Reviewer A | Reviewer B |
|---|---|---|
| Definition | DEF4, as a test: *"Name one ordinary situation where the flagged code is correct and appropriate. If you can, it is opinionated."* | DEF2 if the facet is kept, with worked examples |
| Keep the facet? | Keep. It costs about one line per rule and is checked at compile time. At 81% it is really a "core set" marker, which a future preset could replace. | **Drop.** At 81% `ignore opinionated` switches Omni nearly off, and the useful artifact is the five-rule complement, which is naturally a curated preset. |
| DEF3 | As **References** in the rule docs (T2), not as a classifier | Same |
| Detection | Keep exact/heuristic | Keep exact/heuristic |

### 1.5 Synthesis and recommendation

- **The meaning is settled:** DEF2 is the property we want, and DEF4 is the check we can apply consistently. The two coincide on this corpus.
- **Dropping the facet conflicts with D22.** Today's `Opinionated` tag (8 rules, applied inconsistently) is a selectable distinction, so removing the facet would remove a selector. The drop option therefore needs an explicit exception to D22.
- **My recommendation: keep the facet, defined by DEF2, and apply it with the DEF4 test.**
  - The tag guide lists `no-sleep-in-tests` and `unknown-suppression-rule` as worked examples of not opinionated, and `no-mocks-in-tests` and `prefer-timedelta-over-seconds` as opinionated.
  - Record on the roadmap that a future `recommended` preset may replace the facet, seeded from the not-opinionated set.
  - Keep exact/heuristic under A's working definition (§1.2).

---

## 2. E2: The topic tree (task 2)

Two independent designers built the topic tree under D16 (tree only), D19/D20 (bare, globally unique values), D22 (at least today's granularity), the all-and-some parent test (R6) and the admission rule (at least 2 rules, or 1 rule plus a named roadmap candidate).

| Designer | Conversation |
|---|---|
| A | `81494042-c2b5-4b80-a8c0-291e076829e6` |
| B | `4629f610-3f16-43d1-af19-6427b76f2781` |

### 2.1 Where they agree (adopt as is)

- **`style` is dissolved.** `naming` is a peer, not a child: `prefer-timedelta-over-seconds` is a naming rule motivated by bugs, not taste. "Matter of taste" is the opinion facet's job.
- **`safety` is retired.** It has two meanings (type safety, and destructive commands), and it describes a quality, not a subject. `no-typing-cast` joins the type-safety group; both designers read its earlier omission as an inconsistency. The destructive-command meaning has no rules yet; a future `destructive-operations` topic would be a root.
- **Qualities are not topics.** Words like readability, safety and style describe what nearly every rule claims (R9 meta-tags), so they stay off the tree.
- **Shared parts of the tree:**
  - `naming` > `abbreviated-names`, `type-encoded-names`
  - `testing` > `test-assertions`, `test-doubles`, `test-timing`
  - `vcs` > `jj` (synonym `jujutsu`)
- **Shared roots:** `literals`, `complexity`, `error-handling` (renamed from Exceptions, so it also covers Rust `Result`), `logging`, `async`, `global-state` (renamed from SideEffects: reading env is ambient input, not a side effect), `suppression-directives`.
- **The `suppress` homonym:** `contextlib.suppress` goes under `error-handling`, and `omni:` directives go under `suppression-directives`. The two are linked as "see also" only. The runner's special handling of suppression rules becomes a typed role (DI4), not a tag.
- **`no-mock-assertions`** carries both `test-assertions` and `test-doubles`.
- **Six single-member topics are kept** because D22 overrides the admission rule: `complexity`, `global-state`, `logging`, `async`, `vcs`, `jj`.

### 2.2 Where they diverge

| Point | Designer A | Designer B | Recommendation |
|---|---|---|---|
| Typing | `typing` > `type-safety`, `duration-types`, `record-types`. Old Typing stays **one** selector. | `static-typing` > `type-checker-bypass`. `durations` and `record-types` are **roots**. Old Typing needs **3** selectors. | **B.** The all-and-some test fails under a typing parent: the frozen/slots rationale is about mutability, and a future rule on time-unit literal arithmetic or monotonic clocks is about durations, not types. `typing` also collides with the Python module name. |
| Positional code | `tuples` (tuple-unpacking) and `function-signatures` (identical-positional), both roots | `positional-meaning` > `positional-indexing`. Identical-positional sits on `positional-meaning` and on `static-typing`. | **B.** Topics name a subject, not a syntax. This groups the roadmap follow-ups (`re.Match` groups, tuple structs, sparse access) and adds one root instead of two. |
| `history-rewriting` under `vcs` | — | Added. B calls it its own weakest topic. | **Drop.** It fails admission and D22 does not require it. |

### 2.3 Merged tree (proposal)

22 topics: 14 roots and 8 children, at most two levels deep.

```text
naming
├── abbreviated-names
└── type-encoded-names
testing
├── test-assertions
├── test-doubles
└── test-timing
static-typing
└── type-checker-bypass
positional-meaning
└── positional-indexing
vcs                      (synonym: version-control)
└── jj                   (synonym: jujutsu)
durations
record-types
literals
complexity
global-state
error-handling
logging
async
suppression-directives
```

### 2.4 Rule assignments (most specific topic only; the parent is implied)

| Rule | Topics |
|---|---|
| banned-abbreviations, single-letter-variable-name | `abbreviated-names` |
| no-hungarian-notation | `type-encoded-names` |
| prefer-timedelta-over-seconds | `type-encoded-names`, `durations` |
| prefer-dedent-for-multiline-strings | `literals` |
| prefer-tuple-unpacking | `positional-indexing` |
| flat-scope-enforced | `complexity` |
| enforce-frozen-slots-dataclass | `record-types` |
| no-typing-cast, no-dynamic-attribute-access | `type-checker-bypass` |
| no-identical-positional-types | `static-typing`, `positional-meaning` |
| no-env-in-functions | `global-state` |
| no-logging-error-in-except | `logging`, `error-handling` |
| no-uncommented-suppress | `error-handling` |
| no-unstructured-task-creation | `async` |
| no-sleep-in-tests, no-zero-sleep-in-tests | `test-timing` |
| max-test-assertions, no-assertion-packing | `test-assertions` |
| no-mocks-in-tests | `test-doubles` |
| no-mock-assertions | `test-assertions`, `test-doubles` |
| the 4 suppression audit rules | `suppression-directives` |
| no-edits-on-described-commits | `jj` |

### 2.5 Old tag → new selectors

| Old tag | New selectors | Result |
|---|---|---|
| Naming | `naming` | Exact, with two finer children |
| Testing | `testing` | Exact, with three finer children |
| Complexity, Logging, Async, Vcs, JJ | same name | Exact |
| Exceptions | `error-handling` | Exact (renamed) |
| SideEffects | `global-state` | Exact (renamed) |
| Suppression | `suppression-directives` | Exact. The runner's use of the tag moves to a typed role (DI4). |
| Typing | `static-typing` + `durations` + `record-types` | Exact, but needs 3 selectors |
| Style | `naming` + `literals` + `positional-indexing` + `complexity` + `record-types` | Exact, but needs 5 selectors |
| Safety | `static-typing` | **Broader by one rule** (`no-typing-cast`). The old set is still reachable by also ignoring `no-typing-cast`, since a rule name beats a tag (D15). |
| Workflow | `vcs` | Same members today |
| Cli | — | Replaced by the derived code/command facet |
| Opinionated, Heuristic | the opinion and detection facets | These move to facets (§1). The old tags were applied inconsistently: 8 rules were tagged opinionated, against 21 under DEF2, and 3 were tagged heuristic, against 7. |

---

## 3. Side findings (out of scope, proposed for ROADMAP)

| # | Finding | Found by | Verified |
|---|---|---|---|
| S1 | `no-logging-error-in-except` flags `exc_info=True`, which keeps the traceback. This contradicts the rule's own rationale. | E1 A, B | Tested behaviour |
| S2 | `prefer-dedent-for-multiline-strings` flags `textwrap.dedent`. This contradicts the rule's own summary and module doc. | E1 A, B | Tested behaviour |
| S3 | Call rules match callees by bare name without resolving imports, so `sqlalchemy.cast`, `ctypes.cast` and httpx `patch` are flagged. | E1 A, B | Code reading |
| S4 | **`blanket-suppression`'s rationale is wrong.** A blanket directive suppresses nothing: `target_rules` is empty ([suppression.rs:287](../../../src/code_lint/suppression.rs#L287)). The rationale says the rule prevents *unintended* suppression. | E1 A | ✅ Checked in code |
| S5 | `unused-suppression` probably reports a directive as unused when its target rule is disabled by config. | E1 A, B | Not tested |
| S6 | `no-unstructured-task-creation` flags tasks that are stored and awaited. | E1 A | Code reading |
| S7 | The Python half of `no-zero-sleep-in-tests` bans `asyncio.sleep(0)`, which the asyncio docs endorse. | E1 A, B | Docs |

---

## 4. Checkpoint 3: decisions requested

- **DI1, the opinion facet:**
  - (a) Keep it, defined by DEF2 and applied with the DEF4 test (recommended), or
  - (b) drop it and make an explicit exception to D22, relying on a future preset.
- **DI2, display labels** (display only, per D19):
  - Detection facet: "Detection" (recommended) or "Match precision". The values are `exact` / `heuristic`.
  - Opinion facet (if kept): "Opinion", "Consensus" or "Stance". The values are `opinionated` / `unopinionated`.
- **The topic tree:** validate §2.3, including:
  - B's options on the three diverging points (§2.2);
  - the 5-selector union for old Style and the 3-selector union for old Typing, which have no single handle (a handle would be a preset, NG5);
  - Safety being broader by one rule;
  - the formal admission exemption for the six legacy single-member topics;
  - `no-zero-sleep-in-tests` staying out of `async` (both designers lean no);
  - `vcs` staying canonical, with `version-control` as a synonym.
- **Side findings:** add S1–S7 to ROADMAP as out-of-scope items.

---

## 5. Open after checkpoint 3

### Q23 — A value or facet beyond opinionated/unopinionated?

The E1 definitions that did not win still measure something:

| Candidate | What it answers | Evidence | Verdict |
|---|---|---|---|
| DEF3 "beyond an external baseline" | Which tool or document backs this rule? | 92–96% say "none", so it is useless as a selector. | Not a facet. It becomes a **References** field in the rule docs (T2). |
| DEF1 "prevents a nameable defect" | Does this rule catch bugs, or keep code maintainable? | 16/10 for both reviewers once the boundary is stated: a mechanism leading to wrong behaviour (product bug, false-green test, silently wrong suppression); readability does not count. It is **orthogonal** to opinion: `no-mocks-in-tests` prevents a false green yet is opinionated. | **Candidate facet.** SOTA: SonarQube "software qualities" (reliability, security, maintainability); Clippy `correctness`/`suspicious` vs `style`. CUJ it serves: "enable only the rules that catch bugs". |
| A graded opinion scale (consensus / common / opinionated) | How contested is it? | Both reviewers' low-confidence calls fall exactly on the middle band. | Not recommended: a middle value is where raters diverge. |

### Q24 — Two-word display labels

Labels are display only (D19), so they can be explicit phrases. The values stay bare selectors.

| Facet | Values | Candidates (first = recommended) |
|---|---|---|
| Detection | `exact` / `heuristic` | "Detection method", "Match precision", "False-positive risk" |
| Opinion | `opinionated` / `unopinionated` | "Consensus level", "Opinion level", "Community consensus" |
| Quality (if Q23 adopted) | `reliability` / `maintainability` | "Protected quality", "Primary impact" |
| Topic | the tree | "Topic" (accepted, D20) |
| Derived: languages | `python` / `rust` | "Languages" |
| Derived: code/command | `code` / `command` | "Analyzed input", "Checks" |
| Derived: target | `tests-only` / `source-only` | "File scope", "Applies to" |

### Q25 — Alternative prototypes from SOTA

P0 is the decided design (D17–D22). Each alternative is built against the **same CUJ tests and all 26 rules**, so the comparison is on evidence, not taste. Losers are archived with the reason (Phase 3 rule).

| # | Prototype | Source (02 refs) | Question it answers |
|---|---|---|---|
| P0 | Typed facet fields + topic tree + precedence B | R1, R2, R6, R8 | Baseline |
| P1 | **Flat topics + curated presets** (`recommended`, `strict`), no hierarchy | R5 typescript-eslint, unicorn; C2 golangci | Does the hierarchy earn its cost over flat tags plus presets? |
| P2 | **Group-side membership**: groups list their rules (and sub-groups), rules declare nothing | R3 rustc lint groups | Rule-side vs group-side declaration: which keeps docs, review and "did you mean" simpler? |
| P3 | **Label-expression selection**: `select = ["testing & !opinionated"]` or Kubernetes-style `topic in (...)` | R7 Kubernetes label selectors | Can intersection selection replace precedence B, and is it clearer for U1–U6? |
| P4 | **Polyhierarchy (DAG)**: a topic may have several parents (e.g. `test-timing` under `testing` and `durations`) | R6 SKOS `broader` | What does D16's "DAG later" really cost now (precedence, cycles, display)? |
| — | Rule-code prefixes (Ruff/Pylint codes) | R2, C3 | Not proposed: its known failure mode (renaming on recategorisation) is already documented. |

---

## 6. Prototype comparison (tasks 4a–4c)

All code is in `scratch/tag_poc/` (git-ignored): a shared `harness` crate and one crate per prototype, each with a `REPORT.md`. Final state: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass for the whole workspace (78 tests plus 7 doctests).

| Prototype | Builder conversation |
|---|---|
| P0 typed facets | `5eba2c3b-c726-4184-82de-a6f312f959ca` |
| P1 flat + presets | `e79f6487-9ad4-4afe-9dd4-3ed7c7a0deff` |
| P2 group-side | `58dfef68-ffdf-414d-819b-62c482d43442` |
| P3 label expressions | `7cc6b930-5747-41df-a167-326eeda1a259` |
| P4 polyhierarchy | `659ded3b-5248-4bab-87d5-b21d78167245` |

### 6.1 The harness, and what the prototypes taught it

- **Contract.** `Prototype` trait with `load`, `is_enabled`, `matched_selectors` and `express` (a prototype whose syntax differs writes its own config for each scenario's *intent*). Three generic checks:
  - 22 CUJ scenarios with outcomes fixed by intent;
  - `select = [S]` for every selector, synonym and rule name (vocabulary);
  - every rule's matched selectors against a hand-written answer key (classification).
- **RED.** A stub that enables everything fails all scenarios but the no-config one.
- **Fixed after the first round** (findings from P1 and P3):
  - The two D18 case-2 scenarios did not discriminate: `select = [child]` alone passed them. They now also select a broad set (`python`, `exact`), so model A and model B give different results.
  - The conflict and facet-label scenarios accepted any error mentioning the label. They now forbid "unknown"/"did you mean" and require the facet's values.
  - The macro needed a path; it now accepts a bare type.
- **Known limit** (P1, P4): scenarios check today's rules only. A config listing a child's current rules by name passes case 2, and a tree passes where a DAG is expected. The prototypes' own tests cover this.

### 6.2 Results

| | P0 typed facets | P1 flat + presets | P2 group-side | P3 expressions | P4 DAG |
|---|---|---|---|---|---|
| CUJ scenarios (of 21 shared + Q1) | 22/22 | 21/21 | 21/21 | 21/21 | 21/21 |
| Own-syntax configs needed | 0 | 3 | 0 | 21 | 0 |
| Production LOC | 919 (475 logic) | 420 | 717 (554 mechanism) | 703 (206 parser) | 597 |
| Lines per rule classification | 6 | ~1 | 0 in the rule; 3–4 edits in `groups.rs` | table (not compared) | as P0 |
| Places to edit: add a rule | 1 | 1 (+ each broad tag, by hand) | 3–4, none in the rule file | — | 1 |
| Places to edit: add a topic | 1 file, 2 spots | 1 + every member rule | 1 file, 2–3 spots | — | 1 file, 2 spots |
| Mis-tags at compile / test / never | 6 / 6 / 1 (M13) | 5 / 3 / **forgotten broad tag** | 2 / 10 / 0 (M13 n.a.) | not assessed | 8 / 5 / 0 |

LOC counts are each builder's (non-blank, non-comment lines before `#[cfg(test)]`); they include data, so compare orders of magnitude, not units.

### 6.3 What each alternative taught

- **P1, flat + presets: the hierarchy earns its cost.**
  - It is simplest (420 LOC) and passes every scenario, but it pays twice.
  - 15 broad tags are duplicated by hand, and a forgotten one (removing `testing` from `no-sleep-in-tests`) is **caught by nothing** except the answer key, which production does not have.
  - "Keep the child of an ignored parent" can only be written as a snapshot of rule names. A rule added to that child later stays off silently.
  - Computed presets (`recommended` = exact ∧ unopinionated) add no drift beyond `ignore = ["heuristic", "opinionated"]`. Member lists would freeze membership but add a second place to edit.
- **P2, group-side: fails C1, wins set-level review.**
  - Once facets, sub-groups and single parents are added, the group table is P0's tree written inverted.
  - A rule's classification is spread over 3–4 hunks of one file, and M1/M2 fall from compile time to test time.
  - Its real advantage: "are these 7 rules really heuristic?" and tag pages (I2) are one hunk. Clippy goes the P0 way (category declared rule-side, groups generated).
- **P3, expressions: shorter configs, weaker guarantees.**
  - Configs are 21% shorter and need no precedence code; `exact & unopinionated` is a genuine positive intersection, which lists can only write as complements.
  - But D15 and case 2 become positional: `(X | no-sleep-in-tests) & !testing` silently drops the rule, and a contradiction check cannot see it.
  - "Empty expression" checks depend on the corpus, so a config can break when rules change.
- **P4, DAG: cheap in code, costly in meaning.**
  - Multi-parent support is about 24 lines (4%), and model B needed no change.
  - The costs are elsewhere:
    - a topic loses its single canonical path (docs, listings);
    - adding a second parent silently changes every config naming it;
    - a rule tagged `[test-timing, time]` passes all shared checks yet flips a case.
  - The corpus did not need it: `time` had to be invented.

### 6.4 Recommendation

**Keep P0: typed facet fields (D21), topic tree (D16), precedence model B (D18), lists in config.** None of the alternatives passed more CUJs, and each lost a guarantee P0 has:
- P1 loses detection of forgotten broad tags, and re-inclusion becomes a snapshot.
- P2 loses the one-place rule declaration (C1) and compile-time M1/M2.
- P3 loses D15 by construction.
- P4 loses the single path.

**Amendments to P0, borrowed from the alternatives (as revised at checkpoint 6):**

| # | Amendment | From | Status / Why |
|---|---|---|---|
| A1 | Keep single-parent tree (`parent: Option<&'static Topic>`); model B and `describe_rule` consume `ancestors()` / `path()`, never reading `parent` directly | P0, P4 | **Accepted (D33).** Keeps compile-time single-parent guarantee without a redundant `len() <= 1` test, while isolating callers if D16 ever shifts to a DAG |
| A2 | Represent `Topic` as a plain `struct` with associated `const`s (`parent: Option<&'static Topic>`) so cycles (M8) fail to compile natively with `E0391`; **drop the depth ≤ 3 limit** | P0, P4 | **Accepted (D34).** Gives 1-spot topic declarations without macros, moves M8 to native `rustc` `E0391`, and removes the arbitrary depth cap |
| A3 | Warn when a config selector entry changes no rule's outcome (shadowed selector, ~20 lines by removing each entry in turn and comparing) | P1, P3 | **Accepted if cheap, else roadmap (D35).** Requires `Config::load` to surface warnings; expression-only contradiction/blank checks dropped |
| A4 | `explain` prints each branch with its `via` provenance (`testing [via test-doubles]`) | P3 | **Deferred to T3 roadmap (D36).** Rendering concern for the `explain` surface |
| A5 | Generated per-tag listing | P2 | **Dropped (D36).** Same as I2 / filtering rendered by T3 |
| A6 | M13: prevent rules and runners from reading tags. `Rule` exposes no tags; each domain registry pairs `(rule, classification)` in a single source; `RuleSelection` resolves selectors into tag-free `RuleName` sets on `core::Config`; suppression audits use their own contract/registry; enforced by `tests/architecture_conformance.rs` (ADR 006) | P0 | **Accepted (D37).** Makes tag-free rule execution the default architecture and eliminates the last uncaught mis-tag class |

**Roadmap only:**
- a computed `recommended` view, if presets ever return (P1, NG5);
- `explain` branch provenance (`via`) in T3 (D36);
- shadowed-selector config warning if `Config::load` warning plumbing is deferred (D35).

### 6.5 The trial quality facet (D27 → D38)

- **Cost:** one line per rule, zero friction in P0; Q1 passes.
- **What it enables:** "enable only rules that catch bugs" (`select = ["reliability"]`), which no other facet expresses.
- **Risk:** it rests on DEF1, the definition the E1 reviewers could apply consistently only after stating a boundary (a mechanism that leads to wrong behaviour; readability does not count). Both then reached the same 16/10 split, but not the same sets (at least `no-zero-sleep-in-tests` differs), so agreement is good, not proven.
- **Recommendation:** keep it, **with that boundary written into the tag guide** as a yes/no test and worked examples.

### 6.6 Open questions for checkpoint 6

- **Q26:** accept §6.4 (keep P0) and amendments A1–A6?
- **Q27:** keep the quality facet (§6.5)? Its label is "Quality" (SonarQube: "software quality").
- **Q28:** accept the two missed metric targets? Rule declarations take 6 lines instead of ≤5, forced by rustfmt, not the model. Adding a topic means 2 spots in one file, not 1, and the compiler enforces both.

### 6.7 Checkpoint 6 outcome

- **D30 — Keep P0** (Q26): typed facet fields, topic tree, precedence model B, lists in config.
- **D31 — No expression selection, not even on the roadmap**: judged too speculative. P3 is archived with its lessons (§6.3).
- **D32 — 6-line rule declarations & 1-spot `struct Topic` `const`s** (Q28, refined in Phase 7): 6 lines per rule declaration is short enough. Representing `Topic` as a plain `struct` with associated `const`s (`Topic { label, parent, description, scope_note, synonyms }`) eliminates `TopicDef` and `.def()` so adding a topic is **1 spot in 1 file** with zero macros, while active topics (`all_topics()`) are derived from the registered rules and their ancestors.
- **D33 — A1 (`parent: Option<&'static Topic>`, hierarchy read via `ancestors()` / `path()`)**: keep a single-parent field so tree structure is enforced by the type system without a test, and route all callers through `ancestors()` / `path()` in `RuleSelection`.
- **D34 — A2 (compile-time cycle check via `E0391`; no depth limit)**: because each topic is a `const` and `parent` is `Option<&'static Topic>`, any cycle in `parent` links (M8) fails to compile natively with `rustc` `E0391` (`cycle detected when const-evaluating`), regardless of declaration order. The depth ≤ 3 limit (M12) is dropped everywhere.
- **D35 — A3 (shadowed-selector warning)**: warn when a `select` or `ignore` entry changes no rule's outcome (~20 lines by removing each entry in turn). Implement in T1 if `Config::load` can return warnings cheaply; otherwise defer to the roadmap. Contradiction and blank-entry checks are dropped (only needed for expressions).
- **D36 — A4 & A5 (`explain` provenance to T3; static per-tag listing dropped)**: per-branch `via` provenance (`testing [via test-doubles]`) is added to the T3 roadmap. Static per-tag listing is dropped because it duplicates I2 / T3 filtering.
- **D37 — A6 (architecture prevents rules and runners from reading tags, M13)**:
  1. `Rule` exposes no tag getter; each rule declares its `Classification` `const` in its own file.
  2. Each domain registry (`CodeLintRules`, `CodeSuppressionEngine`, `CommandLintRules`) registers `(rule, Classification)` in a single source (so an unclassified rule does not compile), and `RuleSelection` eagerly resolves `select`, `ignore`, and `per_file_ignores` into a tag-free `RuleName` filter on `core::Config`. Neither `CodeLintRules`/`CommandLintRules`/`CodeSuppressionEngine` nor `CodeLintRunner`/`CommandLintRunner` may depend on `RuleSelection`, enforced by `tests/architecture_conformance.rs` (ADR 006).
  3. Suppression audits become a distinct rule contract/registry rather than branching on `Tag::Suppression` in the runner.
- **D38 — Keep the quality facet as "Impacted quality"** (Q27, finalizes D27/D28):
  - Display label: **"Impacted quality"** (`impacted_quality: ImpactedQuality`), matching SonarQube ("software quality impacted") and ISO/IEC 25010 quality characteristics.
  - Allowed values are the nine ISO/IEC 25010:2023 characteristics (`reliability`, `maintainability`, `security`, `performance-efficiency`, `functional-suitability`, `compatibility`, `interaction-capability`, `flexibility`, `safety`), documented in the tag guide. The code enum contains **only values used by at least one rule** (`Reliability` and `Maintainability` today), so no selector can match zero rules. Each rule declares its single primary impacted quality.
  - **Boundary with Topic**: Topic names *what construct, API, or domain the rule inspects* (`testing`, `static-typing`, `async`, or future `sql`, `crypto`), whereas Impacted quality names *what software quality suffers when the rule is violated* (`reliability`, `maintainability`, `security`, `performance-efficiency`). Consequence words (`security`, `performance`, `reliability`, `maintainability`, `safety`) are never topics.
