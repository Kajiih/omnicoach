# Rule dependencies and soundness: 01 Understand

> [!NOTE]
> **Status: UNDERSTAND COMPLETE. Vocabulary derived from first principles (§2.1, prior art in `02_references.md`). Ready to design option A.**
> Scope: the ROADMAP item "Rule dependencies and soundness". How Omni rules relate to each other and to external linters, and whether a set of enabled rules can give contradictory or harmful advice.
> Line numbers are from the investigation at change `tvvyzump` and will drift. [U] marks claims not checked against code or tool output.

## 1. Problem

- Rules are designed one at a time. Nothing checks that one rule's fix is clean under the other rules, or that a rule relying on a companion (Omni or external) still makes sense when the companion is off.
- Authors already keep examples consistent across rules by hand (the dataclass and `*-collection-attribute` examples use `@dataclass(frozen=True, slots=True)` so they satisfy each other). Nothing enforces it, and it has already drifted (§3).

## 2. Goals and non-goals

- **Goals**
  - Find fix conflicts automatically: a rule's documented fix must not be flagged by another rule unless the relationship is declared.
  - Record relationships (§2.1: chain, divergent advice, relies on, delegates, partitions, duplicates) in one typed place, shown by `--explain`.
- **Non-goals**
  - Enabling or disabling external linters' rules.
  - Auto-enabling or auto-disabling Omni rules (would break ADR 007's predictable precedence).

### 2.1 Vocabulary, from first principles

Evidence and prior art: `02_references.md`.

**Objects.**

- An *input* is a file: a path and its source. The path is part of the input, not of the configuration.
- A *configuration* C is everything in `.omnilint.toml`: selection, `per-file-ignores`, rule options, enforcement modes and `test-patterns`. Whether a path is a source or a test path is computed from the input's path and C's `test-patterns`.
- Under C, a rule r has a *detector*: for an input x, the set V(r, x) of locations it flags. x is *r-clean* when V(r, x) is empty. A disabled or ignored rule flags nothing.
- r also has a *suggestion*: for each finding, the set F_r of rewrites it accepts, meaning every rewrite its template or doc recommends. The template is prose and `Example.fixed` is one member of that set.
- A rule set R is *clean* on x when x is r-clean for every r in R.

**Escapes.** Every finding can be silenced by a comment: `# omni:ignore[rule] -- reason` in `Ban` mode, or a three-word explanation in `RequireExplanation` mode (options.rs:78–99). So every input can always be made "clean" with comments, and any property that counted comments would hold trivially. Consequences:

- All relations below are defined on detectors *without* comments.
- The enforcement mode does not change any relation. It only changes how expensive the escape is.
- `RequireExplanation` is not the same as no rule. The finding still fires until someone writes the reason, so it forces a recorded decision at each site. The difference from `Ban` is only the cost and form of the escape.

**What a user needs from a rule set.** For every input x and the rules R enabled by C:

| | Property | Statement |
| :--- | :--- | :--- |
| P1 | Achievable | Some rewrite of x with the same behavior is R-clean. |
| P2 | Convergent | Starting from x and repeatedly applying any accepted rewrite for any remaining finding, every sequence reaches an R-clean program in finitely many steps. |
| P3 | Unambiguous | Findings at one location never lead to different clean code depending on which one is followed first. |
| P4 | Safe | Applying an accepted rewrite never introduces a defect that R no longer detects. |
| P5 | Complete | Each concern a rule targets is detected wherever it occurs, by that rule or by the rule it leaves the case to. |
| P6 | Economical | One problem produces one finding. |

**Relations, derived.** A relation between rules a and b is either one way a property can fail, or a statement that a property holds only if b is enabled. A directed relation (a → b) points from a to b; a symmetric one (a ↔ b) has no direction.

| Relation | Direction | Definition | Property | Example |
| :--- | :--- | :--- | :--- | :--- |
| Contradiction | a ↔ b | on some inputs, no rewrite with the same behavior is clean for both a and b | P1 fails | Ruff D203 / D211; none known in Omni |
| Cycle | a ↔ b | following a's suggestion produces code that b flags, and following b's produces code that a flags again | P2 fails | none known |
| Chain | a → b | a's suggestion introduces code that b flags, and following b's suggestion then ends clean: a's advice is an intermediate step, b's is closer to the end | P2 holds, in several steps | F1; T1; T4; SIM105 → `suppressed-exception` (T5) |
| Divergent advice | a ↔ b | a and b flag the same location, and following a's suggestion first or b's first ends in different clean code | P3 fails | T3: `name_str` ends as `name_string` or `name` |
| Relies on | a → b | following a's suggestion can introduce a defect that only b detects | P4 holds only with b | `quote-wrapped-placeholder` → S608 |
| Delegates | a → b | a leaves part of its concern to b (a sub-case, or checking the shape it suggests) | P5 holds only with b | `nullable-collection-return` → B006; `mock-in-tests` → `fake-without-protocol` |
| Partitions | a ↔ b | a and b split one concern into disjoint parts | P5 holds only with both, under agreeing options | `sleep-in-tests` / `zero-sleep-in-tests` |
| Duplicates | a ↔ b | a and b flag the same location and ask for compatible rewrites | P6 fails | `error-log-in-except` vs TRY400 ∪ G201 |

**Two findings at one location.** When a and b flag the same location with different advice, what matters is where the two paths end:

- *They meet.* `res_list` is flagged by both naming rules. `abbreviated-name` leads to `result_list`, which `type-suffixed-name` turns into `result`. `type-suffixed-name` leads to `res`, which `abbreviated-name` turns into `result`. These are two chains: P3 holds, P6 fails.
- *They do not meet.* For `name_str`, `abbreviated-name` leads to `name_string`, which no rule flags, and `type-suffixed-name` leads to `name`. That is divergent advice (T3).

Divergent advice is order-dependence at one location: the clean result depends on which finding the user follows first.

**Overlap shapes.** When a and b flag some of the same locations, the overlap has a shape:

| Shape | Definition | Example |
| :--- | :--- | :--- |
| Equivalent | a and b flag exactly the same locations | none in Omni (Ruff rule redirects) |
| Subsumes | a flags everything b flags, and more | `unstructured-task` ⊇ RUF006; `dynamic-attribute-access` ⊇ B009 ∪ B010 |
| Overlaps | some locations in common, neither contains the other | `error-log-in-except` vs TRY400 ∪ G201; `packed-assertion` vs PT018 |

The shape does not decide which property fails; the advice does. At the shared locations, compatible advice is a duplicate and paths that meet are chains (both P6); paths that do not meet are divergent advice (P3). The shape decides the remedy:

- If the rules are equivalent, or one subsumes the other, the smaller rule is redundant while the larger one is enabled.
- A partial overlap is fixed by narrowing one rule.

**Introduced versus revealed.** In a chain, b's finding on a's rewrite is either:

- *introduced* (*created redex*, Lévy 1978): a's rewrite constructs the pattern b flags from tokens in a's replacement (F1: `Sequence` was not in the input; T1: `y` was not in the input);
- *revealed* (*residual redex*): the tokens b flags were already in the input, and a's rewrite only removes surrounding context that hid them from b's detector (T2: `t` in `t_ms`).

Only introduced chains are relations between rules. A revealed chain says the input was wrong in two ways; any rename-to-stem rule reveals every naming rule, so declaring it would be noise.

**Quantifiers.** A relation exists when some input shows it. Two qualifiers say how general it is:

- *Over rewrites:* a *forced* relation holds for every accepted rewrite (T2 for `primitive-duration`, which names the stem); a *possible* one holds for some (F1, T1, T4).
- *Over configurations:* an *unconditional* relation holds under every configuration that enables a and b (`quote-wrapped-placeholder` → S608). A *conditional* one holds only under some, and names its condition: option values (T3 holds where `abbreviated-name`'s list contains `str`: Python by default, not Rust) or the path class C assigns (F3 needs the fake under a source path). The sleep partition is complete only while the two deny lists agree.

Omni's tests sample only the default configuration. Conditional relations are declared by hand, with their condition; under a user's configuration, Omni can only check those declared conditions (option B).

Lifecycle (renamed, replaced, split) relates a rule to its past, not to another rule, and is out of scope here.

**Mapping from the first draft.** An earlier version grouped relations by level (detection, fix, soundness, lifecycle, configuration). Each row maps to the vocabulary above:

| First draft | Now | Why |
| :--- | :--- | :--- |
| Equivalent / subsumes, Overlaps | Overlap shapes | Unchanged. |
| Partition | Partitions | Unchanged. |
| Chain | Chain | Unchanged. |
| Cycle | Cycle | Unchanged. The example "T5 if both rules run in Ban mode" was wrong: relations ignore comments and modes (D2), so T5 is a chain. |
| Incompatible | Contradiction | Renamed. |
| Order-dependent | Divergent advice | The same concept at one location, now that divergent advice is defined by where the paths end rather than by the first rewrite (Two findings at one location). |
| Relies on | Relies on | Unchanged. |
| Complements | Delegates | Renamed: a leaves the case to b. |
| Replaces | Out of scope | Lifecycle relates a rule to its past, not to another rule. |
| Shared option | Condition on a relation | A configuration constraint, not a relation between detectors: the sleep partition is complete only while the two deny lists agree (Quantifiers). |

**Conflict** means a failure of P1, P2 or P3: contradiction, cycle or divergent advice. Following the rules then cannot reach clean code, or reaches different clean code depending on order. A *dependency* (relies on, delegates, partitions) is a relation where a property holds only if b is enabled. It is not a conflict, but it becomes a configuration problem when C disables b while a stays enabled. A duplicate is noise.

**Hard and soft targets.** P1–P6 are ideals. Enforcing all of them would make rules more complex, so the proposal is to split them:

- **Hard (Omni must not violate them):** P1 Achievable, P2 Convergent in its termination part (no cycles), and P4 Safe. Violating these traps the user or leads them into a defect.
- **Soft (declare the relation; fix it when the fix is cheap):** P3 Unambiguous, P5 Complete, P6 Economical. Violating these costs the user time, not correctness.
- **Not a target: one-step suggestions.** "Every suggestion lands in code that all rules accept" is stronger than P2 and does not follow from P1–P6. Chains are allowed when declared. Making a rule's suggestion more precise (F1: `Iterable` instead of `Sequence`) is a quality improvement for that rule, decided case by case, not a requirement.

**Definitions versus what a test can verify.** The definitions above quantify over every input and every configuration. A test can only sample them:

- *Inputs:* the documented examples (`flagged` and `fixed`), each linted under a source path and a test path. The path is part of the input, so this samples both kinds without treating them as configuration.
- *Configurations:* only the default configuration, with every rule enabled. A test sees a conditional relation only when the default meets its condition, and can never show that a relation is unconditional.
- *Enforcement modes:* not sampled; they do not affect relations (see Escapes).
- *What a finding proves:* one example where b flags a's fix proves that a chain exists. No finding proves nothing: the relation may hold on inputs the examples do not cover.
- *Detector versus intent:* a rewrite can pass b's detector and still go against b's rationale (`name_string` still names a type). A test only sees the detector.

## 3. Inventory

### 3.1 Other rules' findings on documented examples

| ID | Fix of | Flagged by | Relation (§2.1) | Where |
| :--- | :--- | :--- | :--- | :--- |
| F1 | `concrete-collection-parameter` (`prices: Sequence[int]` passed to `sum`) | `specific-collection-parameter` (suggests `Iterable`) | possible chain, undeclared: the example keeps the hint `Sequence`, while the rule's guidance gives `Iterable` for a single pass | concrete_collection_parameter.rs:79–84 |
| F2 | `type-cast` (keeps `config: dict[str, object]`) | `concrete-collection-parameter` | overlap in the example only (the flagged snippet already has it) | type_cast.rs:71 |
| F3 | `fake-without-protocol` (`self.users: dict[str, str] = {}` in `__init__`) | `inline-public-attribute-annotation`, `concrete-collection-attribute` on non-test paths such as `testing/fakes.py` | overlap, only on some path classes | fake_without_protocol.rs:80 |
| F4 | `unmatched-logger-placeholder` (`logger.info("Order {} filled", order_id)`) | Ruff PLE1205 | chain to an external rule | unmatched_logger_placeholder.rs:75 |

Verified (2026-10-05) with a report-only prototype of option D (§6): every code rule's flagged and fixed snippets run through `runner::lint_file` with the default config, on the paths where the owning rule runs (`src/example.<ext>`, `tests/test_example.<ext>`). Findings from other Omni rules were exactly F1–F3, nothing else. F2 and F3 are noisy examples: the flagged snippet already has the other finding. F1 is a real chain, but only through the example: the message names `Sequence` as a starting point, and the example keeps it. Paused in favor of the AST policy extraction.

### 3.2 Suggestion templates

- **T1** (introduced chain, in the example only). `repeated-index-access`'s suggestion shows `x, y = point` (Python) and `let Point(x, y) = point;` (Rust). `single-letter-name` allows only `i`, `j`, `x`, `f` (and `c` in Rust), so it flags `y`. The input `point[1]` had no single-letter name; the example introduced it.
- **T2** (revealed chain, not a relation). `primitive-duration` and `type-suffixed-name` propose the stem: `t_ms` → `t`, `res_list` → `res`. The stem was already bad in the input; the suffix only hid it from `single-letter-name` and `abbreviated-name`. The same holds for `timeout_secs_int` (§3.3) and for `res_list` under both naming rules.
- **T3** (divergent advice, conditional; resolved by D9). For `name_str` in Python, `abbreviated-name` says spell out `str` and `type-suffixed-name` says rename to `name`. No rule flags `name_string`, so the two paths end in different names. Rust is not affected: `abbreviated-name` does not list `str` there. `user_num` gets both findings in both languages and also ends in two names (`user_number` or `user`). Contrast `res_list` (§2.1, Two findings at one location), where both paths end at `result`. T1–T3 verified 2026-10-05 by running `omni-code-lint` on probe files.
- **T4** (possible chain, depends on an option). `packed-assertion`'s fix (split asserts) raises the count that `too-many-assertions` caps. `too-many-assertions`' fix (compare one domain object) is not flagged by `packed-assertion`, so the chain ends clean. It happens only past the threshold option, and it is per function, not per location.
- **T5** (chain from an external rule). Ruff SIM105 rewrites `try/except: pass` to `contextlib.suppress`, which `suppressed-exception` flags. `suppressed-exception`'s fix logs in an explicit handler, which SIM105 does not flag, so the chain ends clean. It would be a cycle only if the explicit handler did nothing.
- **T6** (possible chains). `nested-function` suggests extracting a module-level `_helper` or using an *inline* `lambda` at the call site. Extracting `_helper` below its caller triggers `call-before-definition` (and `call-before-definition` would contradict the candidate "public before private" rule in ROADMAP). An inline `lambda` at the call site is clean under Ruff E731 (`lambda-assignment`), whereas assigning the `lambda` to a variable would cycle with E731.

### 3.3 Relationships that are documented only in prose, or not at all

- **Omni ↔ Omni**
  - `sleep-in-tests` ↔ `zero-sleep-in-tests` (`Partitions`, conditional on agreeing deny lists): they split the same calls in code ($t > 0$ vs $t = 0$), but each has its own `[rules.<name>]` deny list.
  - `concrete-collection-parameter` ↔ `mutable-collection-parameter` ↔ `specific-collection-parameter` (`Partitions` across `ConcreteMutable`, `AbstractMutable` and `OverspecificReadOnly` parameter annotations; `Chain` from `concrete-collection-parameter` to `mutable-collection-parameter` and `specific-collection-parameter`, and from `mutable-collection-parameter` to `specific-collection-parameter`).
  - `concrete-collection-return` ↔ `mutable-collection-return` (`Partitions` on return annotations; possible `Chain` from `concrete-collection-return` to `mutable-collection-return`).
  - `concrete-collection-attribute` ↔ `mutable-collection-attribute` (`Partitions` on attribute annotations; possible `Chain` from `concrete-collection-attribute` to `mutable-collection-attribute`).
  - `concrete-collection-parameter` / `-return` / `-attribute` and `mutable-collection-parameter` / `-return` / `-attribute` (`Partitions` by AST position).
  - `mock-in-tests` → `fake-without-protocol` (`Delegates`: `mock-in-tests` recommends a fake implementing a `Protocol`; `fake-without-protocol` enforces that invariant).
  - `nested-function` → `call-before-definition` (possible `Chain`: extracting a helper below its caller triggers `call-before-definition`).
  - `packed-assertion` → `too-many-assertions` (possible `Chain`, T4).
  - `abbreviated-name` ↔ `type-suffixed-name` (`Partitions` on naming tokens once D9 assigns `str` to `type-suffixed-name` and `num` to `abbreviated-name`).
  - `primitive-duration` / `type-suffixed-name` on `timeout_secs_int` → `timeout_secs` → `timeout`: a revealed chain (§2.1), not a relation.
  - `blanket-suppression` ↔ `unknown-suppression-rule` ↔ `missing-suppression-reason` ↔ `unused-suppression` (`Partitions` of `# omni:ignore` validation).
- **Omni → external (`ReliesOn`, `Delegates`, `Chain`)**
  - `quote-wrapped-placeholder` → Ruff S608 (`ReliesOn`): without S608, the rule suggests `{x!r}` inside SQL f-strings, which its own design doc calls harmful (quote_wrapped_placeholder/01_understand.md:59).
  - `suppressed-exception` ← Ruff SIM105 (forced `Chain`, T5) and → Ruff S110 / S112 / E722 / BLE001 (`Delegates` `try/except: pass` and bare/broad `except`).
  - `error-log-in-except` → Ruff TRY401 (`Chain` and `Delegates`: redundant exception object in `logging.exception`).
  - `unmatched-logger-placeholder` → Ruff PLE1205 / PLE1206 / G001–G004 (`Chain` F4 to PLE1205 on stdlib `logging`; `Chain` from G004; `Delegates` eager formatting and `%` counts).
  - `nullable-collection-return` → Ruff B006 / Pylint W0102 and Ruff UP007 / UP045 / RUF013 (`Delegates`: Omni checks return types only so it does not contradict B006's `x: list[int] | None = None` pattern on parameter defaults).
  - `inline-public-attribute-annotation` → Pyright / Mypy (`Delegates` unannotated `self.attr = val`).
  - `repeated-index-access` → Ruff RUF015 / `clippy::get_first` (`Delegates` single-index `x[0]` reads).
  - `mutable-module-constant` ↔ Ruff RUF012 (`Partitions` module constants vs mutable class attributes).
  - Naming rules → Ruff ICN001/ICN002, Pylint PLC0414 (`Delegates` import aliases).
- **Omni ↔ external (`Duplicates`)**
  - `Subsumes`: `unstructured-task` ⊇ Ruff RUF006 (and RUF006 → `unstructured-task` is a `Chain` because `background_tasks.add(task)` is still flagged); `dynamic-attribute-access` ⊇ Ruff B009 ∪ B010; `single-letter-name` ⊇ Ruff E741 (under default `allowed_names`).
  - `Overlaps`: `error-log-in-except` vs Ruff TRY400 ∪ G201 (Omni defaults to `logging.error` only and covers both with and without `exc_info=True`); `packed-assertion` vs Ruff PT018 (Omni adds boolean tuple equality and Rust, skips `not (a or b)`); `single-letter-name` (Rust) vs `clippy::min_ident_chars` (different default allow lists and item scopes); `repeated-literal` vs Ruff PLR2004 (Omni requires `>= 2` occurrences across strings and numbers; PLR2004 flags single numeric literals in comparisons).

## 4. How rules are selected today

- `.omnilint.toml`: `select`, `ignore`, `per-file-ignores`, `[rules.<name>]`, `[context] test-patterns`. Every rule is on when `select` is absent (ADR 007).
- Selection is resolved once at load into `Config.disabled_rules` and `per_file_ignores`. A companion check belongs there and must handle per-path ignores.
- Config problems are errors only; there is no warning channel (ROADMAP already needs one for shadowed selectors).
- `enforcement_mode` (`Ban` / `RequireExplanation`) only changes the escape: `# omni:ignore` with a reason, or a plain explanatory comment. It does not change relations (D2).
- No groups, presets or `recommended` set. Topics act as informal clusters.

## 5. State of the art

| Tool | Mechanism | Enforced? |
| :--- | :--- | :--- |
| Ruff | Hard-coded incompatible pairs (D203/D211, D212/D213): warns and ignores one. `ruff format` warns on formatter-conflicting rules. Deprecated rules warn, removed rules error. | Warning, auto-resolve |
| ESLint | `eslint-config-prettier`: a config that turns conflicting rules off, plus a CLI that reports them. `meta.deprecated` with `replacedBy`. | Off-switch, opt-in checker |
| Clippy | Lint groups. `restriction` holds contradictory pairs (`implicit_return` vs `needless_return`); `blanket_clippy_restriction_lints` warns on enabling the whole group. | Group-level warning; pairs documented only |
| Pylint | `old_names`, `useless-suppression`. No conflict pairs. | Stale options only |
| golangci-lint | `linters.default` presets; `Deprecation.Replacement` warnings. No conflict detection. | Deprecation only |
| Biome | Domains auto-enable rules from `package.json` dependencies. Rule metadata `sources: [RuleSource::Eslint(..).same()/.inspired()]` rendered in docs. | Auto-enable; sources documented only |
| Semgrep | Free-form rule metadata, no relationship model. | Documentation only |

Takeaways: nobody runs fixes across rules as a test. Enforced checks are narrow hard-coded pairs or group-level guards.

## 6. Options

- **A. Typed relationship field.** `RuleDoc.related: &'static [Related]` for directed and external links (`Chain`, `DivergentAdvice`, `ReliesOn`, `Delegates`, `Duplicates`), plus named equivalence classes (`RuleDoc.partitions: &'static [Partition]`, mirroring `Topic` in `src/rule_declaration/taxonomy.rs`) for symmetric Omni ↔ Omni `Partitions` (`02_references.md` §3). Contradictions and cycles are not declared: they break hard targets and must be fixed. Rendered by `--explain`. A registry test checks that names exist, there are no self-references, every `Partition` has at least two member rules, and `Chain` and `ReliesOn` have no cycles. Also covers ROADMAP "Typed overlap / sources field".
- **B. Config-load warnings.** Warn when an enabled rule relies on a disabled Omni rule, including on a subset of paths. Needs warning plumbing. External companions would need Ruff's resolved settings (brittle).
- **C. Auto-enable / auto-disable** (Ruff, Biome style). Breaks ADR 007's precedence, and Omni cannot enable Ruff rules. Not recommended.
- **D. Fix-conflict harness.** A test in `tests/` that runs every code rule's `Example.fixed` through all rules via `runner::lint_file` (not `rule_test!`, which ignores `RuleTarget`), under a source path and a test path. It asserts no diagnostics from other rules, except relationships declared in A. No production change. Fails today on F1–F3. Can be extended to `flagged` snippets to detect undeclared overlaps.

Recommended order: A first, registering the known relations of §3 on their rules. Then D, which reads A's declarations as its allowed exceptions. D only finds relations on the documented examples and cannot prove that a relation is absent (§2.1); it is a regression net against undeclared relations, not a proof of soundness. Then B for Omni → Omni once warning plumbing exists. Defer external checks and C.

## 7. Decisions and open questions

- **D1.** Relationships are declared in code, next to the rule, not in a separate table.
- **D2 (was Q8).** Relations are defined on detectors without suppression comments. Every finding has a comment escape in both modes, so counting comments would make every property hold trivially. Enforcement modes therefore do not affect relations, only the cost of the escape.
- **D3 (was Q7).** Checks cover every input, including both source and test paths (the path is part of the input), under the default options with every rule enabled. A relation records the options it depends on.
- **D4 (was Q9).** A relation may target a rule that does not exist yet.
- **D5 (was Q2).** Omni does not re-implement rules that the dedicated linters (Ruff and others) already have; it declares relations to them. S608 comes back as a typed `ReliesOn` entry on `quote-wrapped-placeholder`, shown by `--explain`.
- **D6 (was Q5).** `sleep-in-tests` and `zero-sleep-in-tests` should share one deny list; how is a design question. Both already use the same default (`BANNED`, sleep_in_tests.rs:15, used at lines 65 and 149), so only user overrides in the two `[rules.<name>]` sections can diverge.
- **D7 (was Q6, provisional).** The hard/soft split of §2.1 stands for now and will be revisited.
- **D8 (from the Q3 answer).** Keep each rule simple and useful on its own. Prefer declaring a relation over coding another rule's knowledge into a rule.
- **D9 (was Q3).** Each naming token has one owner. `str` belongs to `type-suffixed-name`: drop it from `abbreviated-name` in Python, as Rust already does. `num` belongs to `abbreviated-name`: drop `_num` from `type-suffixed-name`. T3 then disappears, with nothing to declare. Accepted cost: `str` outside the suffix of a Python name (`str_value`) is no longer flagged.
- **D10 (was Q1).** Start with A, after defining every relation of §3 properly. Conditional relations are declared by hand with their condition, even though tests only see the default configuration.
- **D11 (was Q4).** T2 is a revealed chain (§2.1), so nothing is declared or changed, and `t` is not added to `single-letter-name`'s allow list. T1 is a doc-example fix on `repeated-index-access`: replace `x, y = point` and `let Point(x, y) = point;` with the same full-word example in both languages (`start, end = span` in Python and `let (start, end) = span;` in Rust).
- **D12 (was Q7).** Relations record their direction (`a → b` or `a ↔ b`), with *dependency* defined for `ReliesOn`, `Delegates` and `Partitions`, and whether they are unconditional or conditional (with the condition).

## 8. Side notes

- ROADMAP names audits `unscoped-suppression` and `unexplained-suppression`; the code has `missing-suppression-reason` and `blanket-suppression`.
- ROADMAP says the sleep-rule partition is "only prose". It is enforced in code by a shared predicate; only the separate deny lists are unchecked.
