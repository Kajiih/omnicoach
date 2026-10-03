# Phase 1: Understand — `CatchGenericExceptionRule` (`catch-generic-exception`)

This document records **Phase 1 (Understand)** for evaluating the legacy rule `CatchGenericExceptionRule` ([legacy_lints.py:766-804](../../../scratch/legacy_reference/legacy_lints.py#L766-L804)) as a candidate rule for Omni.

> **Status**: **VALIDATED — DROPPED (Option A)** (2026-10-03).
> **Decision**: **Drop `CatchGenericExceptionRule` as a standalone AST code rule** in favor of Ruff's [E722 (bare-except)](https://docs.astral.sh/ruff/rules/bare-except/) + [BLE001 (blind-except)](https://docs.astral.sh/ruff/rules/blind-except/) + [TRY203 (useless-try-except)](https://docs.astral.sh/ruff/rules/useless-try-except/) + [TRY400](https://docs.astral.sh/ruff/rules/error-instead-of-exception/) + [S110 (try-except-pass)](https://docs.astral.sh/ruff/rules/try-except-pass/) alongside Omni's [error-log-in-except](../../../src/code_lint/rules/error_log_in_except.rs) and [suppressed-exception](../../../src/code_lint/rules/suppressed_exception.rs), and track third-party directive reason auditing (`# noqa`, `# type: ignore`, `# pyright: ignore`) in [ROADMAP.md](../../../ROADMAP.md). See [02_references.md §5](02_references.md) for the exact `pyproject.toml` configuration.

---

## 1. Context & Problem Statement

In the legacy linter ([legacy_lints.py:766-804](../../../scratch/legacy_reference/legacy_lints.py#L766-L804)), `CatchGenericExceptionRule` inspects every Python `ast.ExceptHandler` and requires an explanation comment (`# RICR: <why>`) whenever `_is_generic_handler_type(node)` returns `True`:

1. **Bare `except:`** (`node.type is None`).
2. **Unqualified `Exception` or `BaseException`** (`isinstance(node.type, ast.Name)` and `node.type.id in {"Exception", "BaseException"}`).
3. **`builtins`-qualified `Exception` or `BaseException`** (`builtins.Exception`, `builtins.BaseException`).

To determine whether an explanation comment is present, the legacy linter checks:
- `self.visitor.has_adjacent_comment(node.lineno)` (inline on the `except` line or contiguous `#` lines immediately above `except`), **or**
- A custom line scan (`for l_idx in range(node.lineno, first_stmt.lineno)`) looking for `# RICR: ...` comments written inside the `except` suite before its first statement.

### 1.1 Structural & Design Flaws in the legacy linter's `CatchGenericExceptionRule`

Evaluating `CatchGenericExceptionRule` against Omni's [Rule Design Guide](../rule_design_guide.md) and existing SOTA linters exposes five critical issues:

1. **Direct Redundancy with Ruff `E722` (`bare-except`) and `BLE001` (`blind-except`)**:
   - Every Python project using Ruff already has `E722` (enabled by default in `E`) for bare `except:` and `BLE001` (`flake8-blind-except`) for `except Exception:` and `except BaseException:`.
   - Unlike `suppressed-exception` (`contextlib.suppress`, where Ruff `SIM105` actually *recommends* `contextlib.suppress` over `try-except-pass` without requiring a reason), Ruff's `E722` and `BLE001` already **flag** generic exception handlers directly.
2. **Conflates Two Distinct Antipatterns (Violates [Rule Design Guide §1 — The Split Test](../rule_design_guide.md))**:
   - **Bare `except:`** catches `BaseException`, including `SystemExit`, `KeyboardInterrupt`, and `GeneratorExit`, preventing Ctrl-C termination, `sys.exit()`, and generator cleanup. Its fix is almost always `except Exception:` or a specific exception class (`Ruff E722`).
   - **`except Exception:`** spares `SystemExit` and `KeyboardInterrupt` (except `except BaseException:`), but catches programming errors (`TypeError`, `NameError`, `AttributeError`, `AssertionError`) alongside runtime/I/O errors. Its fix is either narrowing to expected domain/I/O exceptions or properly isolating/logging/re-raising at a boundary (`Ruff BLE001`).
3. **False Positives on Re-Raising and Traceback-Logging Handlers (Inferior to Ruff `BLE001`)**:
   - The legacy linter's `CatchGenericExceptionRule` inspects only `node.type` and ignores `node.body`.
   - Consequently, the legacy linter flags **rollback-and-reraise** handlers:
     ```python
     try:
         commit_transaction()
     except Exception:
         rollback_transaction()
         raise
     ```
     and **exception-chaining** handlers (`except Exception as exc: raise ServiceError("...") from exc`), even though the exception is *not* swallowed and is propagated immediately.
   - By contrast, both the [Google Python Style Guide §2.4](https://google.github.io/styleguide/pyguide.html#24-exceptions) and Ruff's `BLE001` (`ReraiseVisitor` and `LogExceptionVisitor`) explicitly exempt `except Exception:` blocks that re-raise (`raise`) or log the traceback with `exc_info` (`logging.exception(...)` / `logger.exception(...)`).
4. **False Negatives on Tuple Exception Handlers & `except*`**:
   - The legacy linter's `_is_generic_handler_type` checks only `ast.Name` and `ast.Attribute`, missing `except (ValueError, Exception):` and `except (KeyError, BaseException):`. Ruff's `BLE001` (`contains_blind_exception`) recursively walks tuple elements.
5. **Double-Suppression Friction & Statement-Header Mismatch in Omni**:
   - If a team enables both Ruff (`BLE001`) and an Omni `catch-generic-exception` rule:
     - In `ban` mode (Omni's default for 23 of 29 code rules), suppressing a legitimate boundary catch requires **two** directives on the same line: `# noqa: BLE001` and `# omni:ignore [catch-generic-exception] -- reason`.
     - Even in `require-explanation` mode, Omni's `CommentIndex::has_explanation_for_span` ([comments.rs:278-296](../../../src/code_lint/semantic/comments.rs#L278-L296)) and [statements.rs:15-72](../../../src/code_lint/ast/statements.rs#L15-L72) deliberately reject comments inside a block body and resolve `is_statement_container` only on `"module" | "block"` (meaning `find_enclosing_statement` on `except_clause` walks up to `try_statement`, whose header stops at `try:`).

---

## 2. The Real Gap: Suppression Reason Hygiene on `# noqa` vs. `# omni:ignore`

Why did the legacy linter have `CatchGenericExceptionRule` in the first place if Ruff already had `BLE001` and `E722`?

1. In the legacy linter, the goal was to enforce that any intentional broad catch carries a human-reviewed **WHY explanation** (`# RICR: ...`).
2. However, the legacy linter **also** had `check_file_line_comments` ([legacy_lints.py:32-39](../../../scratch/legacy_reference/legacy_lints.py#L32-L39)), which audited `# noqa`, `# ruff: ignore`, `# type: ignore`, `# pyright: ignore`, `# ty: ignore`, and `# pyrefly: ignore` across every line of the file to ensure they included a substantive explanation.
3. **Inspection of [suppression.rs:253-280](../../../src/code_lint/suppression.rs#L253-L280) in Omni**:
   - Today, Omni's four suppression audits (`missing-suppression-reason`, `blanket-suppression`, `unused-suppression`, `unknown-suppression-rule`) **only inspect `# omni:ignore` and `# omni:disable-file`** (`if !content.contains("omni:") { return Self::default(); }`).
   - Omni does **not** currently audit `# noqa: BLE001` or `# type: ignore[...]` for a missing `-- <reason>` explanation!
   - Meanwhile, [comments.rs:28-40](../../../src/code_lint/semantic/comments.rs#L28-L40) (`DIRECTIVE_PREFIXES`) *already* parses `noqa`, `ruff: noqa`, `type: ignore`, `pyright: ignore`, and `pylint: disable` to strip directive prefixes when checking `RequireExplanation` comments.

This reveals a fundamental architectural insight:
- Duplicating Ruff rules inside Omni solely to get `RequireExplanation` / `-- <reason>` enforcement does not scale (one would have to duplicate `BLE001`, `E722`, `S110`, `B006`, etc., and suffer double-suppression whenever `ban` mode is used).
- Conversely, if `# noqa: BLE001 -- <reason>` (or third-party directive reason auditing) is enforced at the suppression-audit layer, Ruff handles `BLE001` and `E722` with full semantic/re-raise/logger awareness, while Omni enforces that any `# noqa: BLE001` suppression documents *why* the broad catch is safe.

---

## 3. Goals & Non-Goals

### 3.1 Goals

| ID | Goal | Reason |
| :--- | :--- | :--- |
| **G1** | Establish whether `CatchGenericExceptionRule` adds genuine signal beyond Ruff (`BLE001`, `E722`, `TRY*`, `S110`) and Omni's existing error-handling rules (`error-log-in-except`, `suppressed-exception`). | User's core requirement ("verify that they actually add something more than regular ruff rules (generic exception i don't think so)") and Rule 2 (*Simplicity First*). |
| **G2** | Prevent false positives on idiomatic Python exception handling (cleanup/rollback + `raise`, exception chaining `raise ... from exc`, and logged isolation boundaries). | [Google Python Style Guide §2.4](https://google.github.io/styleguide/pyguide.html#24-exceptions) and [Rule Design Guide §3](../rule_design_guide.md) (*Anticipate Perverse Incentives*). |
| **G3** | Identify the exact mechanism in [suppression.rs](../../../src/code_lint/suppression.rs) regarding `# noqa` vs `# omni:ignore`. | Clarifies whether delegating `BLE001`/`E722` to Ruff leaves an un-audited `# noqa` gap in Omni. |
| **G4** | Present clear, actionable options (Drop vs. Rescope vs. Implement) with numbered decisions and open questions. | Enables an informed decision before writing any production Rust code. |

### 3.2 Explicit Non-Goals

| ID | Non-Goal | Reason |
| :--- | :--- | :--- |
| **NG1** | Re-implementing Ruff's default `E722` (`bare-except`) or `TRY400` (`error-log-in-except`) inside a generic-exception rule. | Bare `except:` is covered by `E722`, and `logging.error` inside `except` is already covered by Omni's [error-log-in-except](../../../src/code_lint/rules/error_log_in_except.rs). |
| **NG2** | Rust language support for `CatchGenericExceptionRule`. | Rust uses typed `Result<T, E>` values rather than class-hierarchy exception catching; panic catching via `std::panic::catch_unwind` is rare FFI/thread-pool boundary code, and discarded errors (`let _ = ...`) are a distinct construct (`unhandled-result` / Clippy `let_underscore_must_use`). |

---

## 4. Candidate Options & Tradeoffs

We present three concrete options for how Omni should handle `CatchGenericExceptionRule`:

### Option A (Recommended): **Drop `CatchGenericExceptionRule`** as a code rule; rely on Ruff `E722` + `BLE001` + Omni `error-log-in-except` + `suppressed-exception`
- **What we do**:
  1. Do **not** add a `catch-generic-exception` (`broad-exception-caught`) AST rule to `CODE_RULES`.
  2. Document in `ROADMAP.md` under *Not pursued* that generic exception catching is covered by Ruff `E722` (`bare-except`) and `BLE001` (`blind-except`) paired with Omni's `error-log-in-except` and `suppressed-exception`.
  3. Optionally (see **Q2**), track or implement third-party suppression directive hygiene (`# noqa`, `# type: ignore`, `# pyright: ignore`) in `ROADMAP.md` / `suppression.rs` (porting the legacy linter's `check_file_line_comments`) so `# noqa: BLE001` requires `-- <reason>` and rejects blanket `# noqa`.
- **Pros**:
  - Zero duplicate diagnostics or double-suppression (`# noqa: BLE001` + `# omni:ignore`) for users running Ruff + Omni.
  - Preserves Ruff `BLE001`'s smart exemptions for `raise` (rollback/cleanup + re-raise) and `logging.exception` (`exc_info=True`).
  - Honours Rule 2 (*Simplicity First*: minimum code that solves the problem; nothing speculative).
- **Cons**:
  - Standalone Omni runs (without Ruff `BLE001` enabled) will not flag `except Exception:`.

### Option B: **Rescope & Implement** `broad-exception-caught` with `BLE001`-Parity Exemptions
- **What we do**:
  - Implement a Python rule `broad-exception-caught` (or `generic-exception-catch`) that flags `except` / `except*` handlers catching `Exception`, `BaseException`, or bare `except:` (including inside tuples `except (ValueError, Exception):`), **exempting** handlers that re-raise (`raise`) in the handler body (matching Google Python Style Guide §2.4 and Ruff `BLE001`).
  - Default `enforcement-mode = "require-explanation"` (like `suppressed-exception`), and fix `statements.rs` so `except_clause` is recognized as having its own statement header (`except ...:`) for comment attachment.
- **Pros**:
  - Works in standalone Omni without Ruff, while avoiding the legacy linter's false positives on `raise` and false negatives on tuples.
- **Cons**:
  - Still overlaps 100% with Ruff `BLE001` + `E722` on non-reraising handlers, forcing projects that run both Ruff and Omni to either disable `BLE001` in Ruff or disable `broad-exception-caught` in Omni.

### Option C: **Implement** a Faithful Port of the legacy linter's `CatchGenericExceptionRule`
- **What we do**:
  - Flag every bare `except:`, `except Exception:`, and `except BaseException:` (even when re-raising or calling `logging.exception`) with `enforcement-mode = "require-explanation"` by default.
- **Pros**:
  - Strict 1:1 behavioral parity with the legacy linter's `CatchGenericExceptionRule` (forces an inline/preceding comment even when `logging.exception` or `raise` is used).
- **Cons**:
  - Violates Google Python Style Guide §2.4 by flagging cleanup-and-reraise (`except Exception: rollback(); raise`).
  - Violates [Rule Design Guide §1](../rule_design_guide.md) by merging `bare-except` (`E722`) and `blind-except` (`BLE001`) into one rule with conflicting remedies.
  - High friction alongside Ruff (`BLE001` / `E722`).

---

## 5. Numbered Decisions (Proposed for Validation)

| ID | Decision | Rationale |
| :--- | :--- | :--- |
| **D1** | **Drop `CatchGenericExceptionRule` as a standalone AST rule (Option A)**. | Ruff `E722` (`bare-except`) + `BLE001` (`blind-except`) + `S110` (`try-except-pass`), combined with Omni's existing `error-log-in-except` and `suppressed-exception`, already cover the entire design space with higher precision (re-raise and `exc_info` awareness, tuple handling). |
| **D2** | **Record `catch-generic-exception` under *Not pursued* in `ROADMAP.md`** (once validated in Phase 2/3). | Prevents re-evaluating the same duplicate rule in future the legacy linter migration passes and documents the exact Ruff equivalents (`BLE001`, `E722`). |
| **D3** | **Track third-party suppression comment hygiene (`# noqa`, `# type: ignore`, `# pyright: ignore`) as a separate candidate in `ROADMAP.md`** (from the legacy linter's `check_file_line_comments`). | Solves the actual root problem—requiring `-- <reason>` on `# noqa: BLE001` and other third-party tool ignores—once across *all* Ruff and type-checker rules rather than duplicating individual Ruff rules in Omni. |

---

## 6. Open Questions for User Validation

- **Q1 (Drop vs. Rescope vs. Implement)**: Do you agree with **Recommendation / Option A (D1)** to drop `CatchGenericExceptionRule` as redundant with Ruff `BLE001` + `E722` (+ Omni's `error-log-in-except` and `suppressed-exception`), or do you want **Option B** (rescoped `broad-exception-caught` with re-raise exemption) or **Option C** (strict the legacy linter port requiring an explanation comment even when `logging.exception` or `raise` is present)?
- **Q2 (Third-Party Suppression Hygiene — `# noqa` / `# type: ignore`)**: Since [suppression.rs](../../../src/code_lint/suppression.rs) currently only checks `# omni:ignore` and `# omni:disable-file` (whereas the legacy linter's `check_file_line_comments` also required explanations on `# noqa`, `# ruff: ignore`, `# type: ignore`, and `# pyright: ignore`), should we add a roadmap item (or separate rule) to audit third-party suppression comments for missing reasons?
