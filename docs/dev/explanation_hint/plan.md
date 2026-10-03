# Explanation hint: plan (light workflow)

> Status: **implemented**. Deviations are listed in §6.

## 1. Problem

A rule in `require-explanation` mode accepts a finding explained by an adjacent comment, but nothing in the output says so. Templates cannot say it either: the mode is configurable, and the text would be false in `ban` mode (style guide §2.1). `--explain` only says "an adjacent comment", and each rule doc describes the placement in its own words.

## 2. Facts the design rests on

- `CodeRule::check_file` (`src/code_lint/contract.rs`) is the only place that knows both the effective mode for the file and the diagnostics. It already drops explained diagnostics in `require-explanation` mode.
- The placement rules are the same for every rule (`CommentIndex::has_explanation_for_span`): a trailing comment on the flagged line, the comment lines directly above it, or the same positions around the header of the enclosing statement. So "at the end of the flagged line or directly above it" is true for every finding.
- `Diagnostic` depends on no other component (`src/architecture.rs`), so it cannot hold an `EnforcementMode` (owned by `RuleDeclaration`).
- 6 rules default to `require-explanation`: `suppressed-exception` and 5 collection rules. (Corrected during implementation: most other code rules also declare a mode, defaulting to `ban` through `RuleOptions::code_rule`, so a user can switch any of them to `require-explanation`. The generic hint covers them without per-rule work.)

## 3. Design

| Option | Verdict |
| :--- | :--- |
| (a) Append to `suggestion` | Rejected: mixes framework text into the rule's single canonical fix (§2.4), and consumers cannot tell them apart. |
| (b) Store the mode on `Diagnostic`; the reporter renders the text | Rejected: needs `EnforcementMode` to move into `Diagnostic`, so the reporter would own lint policy. |
| **(c) Optional text field on `ViolationMessage`, set by `check_file`** | **Chosen.** Respects the DAG, keeps rule text and framework text apart, the text is in one place, and JSON consumers (agents) get the sentence directly. A richer or per-rule hint later changes only the text, not the shape. |

1. `ViolationMessage` gains `explanation_hint: Option<String>`. It is `None` from template rendering and omitted from JSON when `None`, so existing JSON output is unchanged. Plain output prints it as an extra line, `  Or: <hint>`, after `Suggestion:`.
2. `EnforcementMode::EXPLANATION_HINT` (next to `DOC`) holds the sentence: "Explain why in a comment at the end of the flagged line or directly above it."
3. `check_file` sets the hint on every diagnostic it keeps when the effective mode is `require-explanation`. In `ban` mode nothing is added.
4. `EnforcementMode::DOC` states the full rules once, without numeric thresholds, which stay in the code: the positions in §2, that a comment above a statement header excuses every finding on that header, that the comment must state a reason, and that a bare tool directive (`# noqa`, `# type: ignore`) does not count.
5. The placement sentences are removed from the docs of the 6 moded rules, since `--explain` now prints them from `DOC`. `suppressed-exception` drops "or add a comment…" from its suggestion and "by default" from its module doc and doc summary.
6. Guardrail: a registry test fails if the template of a rule that declares a mode contains "comment" as a fix alternative ("add a comment").
7. Style guide §2.1: the mode bullet points to this mechanism.
8. ROADMAP: the "Mode-Aware Explanation Hint" entry is removed. The "Header-wide explanation scope" entry stays.

## 4. Tests (written first)

- `contract.rs` unit cases (rstest), for a moded rule: in `require-explanation` mode a kept finding carries the hint; in `ban` mode it carries none; an explained finding is still dropped.
- A rule without a mode carries no hint.
- `diagnostic.rs`: JSON omits the field when `None`; plain output prints the `Or:` line only when set.
- Registry guardrail, plus updated CLI snapshots (only findings from moded rules change).

## 5. Out of scope

- Ban mode: no hint (no `omni:ignore` pointer).
- Per-rule hint placement (header vs line): the generic sentence is always true.

## 6. Implementation deviations

- **Guardrail is broader than planned.** It covers the template and the doc (`summary`, `what_it_does`, `why_is_this_bad`) of every rule that declares a mode, and rejects the word "comment" and the mode name `require-explanation`, not only "add a comment". All rules passed except `suppressed-exception` before migration.
- **`suppressed-exception` fixed example.** It used an explanatory comment, which is only a fix in `require-explanation` mode. It now follows the suggestion (`try`/`except` with a debug log), so it holds in both modes. Its `why_is_this_bad` second paragraph says the same instead of "state why in a comment".
- **Plain and JSON output are tested end to end** by two CLI snapshots (`explanation_hint_plain`, `explanation_hint_json`), not by `diagnostic.rs` unit tests: `print_diagnostics` writes to stdout, and the snapshots also show that a rule without the hint is unchanged.

## 7. Review (two independent reviewers: code and tests, user-facing text)

Both verdicts: keep, with changes. Mutating the hint assignment, the `retain`, or the mode branch fails `test_check_file_sets_explanation_hint_by_mode`.

- **Fixed:**
  - `DOC` now states the real rules. This reverses §3.4's "no thresholds", because users had no way to learn why a short comment was rejected. It now says:
    - the comment needs at least three words;
    - a header comment excuses findings from every rule in this mode;
    - "header" includes the decorators and excludes a block body;
    - directives are named without `#`, since `DOC` is shown for Rust rules too, and a reason written after a directive counts.
  - The guardrail now matches `comment` as a substring (`commented` used to slip through).
  - `assert_documented_examples` also runs each documented fix in `ban` mode. Proven by restoring the old comment-based `suppressed-exception` fix, which then fails "documented fix in `ban` mode".
  - The style guide states exactly what the guardrail enforces.
  - Smaller fixes:
    - the CLI test comment was wrong (`nested-function` defaults to `ban`; it is not mode-less);
    - the fixed example's log message now gives the reason;
    - `println!` arguments are all positional;
    - the test name now has the `test_` prefix;
    - 06 §3.2 has a "superseded" note.
- **Not changed:** the README pointer to `--explain` (out of scope). The commit message typo is the user's to fix.
