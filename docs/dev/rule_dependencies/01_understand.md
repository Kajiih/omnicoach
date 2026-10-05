# Rule dependencies and soundness: 01 Understand

> [!NOTE]
> **Status: UNDERSTAND. Waiting for answers to Q1–Q5 (§7) before design.**
> Scope: the ROADMAP item "Rule dependencies and soundness". How Omni rules relate to each other and to external linters, and whether a set of enabled rules can give contradictory or harmful advice.
> Line numbers are from the investigation at change `tvvyzump` and will drift. [U] marks claims not checked against code or tool output.

## 1. Problem

- Rules are designed one at a time. Nothing checks that one rule's fix is clean under the other rules, or that a rule relying on a companion (Omni or external) still makes sense when the companion is off.
- Authors already keep examples consistent across rules by hand (the dataclass and `*-collection-attribute` examples use `@dataclass(frozen=True, slots=True)` so they satisfy each other). Nothing enforces it, and it has already drifted (§3).

## 2. Goals and non-goals

- **Goals**
  - Find fix conflicts automatically: a rule's documented fix must not be flagged by another rule unless the relationship is declared.
  - Record relationships (relies on, overlaps, conflicts with, continues into) in one typed place, shown by `--explain`.
- **Non-goals**
  - Enabling or disabling external linters' rules.
  - Auto-enabling or auto-disabling Omni rules (would break ADR 007's predictable precedence).

## 3. Inventory

### 3.1 Fix conflicts in documented `Example.fixed` snippets

| ID | Fix of | Flagged by | Where |
| :--- | :--- | :--- | :--- |
| F1 | `concrete-collection-parameter` (`prices: Sequence[int]` passed to `sum`) | `specific-collection-parameter` (suggests `Iterable`) | concrete_collection_parameter.rs:79–84 |
| F2 | `type-cast` (keeps `config: dict[str, object]`) | `concrete-collection-parameter` | type_cast.rs:71 |
| F3 | `fake-without-protocol` (`self.users: dict[str, str] = {}` in `__init__`) | `inline-public-attribute-annotation`, `concrete-collection-attribute` on non-test paths such as `testing/fakes.py` | fake_without_protocol.rs:80 |
| F4 | `unmatched-logger-placeholder` (`logger.info("Order {} filled", order_id)`) | Ruff PLE1205 | unmatched_logger_placeholder.rs:75 |

Verified (2026-10-05) with a report-only prototype of option D (§6): every code rule's flagged and fixed snippets run through `runner::lint_file` with the default config, on the paths where the owning rule runs (`src/example.<ext>`, `tests/test_example.<ext>`). Findings from other Omni rules were exactly F1–F3, nothing else. F2 and F3 are noisy examples: the flagged snippet already has the other finding. F1 is a real chain: `concrete-collection-parameter` always suggests `Sequence`, whatever the body needs. Paused in favor of the AST policy extraction.

### 3.2 Conflicts in suggestion templates

- **T1.** `repeated-index-access` suggests `x, y = point`. `y` is not on `single-letter-name`'s allow list.
- **T2.** `type-suffixed-name` and `primitive-duration` rename to the stem without checking it: `t_ms` → `t` (single letter), `res_list` → `res` (abbreviated).
- **T3.** For `name_str`, `abbreviated-name` says spell out `str` (gives `name_string`) and `type-suffixed-name` says rename to `name`. Two findings with contradictory targets. `user_num` gets two findings in both languages.
- **T4.** `packed-assertion`'s fix (split asserts) raises the count that `too-many-assertions` caps. Both docs point to "compare one domain object" as the way out.
- **T5.** `suppressed-exception` vs Ruff SIM105: opposite directions for `try/except: pass`. Safe only because the default mode is `RequireExplanation` and the documented fix logs.
- **T6.** `nested-function` suggests an inline `lambda` (Ruff E731 if assigned [U]) and puts the private helper above the public function (conflicts with the candidate "public before private" rule).

### 3.3 Relationships that are documented only in prose, or not at all

- **Omni → Omni**
  - `sleep-in-tests` / `zero-sleep-in-tests`: they split the same calls in code, but each has its own `[rules.<name>]` deny list. Extending one silently opens a gap.
  - `mutable-collection-parameter` → `specific-collection-parameter` (a documented chain).
  - `primitive-duration` → `type-suffixed-name` (a chain: `timeout_secs_int` → `timeout_secs` → `timeout`).
  - `abbreviated-name` (Rust) leaves `str` to `type-suffixed-name` (a code comment only).
  - `mock-in-tests` suggests a fake implementing a `Protocol`; `fake-without-protocol` enforces it.
- **Omni → external**
  - `quote-wrapped-placeholder` → Ruff S608: recorded only in ROADMAP "Not pursued". Without S608, the rule suggests `{x!r}` inside SQL f-strings, which its own design doc calls harmful (quote_wrapped_placeholder/01_understand.md:59).
  - Naming rules → Ruff ICN001/ICN002, Pylint PLC0414 for import aliases (ROADMAP only).
  - `nullable-collection-return` leaves mutable defaults to Ruff B006 / Pylint W0102.
- **Overlaps**
  - `error-log-in-except` ≈ Ruff TRY400 + G201 (not in the rule doc).
  - [U] `packed-assertion` ≈ PT018, `unstructured-task` ⊇ RUF006, `dynamic-attribute-access` ⊇ B009/B010, `single-letter-name` (Rust) ≈ `clippy::min_ident_chars`, `repeated-literal` ≈ PLR2004.

## 4. How rules are selected today

- `.omnilint.toml`: `select`, `ignore`, `per-file-ignores`, `[rules.<name>]`, `[context] test-patterns`. Every rule is on when `select` is absent (ADR 007).
- Selection is resolved once at load into `Config.disabled_rules` and `per_file_ignores`. A companion check belongs there and must handle per-path ignores.
- Config problems are errors only; there is no warning channel (ROADMAP already needs one for shadowed selectors).
- `enforcement_mode` (`Ban` / `RequireExplanation`) is a second dimension that changes whether a pair conflicts (T5).
- No groups, presets or `recommended` set. Topics act as informal clusters.

## 5. State of the art

| Tool | Mechanism | Enforced? |
| :--- | :--- | :--- |
| Ruff | Hard-coded incompatible pairs (D203/D211, D212/D213): warns and ignores one. `ruff format` warns on formatter-conflicting rules. Deprecated rules warn, removed rules error. | Warning, auto-resolve |
| ESLint | `eslint-config-prettier`: a config that turns conflicting rules off, plus a CLI that reports them. `meta.deprecated` with `replacedBy`. | Off-switch, opt-in checker |
| Clippy | Lint groups. `restriction` holds contradictory pairs (`implicit_return` vs `needless_return`); `blanket_clippy_restriction_lints` warns on enabling the whole group. | Group-level warning; pairs documented only |
| Pylint | `old_names`, `useless-suppression`. No conflict pairs. | Stale options only |
| golangci-lint | `linters.default` presets; deprecation warnings with "Replaced by". No conflict detection. [U] | Deprecation only |
| Biome | Domains auto-enable rules from `package.json` dependencies. Rule metadata `sources: [RuleSource::Eslint(..).same()/.inspired()]` rendered in docs. | Auto-enable; sources documented only |
| Semgrep | Free-form rule metadata, no relationship model. [U] | Documentation only |

Takeaways: nobody runs fixes across rules as a test. Enforced checks are narrow hard-coded pairs or group-level guards.

## 6. Options

- **A. Typed relationship field.** `RuleDoc.related: &'static [Related]`, with a kind (`ReliesOn`, `Overlaps`, `ConflictsWith`, `Continues`) and a target (an Omni rule, or an external tool, code and URL). Rendered by `--explain`. A registry test checks that names exist, there are no self-references, conflicts are symmetric and `ReliesOn` has no cycles. Also covers ROADMAP "Typed overlap / sources field".
- **B. Config-load warnings.** Warn when an enabled rule relies on a disabled Omni rule, including on a subset of paths. Needs warning plumbing. External companions would need Ruff's resolved settings (brittle).
- **C. Auto-enable / auto-disable** (Ruff, Biome style). Breaks ADR 007's precedence, and Omni cannot enable Ruff rules. Not recommended.
- **D. Fix-conflict harness.** A test in `tests/` that runs every code rule's `Example.fixed` through all rules via `runner::lint_file` (not `rule_test!`, which ignores `RuleTarget`), under a source path and a test path. It asserts no diagnostics from other rules, except relationships declared in A. No production change. Fails today on F1–F3. Can be extended to `flagged` snippets to detect undeclared overlaps.

Recommended order: D, then A (gives D's exceptions a typed home), then B for Omni → Omni once warning plumbing exists. Defer external checks and C.

## 7. Decisions and open questions

- **D1.** Relationships are declared in code, next to the rule, not in a separate table.
- **Q1.** Start with D (the harness) and fix F1–F3 as separate commits? Each fix is a doc-example change, except F3, which raises whether fakes under non-test paths should get source rules.
- **Q2.** S608: you removed the sentence naming S608 from `quote-wrapped-placeholder`'s `what_it_does`. With option A, it would come back as a typed `ReliesOn` entry shown by `--explain`, not prose. Acceptable, or should external companions stay in the ROADMAP only?
- **Q3.** T3 (`name_str`): which rule owns `str` and `num`? Removing them from `abbreviated-name` in Python too (as Rust already does) gives one finding with one target.
- **Q4.** T1 and T2: should rename suggestions be checked against the naming rules (for example, fall back to "choose a descriptive name" when the stem is a single letter or an abbreviation), or only declared as known chains?
- **Q5.** The `sleep-in-tests` / `zero-sleep-in-tests` deny lists: share one configuration, or warn when they differ (needs B)?

## 8. Side notes

- ROADMAP names audits `unscoped-suppression` and `unexplained-suppression`; the code has `missing-suppression-reason` and `blanket-suppression`.
- ROADMAP says the sleep-rule partition is "only prose". It is enforced in code by a shared predicate; only the separate deny lists are unchecked.
