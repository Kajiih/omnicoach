# Suppression comments: 03 Design

> **Status: VALIDATED** — superseded by the user decision in 03_design.md §0.

## 0. User Decision (supersedes §1–§6 where they differ)

- **Merged into `missing-suppression-reason` (Q2 → Option 2B).** An unexplained third-party directive fails for the same reason and has the same fix as an unexplained `omni:` directive, so it is one rule, not a new `unexplained-suppression`. The audit in [suppression.rs](../../../src/code_lint/suppression.rs) now also checks Python comments starting with `noqa`, `type: ignore`, `pyright: ignore`, `ty: ignore`, `pyrefly: ignore`, `ruff: noqa|ignore|disable|file-ignore`, `flake8: noqa`, and `pylint: disable|skip-file`.
- **Reason location (D7, for both `omni:` and third-party directives):** a non-empty inline `-- <reason>` (the user confirmed Ruff and mypy accept trailing `-- text`), or a substantive standalone comment block directly above the directive's line (`CommentIndex::has_explanation_above`). One reason covers every directive chained in the same comment. This replaces D6's inline `is_substantive_explanation` check, keeping the inline rule identical to the existing `omni:` one.
- **`unscoped-suppression` dropped (Q1 → Option 1B).** Scoping is owned by the tools themselves: Ruff `PGH003` / `PGH004` / `RUF103`, mypy `ignore-without-code`, `ty` `blanket-ignore-comment`, basedpyright `reportIgnoreCommentWithoutRule`. Tracking every tool's directive grammar is not worth the remaining gaps (standard Pyright, pyrefly, `pylint: skip-file`). `blanket-suppression` keeps covering `omni:` directives.
- **Known gaps (documented in the rule's `known_problems`):** any substantive comment block above counts as the reason, even an unrelated one; third-party directives are recognized only at the start of a comment and in lowercase.

Markers: **[V]** verified in code, corpus, or a primary source; **[U]** not verified.

## 1. Proposed Rules & Verdict

**Verdict: Split into two `CodeRule`s** under `Topic::SUPPRESSION_DIRECTIVES` (mirroring `missing-suppression-reason` and `blanket-suppression` in [suppression.rs](../../../src/code_lint/suppression.rs)):

| Rule | Languages | Flags | Default mode |
| :--- | :--- | :--- | :--- |
| `unexplained-suppression` | Python | A third-party lint or type-checker suppression directive (`# noqa`, `# ruff: ...`, `# type: ignore`, `# pyright: ignore`, `# ty: ignore`, `# pyrefly: ignore`, `# pylint: ...`) without a substantive explanation. | `None` (`RuleOptions::none()`) |
| `unscoped-suppression` | Python | A third-party lint or type-checker suppression directive that omits specific rule or error codes (such as bare `# noqa`, `# type: ignore`, `# pyright: ignore`, `# ty: ignore`, `# pyrefly: ignore`, `# pylint: skip-file`, or `# pylint: disable=all`). | `None` (`RuleOptions::none()`) |

*(Note: See **Q1** on whether to keep both rules or implement only `unexplained-suppression` and delegate scoping of `# type: ignore` / `# noqa` to Ruff `PGH003` + `PGH004`, and **Q2** on `CodeRule` vs. extending `SUPPRESSION_AUDITS`.)*

---

## 2. Numbered Decisions (`D1..D8`)

- **D1 — Split `check_file_line_comments` into `unexplained-suppression` and `unscoped-suppression`.**
  Per [rule_design_guide.md](../rule_design_guide.md) §1 (*"One rule = one `ViolationTemplate`"*), an unscoped directive (`# type: ignore -- dynamic attribute`) and an unexplained directive (`# type: ignore[attr-defined]`) fail for different reasons and require different fixes (adding `[code]` vs. adding `-- reason`). Splitting them also fixes legacy defect **P3** (which let bare `# noqa -- reason` and `# type: ignore -- reason` pass) and matches the existing split between `blanket-suppression` and `missing-suppression-reason` in [suppression.rs](../../../src/code_lint/suppression.rs) [V].

- **D2 — Register as `CodeRule`s in `src/code_lint/rules/` under `Topic::SUPPRESSION_DIRECTIVES`.**
  Implementing these checks as `CodeRule`s (in a single shared file `src/code_lint/rules/unexplained_suppression.rs` exposing `UNEXPLAINED_SUPPRESSION` and `UNSCOPED_SUPPRESSION`, per [registry.rs](../../../tests/registry.rs) lines 384–408) rather than inside `SuppressionTracker::audit`:
  1. Gives both rules full `rule_test!` coverage, executed `Example`s in `--explain`, and mutation-tested exemptions in [exemption_mutations.py](../../../scripts/exemption_mutations.py) (which `SUPPRESSION_AUDITS` lack) [V];
  2. Preserves `SuppressionTracker::from_file`'s fast path (`contains_directive_prefix` checking only `"omni:"` / `"omnicoach:"`, [suppression.rs](../../../src/code_lint/suppression.rs) line 248) [V];
  3. Reuses the already-parsed tree-sitter comment list (`file.comments()`) and `CommentIndex::from_file(file)` from [comments.rs](../../../src/code_lint/semantic/comments.rs) [V].
  We widen `Topic::SUPPRESSION_DIRECTIVES` in [topic.rs](../../../src/rule_declaration/topic.rs) from `"Rules that audit omni: suppression comments themselves"` to `"Rules that audit lint and type-checker suppression directives"` [V].

- **D3 — Use `RuleOptions::none()` (no `enforcement-mode` option) and `"Suppression directive"` terminology.**
  Like `SUPPRESSION_AUDITS` in [suppression.rs](../../../src/code_lint/suppression.rs) (and tested by `rule_without_mode_adds_no_hint` in [contract.rs](../../../src/code_lint/contract.rs) line 288 and `validate_options` in [registry.rs](../../../tests/registry.rs) line 116), suppression-hygiene rules declare `RuleOptions::none()` (`enforcement_mode: None`) [V]:
  1. `unexplained-suppression` already checks for inline and adjacent comment explanations; wrapping it in `enforcement-mode = "require-explanation"` would be circular.
  2. `unscoped-suppression` must not be bypassed merely because a bare `# type: ignore -- reason` has an explanation (that was legacy defect **P3**).
  3. Using `"Suppression directive"` in `template` and `doc` (matching `missing-suppression-reason` and `blanket-suppression` in [suppression.rs](../../../src/code_lint/suppression.rs)) satisfies [registry.rs](../../../tests/registry.rs) lines 535–668 under any mode [V].

- **D4 — Scope to Python; leave Rust to Clippy (`clippy::allow_attributes_without_reason` and `clippy::allow_attributes`).**
  Rust 1.81+ and Clippy already enforce `#[expect(lint, reason = "...")]` natively via `clippy::allow_attributes_without_reason` and `clippy::allow_attributes`, both of which are already enabled as `"deny"` in this repository's [Cargo.toml](../../../Cargo.toml) (lines 60–61) [V], while `omni:` comments in Rust are already audited by [suppression.rs](../../../src/code_lint/suppression.rs) [V].

- **D5 — Supported Python directive families and grammar (shared parser in `semantic::comments`).**
  Inspect only tree-sitter `comment` nodes via `file.comments()` (eliminating **P1** string/docstring false positives). A comment may contain one directive at the start of the comment body (`# <directive>`) or multiple chained directives separated by `#` on the same line (`# noqa: F401  # type: ignore[arg-type] -- reason`, fixing **P5** and **P6**). Supported directive heads (case-insensitive, with a word boundary after the keyword so `# noquality` does not match, fixing **P5**):
  1. **Colon-scoped `noqa` family**: `noqa`, `ruff: noqa`, `flake8: noqa`.
     - Scoped when followed by `:` and one or more comma- or whitespace-separated rule codes matching `[A-Za-z]+[0-9][A-Za-z0-9]*` (such as `F401`, `E501`, `RUF100`, `PLR0913`). Requiring at least one digit in each rule code prevents plain-English words after the code from being swallowed as rule codes (fixing **P2**).
     - Any text after the last rule code (with or without `--`) is the inline explanation candidate.
     - Replacement placeholder for `unscoped-suppression`: `# noqa: CODE` (or `# ruff: noqa: CODE` / `# flake8: noqa: CODE`).
  2. **Bracket-scoped `ignore` / `disable` family**: `type: ignore`, `pyright: ignore`, `ty: ignore`, `pyrefly: ignore`, `ruff: ignore`, `ruff: disable`, `ruff: file-ignore`.
     - Scoped when immediately followed by `[<codes>]` containing at least one non-empty code token; text after `]` is the inline explanation candidate.
     - Unscoped when `[` is absent or `[]` is empty; text after the directive head (or empty `[]`) is the inline explanation candidate.
     - Replacement placeholder for `unscoped-suppression`: `<token>[code]`.
  3. **File-level blanket `pyrefly: ignore-errors`**:
     - Always unscoped (`is_scoped = false`); text after `ignore-errors` is the inline explanation candidate.
     - Replacement placeholder for `unscoped-suppression`: `# pyrefly: ignore[error-kind]`.
  4. **Pylint disable family**: `pylint: disable`, `pylint: disable-next`, `pylint: disable-all`, `pylint: skip-file`.
     - `pylint: disable=<msgs>` and `pylint: disable-next=<msgs>` are scoped when `<msgs>` is a non-empty comma-separated list of message tokens (`[A-Za-z0-9_-]+`) that does not contain `all`. Any text after the `<msgs>` list is the inline explanation candidate.
     - `pylint: disable` (without `=<msgs>`), `pylint: disable=all`, `pylint: disable-next=all`, `pylint: disable-all`, and `pylint: skip-file` are unscoped (`is_scoped = false`).
     - Replacement placeholder for `unscoped-suppression`: `# pylint: disable=rule-name`.

- **D6 — Reuse `comments::is_substantive_explanation` (`>= 3` words, `>= 10` non-whitespace chars) instead of the legacy linter's fragile verb/indicator heuristic.**
  The legacy linter's 4-word / 15-char threshold plus its 10-verb `what_verbs` and 24-word `why_indicators` list ([legacy_lints.py](../../../scratch/legacy_reference/legacy_lints.py) lines 75–146) is brittle and English-grammar-specific [V]. Omni already standardizes explanation quality across `missing-suppression-reason` ([suppression.rs](../../../src/code_lint/suppression.rs) line 394) and `require-explanation` mode ([comments.rs](../../../src/code_lint/semantic/comments.rs) line 213) via `comments::is_substantive_explanation`: at least 3 words and 10 non-whitespace characters, excluding bare 3-word `TODO`/`FIXME` placeholders [V]. Using the same validator keeps explanation standards uniform across all of Omni.

- **D7 — Accept explanations inline, across chained same-line directives, or in the adjacent preceding standalone comment block.**
  A directive satisfies `unexplained-suppression` if any of the following holds:
  1. **Inline explanation**: The text after the directive (or after the last directive in a chained `# dir1  # dir2 -- reason` comment on the same line) satisfies `is_substantive_explanation` (fixes **P6**);
  2. **Preceding standalone comment block**: For a trailing directive on a code line (or a standalone directive comment above a statement), `comment_index.has_explanation_before_line(line)` is true or another line in the same contiguous standalone comment block satisfies `is_substantive_explanation` (fixes **P8**).

- **D8 — Independent evaluation on bare directives (`# type: ignore` with neither `[code]` nor `-- reason`).**
  Matching `audit_single_directive` in [suppression.rs](../../../src/code_lint/suppression.rs) (lines 495–503), `unscoped-suppression` checks `is_scoped` and `unexplained-suppression` checks `has_explanation` independently [V]. Span is the comment node's `SourceSpan`.

---

## 3. Rule Declarations & Templates

### 3.1 `unexplained-suppression`

- **Name**: `RuleName("unexplained-suppression")`
- **Topic**: `Topic::SUPPRESSION_DIRECTIVES`
- **Languages**: `&[Language::Python]`
- **Options**: `RuleOptions::none()`
- **Tags**: `&[Tag::Confidence(Confidence::High), Tag::FixEffort(FixEffort::Local)]`
- **`ViolationTemplate`**:
  - `summary`: ``LanguageText::new("Suppression directive `{token}` has no explanation.")``
  - `rationale`: ``LanguageText::new("Without a reason, readers cannot tell why the check is suppressed or whether the workaround is still needed.")``
  - `suggestion`: ``LanguageText::new("Add `-- <reason>` after `{token}` explaining why the check cannot be satisfied in code.")``

### 3.2 `unscoped-suppression`

- **Name**: `RuleName("unscoped-suppression")`
- **Topic**: `Topic::SUPPRESSION_DIRECTIVES`
- **Languages**: `&[Language::Python]`
- **Options**: `RuleOptions::none()`
- **Tags**: `&[Tag::Confidence(Confidence::High), Tag::FixEffort(FixEffort::Local)]`
- **`ViolationTemplate`**:
  - `summary`: ``LanguageText::new("Suppression directive `{token}` does not name a specific rule or error code.")``
  - `rationale`: ``LanguageText::new("An unscoped directive silences every check from that tool in its scope, hiding unrelated problems introduced later.")``
  - `suggestion`: ``LanguageText::new("Specify the suppressed code in `{token}`, such as `{replacement}`.")``

---

## 4. Exemptions (`E1..E9`)

Every exemption below maps 1-to-1 to a dedicated `pass` test case for [exemption_mutations.py](../../../scripts/exemption_mutations.py):

| # | Rule(s) | Exemption | Rationale |
| :--- | :--- | :--- | :--- |
| **E1** | Both | Closing re-enable directives (`# ruff: enable[F401]`, `# pylint: enable=protected-access`). | Re-enables checks at the end of a suppression block; the opening `disable` directive already carries the code and reason. |
| **E2** | Both | Omni's own directives (`# omni: ignore[rule]`, `# omni: disable-file[rule]`, `# omnicoach: ...`). | Audited by `SuppressionTracker` in [suppression.rs](../../../src/code_lint/suppression.rs) (`missing-suppression-reason`, `blanket-suppression`). |
| **E3** | Both | Directive-like text inside string literals or docstrings (`s = "# type: ignore"`). | Tree-sitter `file.comments()` visits only real comment nodes (fixes **P1**). |
| **E4** | Both | Prose comments that mention a directive mid-sentence (`# Avoid using # type: ignore here`). | Directives are recognized only at the start of a comment or after a chained directive delimiter (fixes **P5**). |
| **E5** | Both | Words sharing a prefix with a directive keyword (`# noquality check`, `# noexcept`). | Keyword matching enforces a word boundary after `noqa` / `ty` / `ruff` (fixes **P5**). |
| **E6** | `unexplained-suppression` | Directive with an inline substantive explanation (`# type: ignore[arg-type] -- dynamic callback from plugin`). | Satisfies `comments::is_substantive_explanation` (`>= 3` words and `>= 10` non-whitespace chars), including `# noqa: F401 needed because package reexports symbol` without `--` (fixes **P2**). |
| **E7** | `unexplained-suppression` | Chained directives on one line sharing a trailing explanation (`# noqa: F401  # type: ignore[arg-type] -- re-exported dynamic symbol`). | Trailing explanation applies to all chained directives on that line (fixes **P6**). |
| **E8** | `unexplained-suppression` | Directive explained by the adjacent preceding standalone comment block (or an adjacent comment line in the same contiguous comment block). | Allows placing a longer explanation on the line directly above the statement so the trailing `# type: ignore[code]` stays within line-length limits (fixes **P8**). |
| **E9** | `unscoped-suppression` | Directives that name at least one specific rule or error code (`# noqa: F401`, `# ruff: noqa: F401`, `# ruff: ignore[F401]`, `# type: ignore[arg-type]`, `# pyright: ignore[reportAttributeAccessIssue]`, `# ty: ignore[unsupported-operator]`, `# pyrefly: ignore[bad-assignment]`, `# pylint: disable=protected-access`). | Directive is properly scoped to specific rule/error code(s). |

---

## 5. Planned Test Grid (`rule_test!`)

### 5.1 `unexplained-suppression`

- **`fail` cases**:
  - `noqa_without_reason`: `import os  # noqa: F401`
  - `ruff_noqa_without_reason`: `import os  # ruff: noqa: F401`
  - `ruff_ignore_without_reason`: `# ruff: ignore[F401]\nimport os`
  - `type_ignore_without_reason`: `x: int = "a"  # type: ignore[assignment]`
  - `pyright_ignore_without_reason`: `x: int = "a"  # pyright: ignore[reportAssignmentType]`
  - `ty_ignore_without_reason`: `x: int = "a"  # ty: ignore[invalid-assignment]`
  - `pyrefly_ignore_without_reason`: `x: int = "a"  # pyrefly: ignore[bad-assignment]`
  - `pylint_disable_without_reason`: `x = obj._secret  # pylint: disable=protected-access`
  - `too_short_explanation`: `x: int = "a"  # type: ignore[assignment] -- todo`
  - `chained_directives_without_reason`: `import os  # noqa: F401  # type: ignore[import-not-found]  # pyright: ignore[reportMissingImports]`
- **`pass` cases (1-to-1 with `E1..E8`)**:
  - `reenable_directive_exempt` (**E1**): `# ruff: enable[F401]\n# pylint: enable=protected-access`
  - `omni_directive_exempt` (**E2**): `x = 1  # omni: ignore[some-rule]`
  - `directive_inside_string_or_docstring_exempt` (**E3**): `DOC = "# type: ignore[assignment]"`
  - `prose_mentioning_directive_exempt` (**E4**): `# Never use # type: ignore[assignment] in this module.`
  - `keyword_prefix_collision_exempt` (**E5**): `# noquality assurance check`
  - `inline_explanation_exempt` (**E6**): `import os  # noqa: F401 needed because package reexports symbol`
  - `chained_directives_shared_reason_exempt` (**E7**): `import os  # noqa: F401  # type: ignore[import-not-found] -- optional C extension re-exported at import time`
  - `preceding_comment_block_explanation_exempt` (**E8**): `# C extension stub is generated at build time and missing in static analysis.\nimport _fast_ext  # type: ignore[import-not-found]`

### 5.2 `unscoped-suppression`

- **`fail` cases**:
  - `bare_noqa`: `import os  # noqa -- re-exported for backwards compatibility`
  - `bare_ruff_noqa`: `# ruff: noqa -- generated protocol file`
  - `bare_flake8_noqa`: `# flake8: noqa -- generated protocol file`
  - `bare_type_ignore`: `x: int = "a"  # type: ignore -- dynamic payload coerced by validator`
  - `empty_brackets_type_ignore`: `x: int = "a"  # type: ignore[] -- dynamic payload coerced by validator`
  - `bare_pyright_ignore`: `x: int = "a"  # pyright: ignore -- dynamic payload coerced by validator`
  - `bare_ty_ignore`: `x: int = "a"  # ty: ignore -- dynamic payload coerced by validator`
  - `bare_pyrefly_ignore`: `x: int = "a"  # pyrefly: ignore -- dynamic payload coerced by validator`
  - `pyrefly_ignore_errors`: `# pyrefly: ignore-errors -- generated file`
  - `pylint_skip_file_and_disable_all`: `# pylint: skip-file -- generated file\nx = 1  # pylint: disable=all -- legacy module`
- **`pass` cases (1-to-1 with `E1..E5`, `E9`)**:
  - `reenable_directive_exempt` (**E1**): `# ruff: enable\n# pylint: enable=all`
  - `omni_directive_exempt` (**E2**): `x = 1  # omni: ignore -- audited by suppression.rs`
  - `directive_inside_string_exempt` (**E3**): `s = "# type: ignore"`
  - `prose_mentioning_directive_exempt` (**E4**): `# Do not write # type: ignore without a code.`
  - `keyword_prefix_collision_exempt` (**E5**): `# noquality check`
  - `scoped_directives_exempt` (**E9**): `import os  # noqa: F401\nx: int = "a"  # type: ignore[assignment]  # pyright: ignore[reportAssignmentType]  # ty: ignore[invalid-assignment]  # pyrefly: ignore[bad-assignment]\ny = obj._x  # pylint: disable=protected-access`

---

## 6. Open Questions (`Q1..Q3`)

- **Q1 — Keep both `unexplained-suppression` and `unscoped-suppression`, or only `unexplained-suppression`?**
  - **Option 1A (Recommended)**: Implement **both** `unexplained-suppression` and `unscoped-suppression`. This directly fulfills [ROADMAP.md](../../../ROADMAP.md) lines 67–69, closes legacy defect **P3** (`# type: ignore -- reason` passing without `[code]`), and covers tools where Ruff `PGH003`/`PGH004` do not check scoping (`# pyright: ignore` in standard Pyright, `# ty: ignore`, `# pyrefly: ignore`, `# pylint: skip-file` / `disable=all`).
  - **Option 1B**: Implement **only** `unexplained-suppression` and rely on external linters/type checkers (Ruff `PGH003` + `PGH004`, Mypy `ignore-without-code`, `ty` `blanket-ignore-comment`) for rule-code scoping.

- **Q2 — Register as `CodeRule`s (`unexplained-suppression` / `unscoped-suppression`) vs. extending `SUPPRESSION_AUDITS` (`missing-suppression-reason` / `blanket-suppression`)?**
  - **Option 2A (Recommended)**: Register as `CodeRule`s in `src/code_lint/rules/unexplained_suppression.rs` under `Topic::SUPPRESSION_DIRECTIVES`. Gives full `rule_test!` harness coverage, executed `Example`s in `--explain`, and [exemption_mutations.py](../../../scripts/exemption_mutations.py) verification without slowing down `SuppressionTracker`'s `"omni:"` fast path.
  - **Option 2B**: Extend `missing-suppression-reason` and `blanket-suppression` in [suppression.rs](../../../src/code_lint/suppression.rs) to parse third-party directives in addition to `omni:`. Avoids adding two rule names, at the cost of losing `rule_test!`, `--explain` examples, and `exemption_mutations.py` coverage.

- **Q3 — Confirm Python-only scope (`D4`)**:
  Confirm that Rust lint attributes (`#[allow(...)]`, `#[expect(...)]`) should remain delegated to Clippy (`clippy::allow_attributes_without_reason = "deny"` and `clippy::allow_attributes = "deny"`, already active in [Cargo.toml](../../../Cargo.toml) lines 60–61).
