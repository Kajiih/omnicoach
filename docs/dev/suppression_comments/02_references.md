# Suppression comments: 02 References

> **Status: VALIDATED** — superseded by the user decision in 03_design.md §0.

Markers: **[V]** verified in code, corpus, or a primary source; **[U]** not verified.

## 1. External Linters, Type Checkers, and Compilers

| Tool / Rule or Setting | What it checks | Requires rule/error code? | Requires human explanation? | Gap / Takeaway |
| :--- | :--- | :--- | :--- | :--- |
| **Ruff `PGH003`** (`blanket-type-ignore`, `pygrep-hooks`) [V, Ruff docs] | Flags bare `# type: ignore` comments that omit `[<code, ...>]`. | Yes, for `# type: ignore` only. | **No.** | Does **not** check `# pyright: ignore`, `# ty: ignore`, or `# pyrefly: ignore`, and does **not** require a reason [V]. |
| **Ruff `PGH004`** (`blanket-noqa`, `pygrep-hooks`) [V, Ruff docs] | Flags bare `# noqa`, `# flake8: noqa`, and `# ruff: noqa` comments that omit `: <CODE>`. | Yes, for `# noqa`, `# flake8: noqa`, and `# ruff: noqa`. | **No.** | Does **not** require a reason after the rule code [V]. |
| **Ruff `RUF100` / `RUF102` / `RUF103` / `RUF104`** (`unused-noqa`, `invalid-rule-code`, `invalid-suppression-comment`, `unmatched-suppression-comment`) [V, Ruff docs] | Validates `# noqa`, `# ruff: ignore[CODE]`, `# ruff: disable[CODE]`, `# ruff: file-ignore[CODE]`, and `# ruff: enable[CODE]`. Note that Ruff's range/file directives syntactically require `[CODE]` (`RUF103` rejects bracketless `# ruff: ignore` / `# ruff: disable`). | Yes, for `# ruff: ignore/disable/file-ignore[CODE]` (`RUF103`). | **No.** | Audits unused or malformed Ruff directives, but never checks whether a suppression explains *why* it is needed [V]. |
| **Mypy `ignore-without-code`** (`--enable-error-code=ignore-without-code`) and `warn_unused_ignores` [V, Mypy docs] | `ignore-without-code` flags bare `# type: ignore` without `[code]`; `warn_unused_ignores` flags unused `# type: ignore`. | Yes, for `# type: ignore` (opt-in error code). | **No.** | Only covers `# type: ignore` when Mypy runs with `ignore-without-code` enabled; never checks for an explanation [V]. |
| **Pyright `reportUnnecessaryTypeIgnoreComment`** [V, Pyright docs] | Flags unnecessary `# type: ignore` and `# pyright: ignore` comments. Standard Pyright has **no** setting to forbid bare `# pyright: ignore` without `[rule]`. | **No** in standard Pyright (`basedpyright` adds `reportIgnoreCommentWithoutRule`). | **No.** | Standard Pyright users have no built-in check against bare `# pyright: ignore` (neither Pyright nor Ruff `PGH003` flags it), and neither requires an explanation [V]. |
| **Astral `ty`** (`blanket-ignore-comment`, `unused-ignore-comment`, `ignore-comment-unknown-rule`) [V, ty docs] | Supports `# ty: ignore[rule]` and `# type: ignore[ty:rule]`. `blanket-ignore-comment` flags bare `# ty: ignore` without `[rule]`. | Yes, for `# ty: ignore`. | **No.** | Enforces `[rule]` when `ty` runs, but never requires an explanation [V]. |
| **Meta `pyrefly`** (`# pyrefly: ignore[error-kind]`, `# pyrefly: ignore`, `# pyrefly: ignore-errors`) [V, pyrefly docs] | Supports line-level `# pyrefly: ignore[error-kind]`, bare `# pyrefly: ignore`, and file-level `# pyrefly: ignore-errors`. | **No.** | **No.** | Neither scopes nor explanations are enforced on `# pyrefly: ignore` [V]. |
| **Pylint `I0014` (`deprecated-disable-all`) & `I0021` (`useless-suppression`)** [V, Pylint docs] | Supports `# pylint: disable=<msg>`, `# pylint: disable-next=<msg>`, `# pylint: enable=<msg>`, and `# pylint: skip-file`. `I0014` flags legacy `# pylint: disable-all`. | `# pylint: disable` syntax requires `=<msg>`, except `# pylint: disable=all` or `# pylint: skip-file`. | **No.** | Pylint does not require a human explanation on `# pylint: disable=...` or `# pylint: skip-file` [V]. |
| **Rust `rustc` (`#[expect(lint, reason = "...")]`, RFC 2383)** [V, Rust 1.81 reference] | Built-in `#[expect(lint)]` warns via `unfulfilled_lint_expectations` when the suppressed lint no longer fires; supports `reason = "..."` metadata on `#[allow]` and `#[expect]`. | Syntax requires naming at least one lint or lint group. | Optional in `rustc` unless Clippy lint below is enabled. | Native compiler support for both unused-suppression detection and structured `reason = "..."` strings [V]. |
| **Clippy `clippy::allow_attributes_without_reason` & `clippy::allow_attributes`** [V, Clippy docs & [Cargo.toml](../../../Cargo.toml) lines 60–61] | `allow_attributes_without_reason` flags `#[allow(...)]` and `#[expect(...)]` attributes that lack `reason = "..."`. `allow_attributes` bans `#[allow(...)]` in favor of `#[expect(...)]`. Both are already set to `"deny"` in this repository's [Cargo.toml](../../../Cargo.toml) [V]. | Yes (Rust attribute syntax). | **Yes**, via `reason = "..."` (must be non-empty in Rust syntax). | Rust already has first-party enforcement for `#[expect(..., reason = "...")]` via Clippy [V]. |
| **Clippy `clippy::blanket_clippy_restriction_lints`** [V, Clippy docs] | Flags `#![warn/deny/forbid(clippy::restriction)]` (enabling the entire restriction group). Does **not** check `#[allow(warnings)]` or `#[allow(clippy::all)]`. | N/A | N/A | Clippy does not specifically ban `#[allow(warnings)]` or `#[allow(clippy::all)]`, though `clippy::allow_attributes = "deny"` already rejects `#[allow(...)]` [V]. |

---

## 2. Existing Rules and Helpers in `omnicoach`

| Component | Location | What it does today |
| :--- | :--- | :--- |
| **`missing-suppression-reason`** | [suppression.rs](../../../src/code_lint/suppression.rs) lines 21–48 | Audits `# omni: ignore[rule]` and `# omni: disable-file[rule]` (and `// omni: ...` in Rust) for an inline `-- <reason>` satisfying `comments::is_substantive_explanation` [V]. |
| **`blanket-suppression`** | [suppression.rs](../../../src/code_lint/suppression.rs) lines 110–137 | Audits `# omni: ignore` and `# omni: disable-file` that omit `[rule-name]` [V]. |
| **`unused-suppression` & `unknown-suppression-rule`** | [suppression.rs](../../../src/code_lint/suppression.rs) lines 50–108 | Audits `omni:` directives whose rule never fired or is not in the rule registry [V]. |
| **`Topic::SUPPRESSION_DIRECTIVES`** | [topic.rs](../../../src/rule_declaration/topic.rs) lines 63–69 | Groups suppression-hygiene rules in `--explain` and configuration docs (currently described as `"Rules that audit omni: suppression comments themselves"` with scope note `"Every file that contains an omni: directive"`) [V]. |
| **`CommentIndex`, `clean_explanation`, `is_substantive_explanation`** | [comments.rs](../../../src/code_lint/semantic/comments.rs) lines 23–238 | Extracts tree-sitter Python (`comment`) and Rust (`line_comment`, `block_comment`) nodes, links standalone comment blocks to the following statement, strips `DIRECTIVE_PREFIXES`, and checks `>= 3` words and `>= 10` non-whitespace chars (excluding bare 3-word `TODO`/`FIXME` notes) [V]. |

---

## 3. Coverage Matrix: What External Tools Miss

1. **Unexplained Python suppression directives (`# noqa: F401`, `# type: ignore[arg-type]`, `# pyright: ignore[...]`, `# ty: ignore[...]`, `# pyrefly: ignore[...]`, `# ruff: ignore[...]`, `# pylint: disable=...`)** [V]:
   - **No external Python linter or type checker enforces an explanation on suppression comments.**
   - This is the core value of the legacy linter's `check_file_line_comments` and is explicitly called out in [ROADMAP.md](../../../ROADMAP.md) lines 67–69 [V].
2. **Unscoped (blanket) Python suppression directives (`# noqa`, `# type: ignore`, `# pyright: ignore`, `# ty: ignore`, `# pyrefly: ignore`, `# pylint: skip-file` / `# pylint: disable=all`)** [V]:
   - Partial external coverage exists (`PGH003` for `# type: ignore`, `PGH004` for `# noqa` / `# ruff: noqa` / `# flake8: noqa`, `RUF103` for `# ruff: ignore/disable/file-ignore`, Mypy `ignore-without-code` for `# type: ignore`, `ty` `blanket-ignore-comment` for `# ty: ignore`).
   - **Gaps in external coverage**:
     1. Standard Pyright + Ruff does **not** flag bare `# pyright: ignore` (neither `PGH003` nor standard Pyright flags it) [V].
     2. Ruff does **not** flag bare `# ty: ignore`, `# pyrefly: ignore`, `# pyrefly: ignore-errors`, `# pylint: skip-file`, or `# pylint: disable=all` [V].
     3. The legacy linter's `check_file_line_comments` accidentally allowed bare `# noqa -- reason` and `# type: ignore -- reason` (`P3` in [01_understand.md](01_understand.md)), which [ROADMAP.md](../../../ROADMAP.md) lines 67–69 identifies as needing a scoped-directive check alongside the explanation check [V].
3. **Rust suppression attributes (`#[allow(...)]`, `#[expect(...)]`)** [V]:
   - Already covered by Clippy's `clippy::allow_attributes_without_reason` and `clippy::allow_attributes` (both enabled as `"deny"` in this repository's [Cargo.toml](../../../Cargo.toml) lines 60–61) [V].
