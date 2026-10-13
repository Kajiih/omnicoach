# Suppression comments: 01 Understand

> **Status: VALIDATED** — superseded by the user decision in 03_design.md §0.

Markers: **[V]** verified in code, corpus, or a primary source; **[U]** not verified.

## 1. Original Check in the Legacy Linter

The legacy linter's `check_file_line_comments` ([legacy_lints.py](../../../scratch/legacy_reference/legacy_lints.py), lines 31–148, 246–261, and 3737) scans every line of a Python source file for inline lint or type-checker suppression directives and requires a substantive `"why"` explanation on the same line [V].

### 1.1 Regex Table (`_IGNORE_PATTERNS`, lines 31–38) [V]

| Directive label | Pattern (`re.IGNORECASE`) | Group 1 captured as `comment` |
| :--- | :--- | :--- |
| `"noqa"` | `r"#\s*noqa(?::\s*[A-Z0-9,\s]+)?(.*)"` | Trailing text after optional `: <codes>` |
| `"type: ignore"` | `r"#\s*type:\s*ignore\s*(?:\[[^\]]*\])?(.*)"` | Trailing text after optional `[...]` |
| `"pyright: ignore"` | `r"#\s*pyright:\s*ignore\s*(?:\[[^\]]*\])?(.*)"` | Trailing text after optional `[...]` |
| `"ty: ignore"` | `r"#\s*ty:\s*ignore\s*(?:\[[^\]]*\])?(.*)"` | Trailing text after optional `[...]` |
| `"pyrefly: ignore"` | `r"#\s*pyrefly:\s*ignore\s*(?:\[[^\]]*\])?(.*)"` | Trailing text after optional `[...]` |
| `"ruff: ignore"` | `r"#\s*ruff:\s*ignore\s*(?:\[[^\]]*\])?(.*)"` | Trailing text after optional `[...]` |

### 1.2 Explanation Validator (`is_valid_why_explanation`, lines 40–148) [V]

For each regex match on a line, `check_file_line_comments` passes `match.group(1)` to `is_valid_why_explanation(comment_text)`:

1. **Prefix stripping (`_clean_suppression_prefixes`, lines 62–72)** [V]: Strips at most one leading chained suppression directive (`noqa`, `type: ignore`, `pyright: ignore`, `ty: ignore`, `pyrefly: ignore`, `ruff: noqa`, `ruff: ignore`) plus optional `[...]` or `: [A-Z0-9,\s]+` and leading `[-#\s:]+`.
2. **Length floor (lines 110–116)** [V]: Rejects the explanation if it has fewer than `_MIN_WHY_WORDS = 4` space-separated words or fewer than `_MIN_WHY_CHARS = 15` characters.
3. **Action-verb filter (lines 118–146)** [V]: If the first word is an action verb describing *what* the code does (`catch`, `ignore`, `cast`, `suppress`, `capture`, `handle`, `getattr`, `hasattr`, `setattr`, `delattr`, or inflections), requires at least one subsequent word from a hardcoded 24-word `why_indicators` list (`to`, `because`, `as`, `since`, `prevent`, `avoid`, `keep`, `ensure`, `allow`, `unblocked`, `cleanly`, `runtime`, `fallback`, `retry`, `guard`, `safe`, `shield`, `recovery`, `needed`, `necessary`, `required`, `due`, `purpose`, `narrowing`).
4. **Diagnostic emitted (lines 254–260)** [V]: Formats `_EXPLANATION_RULE_TEMPLATE` (`"'{name}' must have a comment starting with 'RICR: ' explaining why it is necessary."`, lines 21–24) with `rule="inline-ignore-explanation"` and `Severity.ERROR`.

---

## 2. Defects in the Legacy Linter's Implementation

| # | Defect | Evidence in [legacy_lints.py](../../../scratch/legacy_reference/legacy_lints.py) |
| :--- | :--- | :--- |
| **P1** | **Raw line scan instead of comment tokens (false positives in strings and docstrings).** `check_file_line_comments` iterates `content.splitlines()` (line 249) instead of comment tokens. Any string literal or docstring mentioning `# type: ignore` or `# noqa` is parsed as a real directive. In `check_file_line_comments`'s own docstring (line 247), the author had to write `# type/pyright/ty/pyrefly: ignore` to avoid self-triggering, and `# noqa` on line 247 only passed because the rest of the docstring line happened to have 6 words [V]. It also ignores `changed_lines` in `--ref` diff mode (line 3737) [V]. | Lines 247–250, 3737 |
| **P2** | **Greedy `re.IGNORECASE` character class on `# noqa` swallows plain-English explanations (false positives).** Because `r"#\s*noqa(?::\s*[A-Z0-9,\s]+)?(.*)"` is compiled with `re.IGNORECASE`, `[A-Z0-9,\s]+` matches all ASCII letters and spaces. In `# noqa: F401 needed because package reexports symbol`, the entire explanation is consumed by the optional code group, leaving `match.group(1) == ""` and falsely flagging the line unless a punctuation delimiter such as `--` or `-` immediately follows the code [V]. | Line 32 |
| **P3** | **Allows unscoped (blanket) directives whenever an explanation is present (false negatives).** Because `(?::\s*[A-Z0-9,\s]+)?` and `(?:\[[^\]]*\])?` are optional (`?`) and never checked for presence, bare `# noqa -- needed for symbol re-exports` and bare `# type: ignore -- dynamic third-party attribute` pass silently while silencing every linter rule or type error on the line [V]. | Lines 32–37 |
| **P4** | **Misses `# ruff: noqa`, `# flake8: noqa`, `# ruff: disable`, `# ruff: file-ignore`, and `# pylint: disable` (false negatives).** `r"#\s*noqa..."` requires `noqa` immediately after `#\s*`, so `# ruff: noqa: F401` and `# flake8: noqa: F401` never match (even though `_clean_suppression_prefixes` on line 64 mentions `ruff:\s*(?:noqa|ignore)`). Similarly, `# pylint: disable` is listed in `_should_ignore_comment` (line 272) as a suppression directive but omitted from `_IGNORE_PATTERNS` [V]. | Lines 32–38, 64, 272 |
| **P5** | **Substring matching inside prose comments (false positives).** `regex.search(line)` matches anywhere on a line and lacks a word boundary after `noqa`, so a prose comment explaining `# Avoid using # type: ignore` or `# noquality check` triggers the regex [V]. | Lines 32–38, 251 |
| **P6** | **Stacked directives misparse when 3+ directives share a line.** `_clean_suppression_prefixes` strips only one chained directive from `match.group(1)`. When three directives are chained (`# noqa: F401  # pyright: ignore[x]  # type: ignore[arg-type, return-value]`), the third directive's tokens are counted as the 4-word explanation for the first directive [V]. | Lines 62–72 |
| **P7** | **Diagnostic message contradicts the validator.** `_EXPLANATION_RULE_TEMPLATE` instructs the user to write `"a comment starting with 'RICR: '"`, whereas `check_file_line_comments` calls `is_valid_why_explanation` (which does not check for `RICR:`), not `is_valid_ricr_explanation` [V]. | Lines 21–24, 252–255 |
| **P8** | **Ignores explanations in the preceding standalone comment block.** When a multi-line explanation sits on the comment line immediately above the statement and `# type: ignore[arg-type]` sits at the end of the statement line to keep line length under the formatter limit, `check_file_line_comments` only inspects `match.group(1)` on the directive line and rejects it [V]. | Lines 251–253 |

---

## 3. Two Orthogonal Antipatterns (The Split Test)

Per [rule_design_guide.md](../rule_design_guide.md) §1 (*"One rule = one `ViolationTemplate`. Split a rule into several when violations differ in why they are bad or how to fix them"*), third-party suppression comments fail in two distinct ways that already correspond to two separate audits in [suppression.rs](../../../src/code_lint/suppression.rs) (`blanket-suppression` and `missing-suppression-reason`) [V]:

| Failure mode | Example | Why it is bad (`rationale`) | How to fix it (`suggestion`) |
| :--- | :--- | :--- | :--- |
| **1. Unscoped (blanket) suppression directive** | `# noqa -- re-export` or `# type: ignore -- dynamic attr` | Silences every rule or type check in its scope, hiding unrelated bugs on the same line or file. | Scope the directive to specific rule or error codes (`# noqa: F401`, `# type: ignore[attr-defined]`). |
| **2. Unexplained suppression directive** | `# noqa: F401` or `# type: ignore[attr-defined]` | States *which* check is silenced, not *why* the violation is acceptable or cannot be fixed in code, leaving readers unable to tell whether the suppression is still needed. | Add an explanation stating why the check is suppressed (inline after `--` or in an adjacent comment). |

---

## 4. Relationship to Existing Omni Infrastructure

1. **`omni:` suppression audits in [suppression.rs](../../../src/code_lint/suppression.rs)** [V]:
   - Omni already defines four `Declaration` entries in `SUPPRESSION_AUDITS` under `Topic::SUPPRESSION_DIRECTIVES`:
     - `missing-suppression-reason`
     - `unused-suppression`
     - `unknown-suppression-rule`
     - `blanket-suppression`
   - Currently, `SuppressionTracker::from_file` only runs when `contains_directive_prefix(content)` finds `"omni:"` or `"omnicoach:"` ([runner.rs](../../../src/code_lint/runner.rs) line 116; [suppression.rs](../../../src/code_lint/suppression.rs) line 248) [V].
   - Furthermore, `SUPPRESSION_AUDITS` are bare `Declaration` structs without a `CodeRule` implementation, so they cannot use `rule_test!`, must declare `examples: &[]` ([registry.rs](../../../tests/registry.rs) line 197), and are not covered by [exemption_mutations.py](../../../scripts/exemption_mutations.py) [V].
2. **Existing comment and explanation utilities in [comments.rs](../../../src/code_lint/semantic/comments.rs)** [V]:
   - `CommentIndex::from_file(file)` already extracts real AST/tree-sitter comment nodes (avoiding `P1` string/docstring false positives) and groups contiguous standalone comment lines into blocks so an explanation on the line(s) immediately above a statement is linked to that statement (solving `P8`) [V].
   - `DIRECTIVE_PREFIXES` (lines 141–155) already lists `omnicoach:ignore`, `omnicoach:disable-file`, `omni:ignore`, `omni:disable-file`, `ruff: noqa`, `ruff:noqa`, `type: ignore`, `type:ignore`, `pyright: ignore`, `pyright:ignore`, `pylint: disable`, `pylint:disable`, and `noqa` [V].
   - `clean_explanation` and `is_substantive_explanation` (lines 157–238) already strip leading directive prefixes and validate that an explanation has at least `MIN_EXPLANATION_WORDS = 3` words and `MIN_EXPLANATION_CHARS = 10` non-whitespace characters and is not a bare 3-word `TODO`/`FIXME` placeholder [V].
3. **Roadmap target in [ROADMAP.md](../../../ROADMAP.md) (lines 67–69)** [V]:
   - Explicitly calls for evaluating suppression-comment hygiene to enforce scoped codes and explanations on third-party linter and type-checker directives (`# noqa`, `# type: ignore`, `# pyright: ignore`, etc.).
