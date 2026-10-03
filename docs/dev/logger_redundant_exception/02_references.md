# Phase 2: Gather Resources and References — `LoggerRedundantExceptionRule`

This document records **Phase 2 (Gather Resources and References)** for evaluating the legacy linter's `LoggerRedundantExceptionRule` ([legacy_lints.py:L2322-2365](../../../scratch/legacy_reference/legacy_lints.py#L2322-L2365)). It provides the source-verified SOTA comparison backing [01_understand.md](01_understand.md).

Confidence markers: ✅ verified directly against source code in this workspace / `google3/third_party/ruff` · 📚 verified against official documentation.

---

## 1. Deep-Dive Comparison: the legacy linter `LoggerRedundantExceptionRule` vs. Ruff `TRY401` (`verbose-log-message`)

### 1.1 Source-Level Architecture of Ruff `TRY401` ✅

Ruff's `TRY401` (`verbose-log-message`, originating from `tryceratops`) is implemented across three files in `google3/third_party/ruff`:

1. **Rule Entrypoint (`crates/ruff_linter/src/rules/tryceratops/rules/verbose_log_message.rs` lines 53–103)**:
   - Invoked on every `Stmt::Try` handler slice (`handlers: &[ExceptHandler]`).
   - Collects logging calls inside each handler body via `LoggerCandidateVisitor` (which does not recurse into nested `ExceptHandler`s, since those are visited separately).
   - Filters for `LoggingLevel::Exception` (i.e. `.exception(...)` method calls or `logging.exception(...)` direct calls).
   - Walks **positional arguments only** (`expr.arguments.args`) using `NameVisitor`:
     ```rust
     impl<'a> Visitor<'a> for NameVisitor<'a> {
         fn visit_expr(&mut self, expr: &'a Expr) {
             match expr {
                 Expr::Name(name) if name.ctx.is_load() => self.names.push(name),
                 Expr::Attribute(_) => {}
                 _ => visitor::walk_expr(self, expr),
             }
         }
     }
     ```
   - For each collected `ExprName`, resolves its lexical binding via `checker.semantic().resolve_name(expr)` and reports `TRY401` if `binding.kind.is_bound_exception()`.

2. **Logger Candidate Detection (`crates/ruff_linter/src/rules/tryceratops/helpers.rs` lines 23–59 & `crates/ruff_python_semantic/src/analyze/logging.rs` lines 17–75)**:
   - **Direct function call (`Expr::Name`)**: Resolves qualified name; matches `["logging", "exception"]` (e.g., `from logging import exception; exception(...)`).
   - **Method call (`Expr::Attribute`)**: Calls `logging::is_logger_candidate`:
     1. **Inline instantiation**: `logging.getLogger(...).exception(...)` or `logging.Logger(...).exception(...)`.
     2. **Imported symbol (`semantic.resolve_qualified_name(value)` returns `Some`)**: Matches `["logging"]` (including `import logging as log; log.exception(...)`), `["flask", "current_app", "logger"]`, or any dotted path listed in `lint.logger-objects`. **Crucial nuance**: if `value` was imported from any other module (e.g., `from loguru import logger` $\to$ `["loguru", "logger"]`) and is *not* in `lint.logger-objects`, `is_logger_candidate` immediately returns `false` at `logging.rs` line 53!
     3. **Local variable or attribute chain (`semantic.resolve_qualified_name(value)` returns `None`)**: Extracts the tail identifier from `UnqualifiedName::from_expr(value)` and matches if `tail.starts_with("log") || tail.ends_with("logger") || tail.ends_with("logging") || tail.starts_with("LOG") || tail.ends_with("LOGGER") || tail.ends_with("LOGGING")`. This matches `logger = logging.getLogger(__name__)`, `self.logger`, `cls.log`, `app_logger`, `LOGGER`, etc.

---

### 1.2 Pattern-by-Pattern Behavioral Matrix

| Code Pattern inside `except ValueError as exc:` | The legacy linter `LoggerRedundantExceptionRule` | Ruff `TRY401` (`verbose-log-message`) | Ideal Behavior & Analysis |
| :--- | :--- | :--- | :--- |
| `logger.exception(f"Failed: {exc}")` (stdlib `logger = getLogger(...)`) | **Flagged** (at call line) | **Flagged** (at `exc` span) | **Flag** ✅ — `.exception()` already appends `ValueError: <exc>` and traceback. |
| `logger.exception(f"Failed: {exc}")` (`from loguru import logger`) | **Flagged** | **Not flagged by default**; **Flagged** when `lint.logger-objects = ["loguru.logger"]` | **Flag** ✅ — This is the exact reason the legacy linter had a custom rule: Ruff requires `logger-objects = ["loguru.logger"]` for imported non-stdlib loggers. |
| `logging.exception(f"Failed: {exc}")` | **Missed** ❌ (`_is_logger_exception_call` only checks `logger`/`log`) | **Flagged** ✅ | **Flag** ✅ — the legacy linter completely misses root `logging.exception(...)`. |
| `from logging import exception; exception(f"Failed: {exc}")` | **Missed** ❌ | **Flagged** ✅ | **Flag** ✅ — Handled by Ruff's `SemanticModel`. |
| `logger.exception("Failed: %s", exc)` | **Flagged** | **Flagged** | **Flag** ✅ — Printf-style positional argument. |
| `logger.exception("Failed: {}", exc)` | **Flagged** | **Flagged** | **Flag** ✅ — Loguru / `{}` positional argument. |
| `logger.exception(exc)` | **Flagged** | **Flagged** | **Flag** ✅ — Bare exception passed as first argument. |
| `logger.exception("Failed: %s", str(exc))` / `repr(exc)` | **Flagged** | **Flagged** (`NameVisitor` walks `Expr::Call` args) | **Flag** ✅ — `str(exc)` and `repr(exc)` duplicate the exception representation. |
| `logger.exception(f"Failed: {exc!s}")` / `f"{exc!r}"` | **Flagged** | **Flagged** | **Flag** ✅ — Conversion flags in f-strings are walked by `NameVisitor`. |
| `logger.exception("Failed: {}".format(exc))` | **Flagged** | **Flagged** (`NameVisitor` skips `.format` attr, walks call arg `exc`) | **Flag** ✅ — `.format(exc)` is caught by both. |
| `logger.exception("Failed: %s" % exc)` | **Flagged** | **Flagged** (`NameVisitor` walks `Expr::BinOp`) | **Flag** ✅ — `%` interpolation is caught by both. |
| `logger.exception(f"Status {exc.status_code} for {exc.url}")` | **Flagged** ❌ (false positive!) | **Not flagged** ✅ (`Expr::Attribute(_) => {}`) | **Pass** ✅ — Custom exception attributes (`status_code`, `errno`, `request_id`, `filename`) carry structured domain context that may not appear in `str(exc)`. |
| `logger.exception("Failed", extra={"code": exc.code})` or `logger.exception("Failed", code=exc.code)` | **Flagged** ❌ (false positive!) | **Not flagged** ✅ (keywords not walked; `Expr::Attribute` skipped) | **Pass** ✅ — Structured logging fields (`extra=...` or structlog/Loguru kwargs) attach machine-indexed metadata, not redundant message text. |
| `logger.exception("Failed", exc_info=exc)` | **Flagged** ❌ (false positive!) | **Not flagged** ✅ (keywords not walked) | **Pass** ✅ — `exc_info=exc` is a logging control keyword (or covered by `LOG007` if falsy), not message duplication. |
| `logger.exception(f"Failed: {exc.args}")` / `f"{exc.__traceback__}"` | **Flagged** | **Not flagged** (skipped by `Expr::Attribute(_) => {}`) | **Flag** (minor edge case) — `exc.args` and `exc.__traceback__` are built-in `BaseException` attributes already rendered by `.exception()`. |
| Outer `except ValueError as e1:` containing inner `try: ... except KeyError: logger.exception(f"{e1}")` | **Missed** ❌ (`break`s at inner `ExceptHandler`) | **Flagged** ✅ (`is_bound_exception()` tracks all active bindings) | **Flag** ✅ — `e1` is still a bound exception in scope. |
| Nested `def cb(exc): logger.exception(f"{exc}")` inside `except ValueError as exc:` | **Flagged** ❌ (false positive! No scope boundary check) | **Not flagged** ✅ (`resolve_name` resolves `exc` to parameter `cb(exc)`) | **Pass** ✅ — Inner function parameter `exc` shadows the handler's `exc` and is not inside the active `except` handler. |

---

## 2. Related Ecosystem Rules & How They Fit Together

| Rule | Tool / Origin | What It Checks | Relationship to `LoggerRedundantExceptionRule` |
| :--- | :--- | :--- | :--- |
| **`TRY401` (`verbose-log-message`)** ✅ | Ruff / `tryceratops` | Flags bound exception objects referenced in `logging.exception` / `logger.exception` positional arguments. | **Direct 1:1 equivalent** (and strictly more accurate than the legacy linter's AST walk). |
| **`TRY400` (`error-instead-of-exception`)** ✅ | Ruff / `tryceratops` | Flags `logging.error(...)` and `logger.error(...)` inside `except` blocks when `exc_info` is not set, suggesting `.exception(...)`. | **Predecessor step**: Ruff's unsafe autofix for `TRY400` rewrites `logger.error(f"Failed: {e}")` $\to$ `logger.exception(f"Failed: {e}")`, relying on `TRY401` to then flag `{e}`. |
| **[error_log_in_except.rs](../../../src/code_lint/rules/error_log_in_except.rs)** ✅ | Omni | Flags `logging.error(...)` (configurable `ListOption`, default `["logging.error"]`) inside `except` blocks, even when `exc_info=True` is passed. | **Omni's counterpart to `TRY400`**: Suggests replacing `logging.error(...)` with `logging.exception(...)`. Two of its test cases (`logging.error("invalid value: %s", err)`) pass `err` in the message. |
| **`LOG004` (`log-exception-outside-except-handler`)** ✅ | Ruff / `flake8-logging` | Flags `.exception()` calls *outside* of `except` handlers (where `sys.exc_info()` is `(None, None, None)` and logs `NoneType: None`). | Orthogonal complement: ensures `.exception()` is only called inside an `except` handler. |
| **`LOG007` (`exception-without-exc-info`)** ✅ | Ruff / `flake8-logging` | Flags `logging.exception("...", exc_info=False)` and suggests `logging.error("...")`. | Orthogonal complement: prevents disabling traceback capture on `.exception()`. |
| **`LOG014` (`exc-info-outside-except-handler`)** ✅ | Ruff / `flake8-logging` | Flags `logging.error("...", exc_info=True)` outside `except` handlers. | Orthogonal complement for `exc_info=True`. |
| **`G201` (`logging-exc-info`)** 📚 | Ruff / `flake8-logging-format` | Flags `logging.error("...", exc_info=True)` inside `except` handlers in favor of `logging.exception("...")`. | Overlaps with Omni's `error-log-in-except` test case `logging_error_with_exc_info_in_except`. |

---

## 3. Candid Assessment & Recommendation

### 3.1 Why Dropping (`D1`) Is the Strongest Engineering Choice

Under Omni's [Rule Design Guide](../rule_design_guide.md) (*Simplicity First: Minimum code that solves the problem. Nothing speculative*) and [Tag Guide §2.3](../tag_guide.md#L82) (*"Omni exists to go beyond the default Ruff and Clippy sets"*):

1. **Zero Conceptual Differentiation from Ruff `TRY401`**:
   - Unlike `signature_collection_types`, `mutable-module-constant`, or `repeated-index-access`—where Ruff has no equivalent rule—`TRY401` is a stable, built-in Ruff rule (`stable_since = "v0.0.250"`) that checks the exact same antipattern.
2. **The legacy linter's Only Motivation Was Missing Ruff Config (`lint.logger-objects`)**:
   - The legacy linter used `from loguru import logger`. Because Ruff's `is_logger_candidate` requires imported third-party loggers to be listed in `lint.logger-objects = ["loguru.logger"]`, `TRY401` did not fire in the legacy linter until configured. Adding `logger-objects = ["loguru.logger"]` in `ruff.toml` enables not only `TRY401`, but also Ruff's `TRY400`, `LOG004`, `LOG007`, `LOG014`, and `G*` rules for Loguru.
3. **Ruff Has Full Semantic Binding Resolution**:
   - Ruff's `SemanticModel` (`binding.kind.is_bound_exception()`) already tracks Python's scope rules, `except ... as exc:` unbinding at the end of the handler, variable shadowing, and nested `try`/`except` blocks.

### 3.2 When Implementing (`D2`–`D6`) Would Make Sense Instead

Implementing `redundant-exception-in-log` in Omni is warranted **only if** one of the following holds:
1. **Self-Contained Pairing with [error_log_in_except.rs](../../../src/code_lint/rules/error_log_in_except.rs)**: Since Omni already ships `error-log-in-except` (telling users to replace `logging.error("invalid value: %s", err)` with `logging.exception(...)`), shipping `redundant-exception-in-log` ensures Omni itself catches the resulting `logging.exception("invalid value: %s", err)` without depending on Ruff `TRY401` being enabled alongside Omni.
2. **Zero-Config Any-Receiver `$OBJ.exception` Matching + `exc.args` / `exc.__traceback__`**: Using Omni's `calls::find_banned_calls` with `base: &["logging.exception", "$OBJ.exception"]` works out-of-the-box for `from loguru import logger`, `structlog`, and custom logger wrappers without needing import resolution or `logger-objects` configuration, and can additionally flag `exc.args` and `exc.__traceback__`.

---

## 4. Final Decision Record (`VALIDATED — DROPPED`) & Recommended Configuration (`pyproject.toml`)

**Final Decision**: **Drop `LoggerRedundantExceptionRule` (`D1`)** and rely on Ruff `TRY401` (`verbose-log-message`).

### 4.1 Standard Library `logging` Configuration
```toml
[tool.ruff.lint]
extend-select = [
    "TRY400", # error-instead-of-exception: flags `logging.error(...)` / `logger.error(...)` in `except` blocks
    "TRY401", # verbose-log-message: flags redundant exception object in `logging.exception(...)` / `logger.exception(...)`
    "G201",   # logging-exc-info: flags `logging.error(..., exc_info=True)` in favor of `logging.exception(...)`
    "LOG004", # log-exception-outside-except-handler: flags `.exception()` outside `except`
    "LOG007", # exception-without-exc-info: flags `.exception(..., exc_info=False)`
    "LOG014", # exc-info-outside-except-handler: flags `exc_info=True` outside `except`
]
```

### 4.2 Loguru (`from loguru import logger`) Configuration
When using `from loguru import logger`, add `"loguru.logger"` to `logger-objects` so Ruff's `TRY400`, `TRY401`, and `LOG*` rules recognize imported `logger` calls (and ignore `PLE1205` if `PLE` is enabled, since `PLE1205` only counts `%` specifiers and not `{}`):
```toml
[tool.ruff.lint]
extend-select = ["TRY400", "TRY401", "LOG004", "LOG007"]
extend-ignore = ["PLE1205"] # Needed if `PLE` is selected, as PLE1205 assumes `%`-style rather than Loguru `{}` formatting
logger-objects = ["loguru.logger"]
```

---

## 5. References

- **The legacy linter `LoggerRedundantExceptionRule`**: [legacy_lints.py:L2322-2365](../../../scratch/legacy_reference/legacy_lints.py#L2322-L2365)
- **Omni `error-log-in-except`**: [error_log_in_except.rs](../../../src/code_lint/rules/error_log_in_except.rs)
- **Ruff `TRY401` (`verbose-log-message`) implementation**: `google3/third_party/ruff/crates/ruff_linter/src/rules/tryceratops/rules/verbose_log_message.rs`
- **Ruff `TRY400` (`error-instead-of-exception`) implementation**: `google3/third_party/ruff/crates/ruff_linter/src/rules/tryceratops/rules/error_instead_of_exception.rs`
- **Ruff `tryceratops` logger visitor**: `google3/third_party/ruff/crates/ruff_linter/src/rules/tryceratops/helpers.rs`
- **Ruff `is_logger_candidate` semantic analyzer**: `google3/third_party/ruff/crates/ruff_python_semantic/src/analyze/logging.rs`
- **Ruff `TRY401` official docs**: https://docs.astral.sh/ruff/rules/verbose-log-message/
- **Ruff `lint.logger-objects` setting**: https://docs.astral.sh/ruff/settings/#lint_logger-objects
