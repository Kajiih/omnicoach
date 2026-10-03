# Phase 1: Understand — `LoggerRedundantExceptionRule`

This document records **Phase 1 (Understand)** for evaluating the legacy linter's `LoggerRedundantExceptionRule` ([legacy_lints.py:L2322-2365](../../../scratch/legacy_reference/legacy_lints.py#L2322-L2365)).

> **Status**: **VALIDATED — DROPPED (`D1`)** (2026-10-03).
> **Decision (`D1`)**: **Drop** as a standalone Omni rule and rely on **Ruff `TRY401` (`verbose-log-message`)** with `lint.logger-objects = ["loguru.logger"]` where Loguru is used. See [02_references.md §4](02_references.md) for the exact `pyproject.toml` configuration.
> **Fallback Specification (`D2`–`D7`)**: Complete Omni design specification preserved below in case self-contained parity alongside [error_log_in_except.rs](../../../src/code_lint/rules/error_log_in_except.rs) is ever revisited.

---

## 1. What `LoggerRedundantExceptionRule` Checks

In the legacy linter ([legacy_lints.py:L2322-2365](../../../scratch/legacy_reference/legacy_lints.py#L2322-L2365)), the rule inspects Python `ast.Call` nodes and flags `.exception(...)` logging calls inside an `except ... as <exc>:` handler when `<exc>` is referenced in the call:

1. **Callee Matching (`_is_logger_exception_call`, [L2358-2365](../../../scratch/legacy_reference/legacy_lints.py#L2358-L2365))**:
   - Requires `node.args` to be non-empty and `node.func` to be an `ast.Attribute` with `attr == "exception"`.
   - Matches only when `func.value` is `ast.Name(id in {"logger", "log"})` (e.g., `logger.exception(...)`, `log.exception(...)`) or `ast.Attribute(attr in {"logger", "log"})` (e.g., `self.logger.exception(...)`, `cls.log.exception(...)`).
   - **Notable omission**: Does **not** match stdlib `logging.exception(...)` (`func.value` is `ast.Name(id="logging")`) or bare `exception(...)` imported from `logging`.
2. **Enclosing Handler Lookup ([L2335-2345](../../../scratch/legacy_reference/legacy_lints.py#L2335-L2345))**:
   - Walks parent AST nodes upward until the first `ast.ExceptHandler` and extracts `parent.name` (`exc_var_name`).
   - **Notable defects**:
     - Does not stop at nested `FunctionDef`, `AsyncFunctionDef`, `Lambda`, or `ClassDef` scope boundaries (unlike Omni's `is_inside_except_clause` in [python.rs:L1590-1599](../../../src/code_lint/ast/python.rs#L1590-L1599)).
     - Unconditionally `break`s at the innermost `ast.ExceptHandler`, missing outer bound exception names when a nested `try`/`except:` block is present.
3. **Exception Variable Reference Scan ([L2348-2355](../../../scratch/legacy_reference/legacy_lints.py#L2348-L2355))**:
   - Walks the entire `ast.Call` subtree (`for child in ast.walk(node)`) and flags if any `ast.Name` has `child.id == exc_var_name`.
   - Catches `logger.exception(f"Failed: {exc}")`, `logger.exception("Failed: %s", exc)`, `logger.exception("Failed: {}", exc)`, and `logger.exception("Failed: %s", str(exc))`.
   - **Notable false positives**:
     - Because `ast.walk(node)` walks into `ast.Attribute.value` and `node.keywords`, the legacy linter falsely flags structured attribute accesses on the exception (such as `logger.exception(f"HTTP {exc.status_code} on {exc.url}")` or `logger.exception("Failed", status_code=exc.status_code)`) as well as explicit `logger.exception("Failed", exc_info=exc)`.

---

## 2. Direct Comparison with Ruff `TRY401` (`verbose-log-message`) and Omni's `error-log-in-except`

### 2.1 Ruff `TRY401` (`verbose-log-message`)

Full source-level analysis in [02_references.md](02_references.md) (verified against `crates/ruff_linter/src/rules/tryceratops/rules/verbose_log_message.rs`, `helpers.rs`, and `crates/ruff_python_semantic/src/analyze/logging.rs`) shows:

1. **Does `TRY401` check `logging.exception` and `logger.exception`?**
   - **Yes.** It checks `logging.exception(...)` (including module aliases and `from logging import exception`), `logging.getLogger(...).exception(...)`, `flask.current_app.logger.exception(...)`, and any local/attribute logger candidate whose final segment starts with `log`/`LOG` or ends with `logger`/`logging`/`LOGGER`/`LOGGING` (`logger.exception`, `log.exception`, `self.logger.exception`, `app_logger.exception`).
   - **Why the legacy linter duplicated it**: When `logger` is imported from a third-party package (`from loguru import logger`), Ruff's `semantic.resolve_qualified_name(value)` resolves `logger` to `loguru.logger` (`logging.rs` lines 38–54) and returns `false` unless `lint.logger-objects = ["loguru.logger"]` is configured in `ruff.toml` / `pyproject.toml`.
2. **Which argument shapes does `TRY401` catch (`str(exc)` vs bare `exc` vs `f"{exc}"` vs `exc.args`)?**
   - `TRY401`'s `NameVisitor` walks every positional argument (`expr.arguments.args`) and collects all `Expr::Name` loads, **except** that it stops recursion at `Expr::Attribute(_) => {}` (`verbose_log_message.rs` lines 95–102):
     - **Bare `exc`** (`logger.exception("Failed: %s", exc)`, `logger.exception("Failed: {}", exc)`, `logger.exception(exc)`): **Caught** ✅
     - **`str(exc)` / `repr(exc)`** (`logger.exception("Failed: %s", str(exc))`, `logger.exception("Failed: " + str(exc))`): **Caught** ✅ (`Expr::Call` walks its arguments)
     - **`f"{exc}"` / `f"{exc!s}"` / `f"{str(exc)}"`**: **Caught** ✅ (`Expr::FString` walks interpolated expressions)
     - **`"Failed: {}".format(exc)` / `"Failed: %s" % exc`**: **Caught** ✅ (`Expr::Call` skips `func` attribute `"Failed: {}".format` and walks argument `exc`; `Expr::BinOp` walks `exc`)
     - **`exc.args` / `exc.status_code` / `exc.<attr>`**: **Skipped** (`Expr::Attribute(_) => {}`). This intentionally avoids false positives when logging structured attributes of an exception (such as `exc.status_code`, `exc.errno`, `exc.request_id`, `exc.filename`) that are not guaranteed to appear in `str(exc)`, at the minor cost of not flagging `exc.args` or `exc.__traceback__`.
     - **Keyword arguments (`exc_info=exc`, `extra={...}`)**: **Skipped** (only `expr.arguments.args` is walked), avoiding false positives on `exc_info=exc` and structured logging keyword arguments.

### 2.2 Relationship to Omni's `error-log-in-except`

- Omni's `error-log-in-except` ([error_log_in_except.rs:L25-29](../../../src/code_lint/rules/error_log_in_except.rs#L25-L29)) flags `logging.error(...)` inside an `except` block and suggests:
  > ``Replace the call with `logging.exception(...)`, which attaches the active traceback automatically.``
- Notice that two of `error-log-in-except`'s fail test cases ([L160-167](../../../src/code_lint/rules/error_log_in_except.rs#L160-L167), [L176-184](../../../src/code_lint/rules/error_log_in_except.rs#L176-L184)) are written as:
  ```python
  except ValueError as err:
      logging.error("invalid value: %s", err)
  ```
- When a developer or AI agent applies `error-log-in-except`'s suggestion mechanically—changing `logging.error` to `logging.exception` without removing `err` from the message—the result is `logging.exception("invalid value: %s", err)`, which is the exact `TRY401` / `LoggerRedundantExceptionRule` antipattern.
- However, under [Rule Design Guide §1](../rule_design_guide.md#L7-L14) (*1 Rule = 1 Antipattern; The Split Test*), `error-log-in-except` (wrong logging method drops the traceback → reliability hazard) and `TRY401` / `LoggerRedundantExceptionRule` (passing `exc` to `.exception()` duplicates the exception message → maintainability/noise hazard) have different rationales, different fixes, and different `ImpactedQuality` facets (`Reliability` vs `Maintainability`), so they must not be merged into a single rule.

---

## 3. Goals & Explicit Non-Goals

### 3.1 Goals

| ID | Goal | Reason |
| :--- | :--- | :--- |
| **G1** | Establish a clear, evidence-backed decision on whether to drop `LoggerRedundantExceptionRule` in favor of Ruff `TRY401` or implement it in Omni. | Avoids duplicating SOTA linter rules unless Omni provides material differentiation ([Tag Guide §2.3](../tag_guide.md#L82), [ROADMAP.md](../../../ROADMAP.md#L100)). |
| **G2** | Document the exact configuration (`lint.logger-objects`) needed for Ruff `TRY401` to cover `from loguru import logger`. | Solves the root cause that originally led the legacy linter to re-implement `TRY401` in `legacy_lints.py`. |
| **G3** | If implemented in Omni (Option B), eliminate all four the legacy linter defects (missing `logging.exception`, flagging `exc.<attr>` domain attributes, flagging keyword arguments like `exc_info=exc`, and ignoring nested scope boundaries). | High signal-to-noise ratio and alignment with Ruff `TRY401`'s proven exemptions. |

### 3.2 Explicit Non-Goals

| ID | Non-Goal | Reason |
| :--- | :--- | :--- |
| **NG1** | Merging redundant exception argument detection into [error_log_in_except.rs](../../../src/code_lint/rules/error_log_in_except.rs). | Violates [Rule Design Guide §1](../rule_design_guide.md#L7-L14) (*The Split Test*): `error-log-in-except` is about missing tracebacks (`Reliability`), whereas redundant `exc` in `.exception()` is about duplicate exception text (`Maintainability`). |
| **NG2** | Rust language support. | Rust's `tracing` / `log` macros (`tracing::error!(?err, "...")`) do not implicitly capture an ambient thread-local exception from a `match` / `if let Err(err)` arm; passing `err` explicitly is required in Rust. |
| **NG3** | Flagging domain attribute access on `exc` (`exc.status_code`, `exc.errno`, `exc.request_id`, `exc.filename`) in `.exception(...)` calls. | `.exception()` only formats `str(exc)` and the traceback; structured attributes on custom exception subclasses are not guaranteed to be included in `str(exc)`. |

---

## 4. Numbered Decisions (`D1`–`D7`)

| ID | Decision | Rationale |
| :--- | :--- | :--- |
| **D1** | **Primary Recommendation — Drop `LoggerRedundantExceptionRule` in Omni and use Ruff `TRY401` (`verbose-log-message`)**. | Ruff `TRY401` targets the exact same antipattern, catches every core pattern (`f"{exc}"`, `"%s", exc`, `"{}", exc`, `str(exc)`, `.format(exc)`), uses full semantic binding resolution (`BindingKind::BoundException`), and avoids the legacy linter's false positives on `exc.<attr>` and keyword arguments. For Loguru projects, setting `lint.logger-objects = ["loguru.logger"]` in `ruff.toml` / `pyproject.toml` makes `TRY401` (along with `TRY400`, `LOG*`, and `G*`) work out of the box. |
| **D2** | *(Fallback if implemented in Omni)* **Rule name**: `redundant-exception-in-log` (file `src/code_lint/rules/redundant_exception_in_log.rs`, const `RULE`). | Follows [Naming and Message Style Guide §1](../naming_and_message_style_guide.md#L13-L29): singular noun phrase naming the flagged pattern, no polarity prefix, $\le 4$ words in `kebab-case`, no library identifier. |
| **D3** | *(Fallback if implemented in Omni)* **Target & Options**: `SupportLang::Python`, `RuleTarget::All`, `RuleOptions::code_rule(BANNED)` where `BANNED` (`ListKind::Deny`) defaults to `base: &["logging.exception", "$OBJ.exception"]`. | Using `calls::find_banned_calls` with `["logging.exception", "$OBJ.exception"]` matches `logging.exception(...)`, `logger.exception(...)`, `log.exception(...)`, and `self.logger.exception(...)` without requiring import resolution for `loguru.logger`, while remaining configurable via `banned` / `extend-banned` / `remove-banned`. |
| **D4** | *(Fallback if implemented in Omni)* **Classification**: `topics: &[Topic::LOGGING, Topic::ERROR_HANDLING]`, `precision: Precision::Heuristic`, `consensus: Consensus::Unopinionated`, `impacted_quality: ImpactedQuality::Maintainability`. | Matches [error_log_in_except.rs:L38-43](../../../src/code_lint/rules/error_log_in_except.rs#L38-L43) on topics (`LOGGING`, `ERROR_HANDLING`) and consensus (`Unopinionated`), while `Precision::Heuristic` reflects `$OBJ.exception` syntactic matching and `ImpactedQuality::Maintainability` reflects log duplication/clutter rather than runtime failure ([Tag Guide §2.2–2.4](../tag_guide.md#L65-L101)). |
| **D5** | *(Fallback if implemented in Omni)* **Enclosing `except` resolution**: Walk ancestors of the call node stopping at scope boundaries (`function_definition`, `lambda`, `class_definition`), collecting bound exception identifiers (`as <name>`) from **all** enclosing `except_clause` ancestors in the same function scope. | Fixes both of the legacy linter's scope bugs: nested functions/lambdas inside an `except` block are not flagged, and nested `try`/`except` blocks preserve outer bound exception variables. |
| **D6** | *(Fallback if implemented in Omni)* **Positional-argument traversal & attribute exemptions**: Inspect only positional arguments (`CallMatch::arguments` excluding `keyword_argument`). Within each positional argument, flag bare `exc` references (including inside f-strings, `str(exc)`, `repr(exc)`, `.format(exc)`, and `% exc`) and explicitly redundant exception attributes (`exc.args`, `exc.__traceback__`), while **skipping** all other attribute accesses (`exc.status_code`, `exc.errno`, etc.) and method calls on `exc`. | Retains Ruff `TRY401`'s zero-false-positive behavior on domain exception attributes (`exc.status_code`) and keyword arguments (`exc_info=exc`, `extra={...}`), while closing `TRY401`'s minor gap on `exc.args` and `exc.__traceback__`. |
| **D7** | *(Optional enhancement to existing rule)* **Clarify `error-log-in-except` doc/rationale**: Regardless of whether `D1` (Drop) or `D2` (Implement) is chosen, update [error_log_in_except.rs:L54-60](../../../src/code_lint/rules/error_log_in_except.rs#L54-L60)'s `why_is_this_bad` to note that when switching `logging.error("...: %s", err)` to `logging.exception("...")`, the exception object `err` should be omitted from the message because `logging.exception` includes it automatically. | Prevents `error-log-in-except` from nudging users into writing `TRY401` violations when fixing `logging.error("invalid value: %s", err)`. |

---

## 5. Open Questions for User Validation (`Q1`–`Q2`)

- **Q1 (Drop vs. Implement)**:
  - Do we accept **D1 (Drop `LoggerRedundantExceptionRule` as covered by Ruff `TRY401`)** and record it in `ROADMAP.md` under *Not pursued*, or do you want to **implement `redundant-exception-in-log` in Omni (`D2`–`D6`)** so Omni catches redundant exception arguments out-of-the-box on `logger.exception` / `log.exception` (including Loguru without extra config) and `exc.args` / `exc.__traceback__` alongside [error_log_in_except.rs](../../../src/code_lint/rules/error_log_in_except.rs)?
- **Q2 (Broadening `error-log-in-except`'s Default `BANNED` List)**:
  - Currently, [error_log_in_except.rs:L15-23](../../../src/code_lint/rules/error_log_in_except.rs#L15-L23) only defaults to `base: &["logging.error"]` (explicitly passing on `logger.error("failed")` in test `logger_instance_error_not_in_banned_calls`), whereas the legacy linter and Ruff `TRY400` check `logger.error` / `log.error` too. Should `error-log-in-except` remain as-is, or should its documentation/defaults be revisited separately?
