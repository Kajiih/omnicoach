# Phase 2: Gather Resources and References — `CatchGenericExceptionRule`

This document records **Phase 2 (Gather Resources and References)** for evaluating the legacy linter's `CatchGenericExceptionRule` ([legacy_lints.py:766-804](../../../scratch/legacy_reference/legacy_lints.py#L766-L804)). It provides the state-of-the-art (SOTA) comparison across Ruff, Pylint, Bandit, Tryceratops, Google Python Style Guide, and Omni's existing rules and suppression architecture.

Confidence markers: ✅ verified directly against official documentation and/or source code this session · ⚠️ synthesized from ecosystem usage.

---

## 1. Deep-Dive on SOTA Exception-Handling Rules

### 1.1 Ruff `BLE001` (`blind-except`, `flake8-blind-except`) ✅
- **Source**: [blind_except.rs](https://github.com/astral-sh/ruff/blob/main/crates/ruff_linter/src/rules/flake8_blind_except/rules/blind_except.rs) · [Ruff Docs: blind-except (BLE001)](https://docs.astral.sh/ruff/rules/blind-except/).
- **What it matches**:
  - `except` and `except*` handlers whose exception type expression resolves (via `SemanticModel::match_builtin_expr`) to `Exception` or `BaseException` (including `builtins.Exception` and `builtins.BaseException`).
  - Recursively inspects `Expr::Tuple` via `contains_blind_exception`, so `except (ValueError, Exception):` and `except (KeyError, BaseException):` are caught (unlike the legacy linter's `_is_generic_handler_type`, which misses tuples).
  - Does **not** match bare `except:` (`type_ == None`), leaving that to `E722` (`bare-except`) so the two distinct antipatterns remain independently configurable.
- **What `BLE001` does on `raise` and `logging.exception` (Crucial Exemptions)**:
  1. **Re-raise exemption (`ReraiseVisitor`)**:
     - Walks the statements of the `except` handler body (`body`).
     - If the handler executes a bare `raise`, re-raises the bound exception variable (`except Exception as e: ... raise e`), or raises a new exception chained `from e` (`raise CustomError from e`), `ReraiseVisitor` marks `seen = true` and **`BLE001` does not flag the handler**.
     - Furthermore, `ReraiseVisitor` tracks nested `try` blocks inside the `except` handler so that a `raise e` caught by an *inner* `except BaseException:` inside the handler does **not** falsely exempt the outer handler (verified in Ruff's test fixture [BLE.py:48-55](https://github.com/astral-sh/ruff/blob/main/crates/ruff_linter/resources/test/fixtures/flake8_blind_except/BLE.py)).
  2. **Traceback-logging exemption (`LogExceptionVisitor`)**:
     - Walks the handler body looking for logging calls on candidate loggers (`logging` module or configured `lint.logger-objects`).
     - If the handler calls `.exception(...)` (`logging.exception(...)`, `logger.exception(...)`) **or** calls `.error(...)` / `.warning(...)` / `.info(...)` / `.critical(...)` / `.log(...)` with a truthy `exc_info` keyword argument (`exc_info=True`, `exc_info=sys.exc_info()`, `exc_info=e`), `LogExceptionVisitor` marks `seen = true` and **`BLE001` does not flag the handler**.
     - Note that a plain `logging.error("failed")` (without `exc_info=True`) is **still flagged** by `BLE001` (and also flagged by `TRY400` and Omni's `error-log-in-except`) because it drops the traceback!

### 1.2 Ruff `E722` (`bare-except`, `pycodestyle`) & Pylint `W0702` (`bare-except`) ✅
- **Source**: [bare_except.rs](https://github.com/astral-sh/ruff/blob/main/crates/ruff_linter/src/rules/pycodestyle/rules/bare_except.rs) · [Ruff Docs: bare-except (E722)](https://docs.astral.sh/ruff/rules/bare-except/) · [Pylint Docs: bare-except (W0702)](https://pylint.readthedocs.io/en/latest/user_guide/messages/warning/bare-except.html).
- **What it matches**: Any `ExceptHandler` where `type` is `None` (`except:`).
- **Why it is separate from `BLE001`**:
  - Enabled by default in Ruff (`E` selector).
  - Bare `except:` catches `BaseException` (`SystemExit`, `KeyboardInterrupt`, `GeneratorExit`), whereas `except Exception:` catches `Exception` subclasses. Even when a developer genuinely wants a catch-all isolation boundary, bare `except:` is almost always a bug compared to `except Exception:`.

### 1.3 Ruff `TRY002`, `TRY203` (formerly `TRY302`), and `TRY400` (`tryceratops`) ✅
- **`TRY002` (`raise-vanilla-class`)** ([Docs](https://docs.astral.sh/ruff/rules/raise-vanilla-class/)):
  - Flags `raise Exception(...)` and `raise BaseException(...)`. This addresses the upstream root cause that forces callers to write `except Exception:`.
- **`TRY203` / `TRY302` (`useless-try-except`)** ([Docs](https://docs.astral.sh/ruff/rules/useless-try-except/)):
  - Flags `try: ... except ...: raise` where the handler immediately re-raises without performing any intermediate cleanup, rollback, or logging.
  - Works in tandem with `BLE001`: `BLE001` allows `except Exception: rollback(); raise` (meaningful cleanup before re-raise), while `TRY203` catches the degenerate `except Exception: raise` (no-op handler).
- **`TRY400` (`error-instead-of-exception`)** ([Docs](https://docs.astral.sh/ruff/rules/error-instead-of-exception/)):
  - Flags `logging.error(...)` and `logger.error(...)` inside `except` blocks, recommending `logging.exception(...)`.
  - Directly corresponds to Omni's `error-log-in-except` ([error_log_in_except.rs](../../../src/code_lint/rules/error_log_in_except.rs)).

### 1.4 Ruff `S110` (`try-except-pass`) & `S112` (`try-except-continue`, `flake8-bandit`) ✅
- **Source**: [Ruff Docs: try-except-pass (S110)](https://docs.astral.sh/ruff/rules/try-except-pass/).
- **What it matches**:
  - `try ... except ...: pass` (`S110`) and `try ... except ...: continue` (`S112`) when catching `Exception`, `BaseException`, or bare `except:` (or all typed exceptions when `lint.flake8-bandit.check-typed-exception = true`).
  - Targets silent exception swallowing (`CWE-703`), complementing Omni's `suppressed-exception` ([suppressed_exception.rs](../../../src/code_lint/rules/suppressed_exception.rs), which covers `with contextlib.suppress(...):`).

### 1.5 Pylint `W0718` (`broad-exception-caught`, formerly `W0703` `broad-except`) ✅
- **Source**: [Pylint Docs: broad-exception-caught (W0718)](https://pylint.readthedocs.io/en/latest/user_guide/messages/warning/broad-exception-caught.html).
- **What it matches**:
  - `except` clauses catching any exception listed in `overgeneral-exceptions` (defaults to `["builtins.BaseException", "builtins.Exception"]`).
  - Uses Astroid type inference to resolve aliases or subclasses.
- **Known Ecosystem Friction**:
  - Unlike Ruff's `BLE001`, Pylint `W0718` does **not** exempt handlers that log via `logging.exception(...)` at top-level worker/request isolation boundaries. In practice, this forces `# pylint: disable=broad-exception-caught` comments on every worker loop, RPC boundary, and plugin runner, without Pylint verifying that the suppression comment includes a reason.

### 1.6 Google Python Style Guide §2.4 ("Exceptions") ✅
- **Source**: [Google Python Style Guide §2.4 — Exceptions](https://google.github.io/styleguide/pyguide.html#24-exceptions).
- **Normative rule**:
  > *"Never use catch-all `except:` statements, or catch `Exception` or `StandardError`, unless you are:*
  > *1. re-raising the exception, or*
  > *2. creating an isolation point in the program where exceptions are not propagated but are recorded and suppressed instead, such as protecting a thread from crashing by guarding its outermost block."*
- **Key takeaway**: Ruff's `E722` + `BLE001` (exempting re-raise and `exc_info` logging) + `TRY203` (forbidding empty `except: raise`) is an exact static-analysis encoding of Google Python Style Guide §2.4. The legacy linter's `CatchGenericExceptionRule`, by contrast, flags both (1) re-raising handlers and (2) `logging.exception` isolation points unless a `# RICR:` comment is added.

---

## 2. Comprehensive SOTA Comparison Table

| Dimension / Case | The legacy linter `CatchGenericExceptionRule` | Ruff `BLE001` (`blind-except`) | Ruff `E722` (`bare-except`) | Ruff `TRY203` (`TRY302`) + `TRY400` | Ruff `S110` (`try-except-pass`) | Pylint `W0718` (`broad-exception-caught`) | Omni Existing (`suppressed-exception` + `error-log-in-except`) |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Bare `except:`** | Flags (unless `# RICR:` comment) | Not flagged (delegated to `E722`) | **Flags unconditionally** | Flagged if `raise` (`TRY203`) or `logging.error` (`TRY400`) | Flags if body is `pass` | Not flagged (delegated to `W0702` `bare-except`) | Flags if body calls `logging.error` |
| **`except Exception:` / `except BaseException:` with `pass` or recovery** | Flags (unless `# RICR:` comment) | **Flags** | Not flagged | Not flagged | **Flags** if body is `pass` (`S110`) | **Flags** | Covered if rewritten as `with suppress(Exception):` (`suppressed-exception`) |
| **`except builtins.Exception:`** | Flags (unless `# RICR:` comment) | **Flags** (via `SemanticModel`) | Not flagged | — | **Flags** if `pass` | **Flags** (via Astroid) | — |
| **Tuple `except (ValueError, Exception):`** | **Missed (False Negative!)** | **Flags** (`contains_blind_exception`) | Not flagged | — | **Flags** if `pass` | **Flags** | — |
| **PEP 654 `except* Exception:`** | Flags (if `ast.ExceptHandler`) | **Flags** | — | — | — | **Flags** | — |
| **Rollback/cleanup + `raise` (`except Exception: rollback(); raise`)** | **Flags (False Positive vs. Google Style §2.4)** | **Exempt** (`ReraiseVisitor`) | **Flags** (bare `except:`) | **Exempt** (not an immediate no-op `raise`) | Not flagged | **Flags** (unless re-raise is sole statement in some configs) | Not flagged |
| **Chained re-raise (`except Exception as e: raise AppError from e`)** | **Flags (False Positive)** | **Exempt** (`ReraiseVisitor`) | **Flags** (bare `except:`) | Exempt | Not flagged | **Flags** | Not flagged |
| **Degenerate `except Exception: raise` (no-op)** | Flags | Exempt in `BLE001`, **caught by `TRY203`** | **Flags** (bare `except:`) | **Flags (`TRY203`)** | Not flagged | **Flags (`W0706` `try-except-raise`)** | Not flagged |
| **Logged with traceback (`except Exception: logging.exception("...")`)** | Flags (unless `# RICR:` comment) | **Exempt** (`LogExceptionVisitor` checks `exc_info`) | **Flags** (bare `except:`) | Exempt | Not flagged | **Flags** | Exempt in `error-log-in-except` |
| **Logged without traceback (`except Exception: logging.error("...")`)** | Flags (unless `# RICR:` comment) | **Flags** (`exc_info` is false) | **Flags** (bare `except:`) | **Flags (`TRY400`)** | Not flagged | **Flags** | **Flags (`error-log-in-except`)** |
| **`with contextlib.suppress(Exception):`** | Covered by `ExceptionSuppressWithoutExplanationRule` | Not flagged (covered by `SIM105`) | Not flagged | Not flagged | Not flagged | Not flagged | **Flags (`suppressed-exception`, `RequireExplanation`)** |
| **Requires a written justification when suppressed?** | Yes (`# RICR:` comment) | Only if `# noqa: BLE001` is audited for a reason | Only if `# noqa: E722` is audited for a reason | Only if `# noqa` is audited | Only if `# noqa` is audited | No (`# pylint: disable=W0718` needs no reason) | Yes (`RequireExplanation` comment or `# omni:ignore [...] -- reason`) |

---

## 3. Internal Codebase Investigation

### 3.1 Existing Omni Exception Rules
1. **`suppressed-exception` ([suppressed_exception.rs](../../../src/code_lint/rules/suppressed_exception.rs))**:
   - Flags `with suppress(...):` and `with contextlib.suppress(...):` in Python (`RuleTarget::All`, default `EnforcementMode::RequireExplanation`).
   - Why this rule *does* add unique value over Ruff: Ruff's `SIM105` (`suppressible-exception`) actively pushes developers to replace `try ... except ...: pass` with `contextlib.suppress(...)`, and Ruff has **no rule** flagging undocumented `contextlib.suppress(...)`. Thus `suppressed-exception` fills a genuine gap in Ruff.
2. **`error-log-in-except` ([error_log_in_except.rs](../../../src/code_lint/rules/error_log_in_except.rs))**:
   - Flags `logging.error(...)` inside `except` clauses (`is_inside_except_clause` in [python.rs:1590-1599](../../../src/code_lint/ast/python.rs#L1590-L1599)), instructing developers to use `logging.exception(...)` so the active traceback is preserved.

### 3.2 How [suppression.rs](../../../src/code_lint/suppression.rs) Handles `# noqa` Today
We inspected [suppression.rs](../../../src/code_lint/suppression.rs) to answer: *"Does Omni's `missing-suppression-reason` audit ALREADY check `# noqa`? Or only `# omni:ignore`?"*

- **Finding**: `SuppressionTracker` in [suppression.rs:253-285](../../../src/code_lint/suppression.rs#L253-L285) **only checks `# omni:ignore` and `# omni:disable-file`**:
  1. Line 255 short-circuits on files without `"omni:"`:
     ```rust
     if !content.contains("omni:") {
         return Self::default();
     }
     ```
  2. `parse_directive_prefix` ([suppression.rs:367-374](../../../src/code_lint/suppression.rs#L367-L374)) matches only `"omni:disable-file"` and `"omni:ignore"`.
  3. Consequently, `missing-suppression-reason` and `blanket-suppression` do **not** currently flag `# noqa`, `# noqa: BLE001`, `# ruff: noqa`, `# type: ignore`, or `# pyright: ignore`.
- **Contrast with the legacy linter ([legacy_lints.py:32-39](../../../scratch/legacy_reference/legacy_lints.py#L32-L39))**:
  - The legacy linter had a general line-comment scanner (`check_file_line_comments`) matching `_IGNORE_PATTERNS` (`noqa`, `type: ignore`, `pyright: ignore`, `ty: ignore`, `pyrefly: ignore`, `ruff: ignore`) and requiring `is_valid_why_explanation` after the directive.
- **Contrast with [comments.rs:28-40](../../../src/code_lint/semantic/comments.rs#L28-L40)**:
  - `comments.rs` *already* defines `DIRECTIVE_PREFIXES` (`omni:ignore`, `omni:disable-file`, `ruff: noqa`, `type: ignore`, `pyright: ignore`, `pylint: disable`, `noqa`) and `clean_explanation(text)` to strip bare linter directives when evaluating `RequireExplanation` comments on Omni's own rules.

### 3.3 How [statements.rs](../../../src/code_lint/ast/statements.rs) Interacts with `except_clause`
If `CatchGenericExceptionRule` (or any rule anchoring a diagnostic on an `except_clause` in `RequireExplanation` mode) were implemented in Omni:
- In `tree-sitter-python`, `try_statement` has children: `"try"`, `":"`, `body: block`, and one or more `except_clause` (or `except_group_clause`) nodes.
- In [python.rs:15-17](../../../src/code_lint/ast/python.rs#L15-L17), `is_statement_container` only returns `true` for `"module" | "block"`.
- Therefore, in [statements.rs:29-38](../../../src/code_lint/ast/statements.rs#L29-L38), `find_enclosing_statement` for a node inside an `except_clause` header walks past `except_clause` (whose parent is `try_statement`, not `block`) and returns the outer `try_statement`!
- And `header_line_range(&try_statement)` stops at the first `block` child (the `try:` suite), returning `try_line..=try_line`.
- While `CommentIndex::has_explanation_for_span` ([comments.rs:284-286](../../../src/code_lint/semantic/comments.rs#L284-L286)) first checks `self.has_adjacent_explanation(line)` (which works for single-line `except Exception: # reason` or a standalone `# reason` line immediately above `except Exception:`), a multiline `except (\n Exception,\n):` header would not have its header range resolved unless `except_clause` / `except_group_clause` / `elif_clause` / `else_clause` / `finally_clause` were handled in `statements.rs`.
- Furthermore, the legacy linter's special case allowing comments *inside* the `except:` body before the first statement ([legacy_lints.py:788-796](../../../scratch/legacy_reference/legacy_lints.py#L788-L796)) directly contradicts Omni's `EnforcementMode::DOC` ([options.rs:96-97](../../../src/rule_declaration/options.rs#L96-L97): *"the body of a block statement is not part of its header"*).

---

## 4. Candid Assessment & Conclusion

1. **Does `CatchGenericExceptionRule` add genuine value over Ruff `BLE001` + `E722` + Omni's `suppressed-exception` and `error-log-in-except`?**
   - **No.** As an AST check on `except` clauses, it is 100% redundant with Ruff `E722` (`bare-except`) and `BLE001` (`blind-except`), and strictly less accurate than `BLE001`:
     - It misses tuple catches (`except (ValueError, Exception):`).
     - It falsely flags cleanup/rollback + `raise` and `raise ... from exc` handlers.
     - It merges two distinct antipatterns (`bare-except` and `blind-except`) that have different failure modes and remedies.
2. **What about requiring an explanation when `except Exception:` is intentionally used?**
   - When `BLE001` is enabled in Ruff, any un-reraised, un-logged `except Exception:` already requires `# noqa: BLE001` (or logging via `logging.exception`, which records the full traceback at an isolation boundary).
   - If the goal is to ensure developers cannot write bare `# noqa: BLE001` (or `# type: ignore`) without explaining *why*, that belongs in suppression-directive auditing (`# noqa: BLE001 -- <reason>`), not in a duplicate AST rule for `except Exception:` that would collide with Ruff `BLE001`.
3. **Final Decision (`VALIDATED — DROPPED`)**:
   - **Drop `CatchGenericExceptionRule` (`D1`)**, document its coverage by Ruff `BLE001` + `E722` + `TRY203` + `TRY400` + `S110` under *Not pursued* in [ROADMAP.md](../../../ROADMAP.md) (`D2`), and track third-party directive reason auditing (`# noqa`, `# type: ignore`, `# pyright: ignore`) in [ROADMAP.md](../../../ROADMAP.md) (`D3`).

---

## 5. Recommended Alternative Configuration (`pyproject.toml`)

```toml
[tool.ruff.lint]
extend-select = [
    "E722",   # bare-except: flags bare `except:`
    "BLE001", # blind-except: flags `except Exception:` / `except BaseException:` (unless re-raised or logged with exc_info)
    "TRY002", # raise-vanilla-class: flags `raise Exception(...)` / `raise BaseException(...)`
    "TRY203", # useless-try-except: flags `except ...: raise` with no intermediate action
    "TRY400", # error-instead-of-exception: flags `logging.error(...)` inside `except` blocks
    "S110",   # try-except-pass: flags `except ...: pass`
    "S112",   # try-except-continue: flags `except ...: continue`
]
# Optional: register non-stdlib loggers (such as Loguru) so BLE001 and TRY400 recognize `.exception(...)` calls on them
logger-objects = ["loguru.logger"]

[tool.ruff.lint.flake8-bandit]
# Optional: also flag `try ... except SpecificError: pass` without a comment
check-typed-exception = true
```

*(If using Pylint alongside or instead of Ruff)*:
```toml
[tool.pylint."messages control"]
enable = [
    "bare-except",            # W0702
    "broad-exception-caught", # W0718
    "try-except-raise",       # W0706
]

[tool.pylint.exceptions]
overgeneral-exceptions = ["builtins.BaseException", "builtins.Exception"]
```

---

## 6. Sources

- Ruff `BLE001` (`blind-except`): https://docs.astral.sh/ruff/rules/blind-except/ · [blind_except.rs](https://github.com/astral-sh/ruff/blob/main/crates/ruff_linter/src/rules/flake8_blind_except/rules/blind_except.rs)
- Ruff `E722` (`bare-except`): https://docs.astral.sh/ruff/rules/bare-except/ · [bare_except.rs](https://github.com/astral-sh/ruff/blob/main/crates/ruff_linter/src/rules/pycodestyle/rules/bare_except.rs)
- Ruff `TRY002` (`raise-vanilla-class`): https://docs.astral.sh/ruff/rules/raise-vanilla-class/
- Ruff `TRY203` / `TRY302` (`useless-try-except`): https://docs.astral.sh/ruff/rules/useless-try-except/
- Ruff `TRY400` (`error-instead-of-exception`): https://docs.astral.sh/ruff/rules/error-instead-of-exception/
- Ruff `S110` (`try-except-pass`): https://docs.astral.sh/ruff/rules/try-except-pass/
- Pylint `W0718` (`broad-exception-caught`): https://pylint.readthedocs.io/en/latest/user_guide/messages/warning/broad-exception-caught.html
- Pylint `W0702` (`bare-except`): https://pylint.readthedocs.io/en/latest/user_guide/messages/warning/bare-except.html
- Google Python Style Guide §2.4 ("Exceptions"): https://google.github.io/styleguide/pyguide.html#24-exceptions
