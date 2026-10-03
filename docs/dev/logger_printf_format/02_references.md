# Phase 2: Gather Resources and References — `LoggerPrintfFormatRule` (`logger-printf-format`)

This document records **Phase 2 (Gather Resources and References)** for `LoggerPrintfFormatRule`. It builds on [01_understand.md](01_understand.md) (`D1`–`D7`, `Q1`–`Q4`).

> Status: **VALIDATED — DROPPED (Option a)** (2026-10-03). Final decision: **Option (a) — DROP** (`LoggerPrintfFormatRule` is not ported to Omni; see [01_understand.md](01_understand.md) and §5 below).

Confidence markers: ✅ verified against official docs/source this session · ⚠️ synthesized from tool behavior/ecosystem discussions.

---

## 1. External State of the Art (`Q1`)

### 1.1 Python Standard Library `logging` vs. `loguru` vs. `structlog`

1. **Python Standard Library `logging` (`Lib/logging/__init__.py`)** ✅
   - **Runtime mechanism**: `LogRecord.getMessage()` formats positional arguments strictly with the `%` operator:
     ```python
     def getMessage(self):
         msg = str(self.msg)
         if self.args:
             msg = msg % self.args
         return msg
     ```
   - **Crucial nuance (`Formatter(style='{')` vs. `LogRecord.getMessage()`)**: Passing `style='{'` to `logging.Formatter` or `logging.basicConfig` only changes how the *Formatter* renders `LogRecord` attributes (`"{levelname}:{name}:{message}"`); it does **not** change `LogRecord.getMessage()`. Calling `logger.info("User {}", user)` on a standard `logging.Logger` raises `TypeError: not all arguments converted during string formatting` at runtime unless the message or logger is wrapped in a custom `BraceMessage` / `LoggerAdapter` class (documented in the Python Logging Cookbook).
   - **Consequence**: In standard Python `logging`, `logger.info("User %s", user)` is the **only** built-in lazy positional formatting syntax.

2. **Loguru (`loguru/_logger.py`)** ✅
   - **Runtime mechanism**: In `Logger._log`, when `args` or `kwargs` are non-empty, Loguru formats the message using `str.format`:
     ```python
     if args or kwargs:
         record["message"] = message.format(*args, **kwargs)
     ```
   - **Silent data-loss behavior on `%s`**: Unlike `%` formatting (which raises `TypeError: not all arguments converted during string formatting` when extra positional arguments are left over), Python's `str.format(*args, **kwargs)` **silently ignores unused positional arguments**:
     ```python
     "User %s logged in".format("alice")  # => "User %s logged in" (no exception raised!)
     ```
   - **Consequence**: Calling `logger.info("User %s logged in", user)` on a `loguru` logger never raises an error in tests or production—it silently discards `user` and emits literal `"User %s logged in"`.
   - **Import & call patterns**: Loguru exports a single global `logger` (`from loguru import logger` or `import loguru`). Context binding and options use method chaining on `logger` before the level call: `logger.bind(request_id=req_id).info(...)`, `logger.opt(depth=1, exception=True).error(...)`, `logger.patch(...).debug(...)`, and `logger.contextualize(...)`.

3. **`structlog`** ✅
   - **Runtime mechanism**: Encourages structured key-value pairs (`logger.info("user_logged_in", user=user)`). When stdlib positional arguments are used, `structlog.stdlib.PositionalArgumentsFormatter` formats them with `event % positional_args` (`%`-style, matching stdlib `logging`).

---

### 1.2 Ruff, Flake8, and Pylint Logging Rules

1. **Ruff `flake8-logging-format` (`G001`–`G004`)** ✅
   - `G001` (`logging-string-format`): flags `logging.info("User {}".format(user))`.
   - `G002` (`logging-percent-format`): flags `logging.info("User %s" % user)` (eager `%` interpolation).
   - `G003` (`logging-plus-format`): flags `logging.info("User " + user)`.
   - `G004` (`logging-f-string`): flags `logging.info(f"User {user}")`.
   - **Target**: Exclusively enforces stdlib `logging`'s lazy `%s` positional arguments (`logging.info("User %s", user)`).
   - **Logger candidate heuristic (`is_logger_candidate`)**: Matches `logging.*`, `flask.current_app.logger`, locally assigned variables whose name starts with `log` or ends with `logger`/`logging` (`logger = logging.getLogger(__name__)`), and qualified paths in `lint.logger-objects`. Because `from loguru import logger` is an import (not a local assignment), Ruff does **not** treat it as a logger candidate unless `lint.logger-objects = ["loguru.logger"]` is configured.

2. **Ruff `pylint` Logging Rules (`PLE1205`, `PLE1206`, `PLW1201`–`PLW1203`)** ✅
   - `PLE1205` (`logging-too-many-args`): counts `%` conversion specifiers in the format string and flags calls passing more positional arguments than `%` specifiers.
   - `PLE1206` (`logging-too-few-args`): flags calls passing fewer positional arguments than `%` specifiers.
   - **Why Ruff breaks if `lint.logger-objects = ["loguru.logger"]` is set**: Ruff hardcoded `PLE1205`/`PLE1206` to parse `%` specifiers only (it did **not** port Pylint's `logging-format-style` option). Therefore, adding `"loguru.logger"` to `lint.logger-objects` causes Ruff `PLE1205` to flag **every valid Loguru call** `logger.info("User {}", user)` as `logging-too-many-args` (0 `%` specifiers vs. 1 positional argument).

3. **Pylint `logging-format-style` (`old` vs. `new`)** ✅
   - Upstream Pylint provides `logging-format-style = "old"` (`%`, default) and `logging-format-style = "new"` (`{}`).
   - When set to `"new"` together with `logging-modules = ["loguru"]`, Pylint's `E1205` (`logging-too-many-args`) flags `logger.info("User %s", user)` because `"User %s"` contains zero `{}` fields and receives 1 argument. However, Pylint is slow, rarely enabled alongside Ruff, and requires manual repository configuration.

4. **Ruff `pyupgrade` `UP031` (`printf-string-formatting`)** ✅
   - Flags `printf`-style `%` modulo string formatting (`"..." % ...`) on **every** string literal in Python, regardless of whether it appears inside a logger call (`logger.info("User %s" % user)`) or general code.
   - **Implication for the legacy linter's Branch B**: the legacy linter's second branch (`elif isinstance(first_arg, ast.BinOp) and isinstance(first_arg.op, ast.Mod)`) is 100% redundant with Ruff `UP031` (and `G002`).

---

### 1.3 Comparison Table (`R1`–`R7`)

| ID | Reference | Key Ideas | Adopt / Adapt / Reject | Why |
| :--- | :--- | :--- | :--- | :--- |
| **R1** | **Ruff `G001`–`G004` (`flake8-logging-format`)** ✅ | Enforces `%s` lazy positional args on stdlib `logging`; bans `.format()`, `%` operator, `+`, and f-strings in stdlib logging. | **Adopt** as boundary constraint: Omni must **never** flag `logger.info("User %s", user)` on stdlib `logging`.<br>**Reject** the legacy linter's unscoped `_is_logger_call`. | Flagging `%s` on stdlib `logging` directly contradicts Ruff `G001`–`G004` and causes runtime `TypeError`s in stdlib `logging`. |
| **R2** | **Ruff `UP031` (`printf-string-formatting`) & `G002` (`logging-percent-format`)** ✅ | Flags `"..." % ...` (`ast.BinOp` with `ast.Mod`) across all Python code (`UP031`) and in logger calls (`G002`). | **Adopt** **D2**: Drop Branch B (`"..." % ...`) from Omni. | 100% covered by Ruff `UP031` and `G002`, and fails [rule_design_guide.md §1](../rule_design_guide.md) (The Split Test). |
| **R3** | **Ruff `PLE1205` / `PLE1206` & Pylint `E1205` (`logging-too-many-args`)** ✅ | Validates that positional arguments passed to a logger match the format string's placeholders; Pylint supports `logging-format-style = "new"` (`{}`), which Ruff lacks. | **Adapt** if Option (b) or (c) is chosen: scope strictly to `loguru` imports where `{}` formatting is guaranteed by the library. | Fills the exact gap left by Ruff not supporting `loguru` / `logging-format-style = "new"`, without requiring user config in `.omnilint.toml` because `from loguru import logger` is explicit in the file AST. |
| **R4** | **Loguru (`Delgan/loguru`) `Logger._log`** ✅ | Formats messages via `message.format(*args, **kwargs)` only when `args` or `kwargs` are non-empty; exports global `logger` (`trace`, `debug`, `info`, `success`, `warning`, `error`, `critical`, `exception`, `log`) and chainable `bind`/`opt`/`patch`. | **Adopt** in **D4** and **D6a**: match `loguru` imports + chained `bind`/`opt`/`patch` calls, and require non-empty format arguments (`args` or `kwargs`). | Mirrors Loguru's exact runtime execution (`if args or kwargs: message.format(*args, **kwargs)`). |
| **R5** | **Python Library Reference: `printf`-style String Formatting** ✅ | Conversion specifier grammar: `%[(key)][flags][width][.precision][length]type`. Escaped literal percent is `%%`. | **Adapt** in **D5**: parse `%%` first and exclude the space flag `' '` from unmapped specifiers. | Prevents `"100% complete"` (`% c`), `"50% discount"` (`% d`), and `"100%%s"` from being misidentified as printf specifiers. |
| **R6** | **`structlog` (`PositionalArgumentsFormatter`)** ✅ | Uses `%` formatting for positional arguments (`event % positional_args`), not `{}`. | **Adopt**: do **not** include `structlog` in a brace-formatting rule. | `structlog` positional arguments use `%s`, not `{}`. |
| **R7** | **Rust `rustc` `format_args!` (`log::info!`, `tracing::info!`)** ✅ | `log` and `tracing` macros expand to `format_args!`, checked at compile time by `rustc`. | **Adopt**: Python-only rule (`languages: &[SupportLang::Python]`). | See §2 (`Q2`): `rustc` already rejects unused positional arguments and invalid format specifiers at compile time. |

---

## 2. Cross-Language Comparison: Rust `log` and `tracing` (`Q2`)

Does Rust need a rule for `printf`-style `%s` in `log::info!` or `tracing::info!`? **No.**

1. **`log::info!("User %s", user)` and `tracing::info!("User %s", user)` fail to compile in `rustc`**:
   - Both `log` (`info!`, `warn!`, `error!`, `debug!`, `trace!`) and `tracing` pass their format string and positional arguments to `core::format_args!`.
   - When `user` is passed as a positional argument (`info!("User %s", user)`), `rustc` emits a hard compile error: **`error: argument never used`**.
2. **Single-argument `info!("User %s")` without arguments**:
   - In Rust, if no arguments are passed, `"User %s"` is a static message string. Unlike Python, Rust developers do not migrate from a stdlib `%s` logger because Rust has used `{}` (`std::fmt`) since Rust 1.0.
3. **Conclusion**: If ported under Option (b) or (c), this rule is strictly Python-only (`languages: &[SupportLang::Python]`), just like [error_log_in_except.rs](../../../src/code_lint/rules/error_log_in_except.rs).

---

## 3. Internal Codebase Architecture & AST Analysis (`Q3`)

### 3.1 Why Unscoped `calls::find_banned_calls` Cannot Be Used Alone
In [calls.rs](../../../src/code_lint/semantic/calls.rs), `calls::find_banned_calls` matches call sites syntactically against `banned: &HashSet<String>`:
- In [error_log_in_except.rs:19](../../../src/code_lint/rules/error_log_in_except.rs#L19), `BANNED` defaults to `&["logging.error"]` (and intentionally omits `"logger.error"`, see lines 50–52 and pass case `logger_instance_error_not_in_banned_calls` at lines 139–147).
- For `loguru`, however, module-level `loguru.info(...)` **does not exist** in Loguru's API—Loguru calls are always method calls on the `logger` object (`from loguru import logger; logger.info(...)` or `import loguru; loguru.logger.info(...)`).
- If `BANNED` defaulted to `&["logger.info", "logger.debug", ...]`, `calls::find_banned_calls` would match every stdlib `logging.getLogger(__name__)` instance named `logger`!
- Conversely, if `BANNED` defaulted only to `&["loguru.logger.info", ...]`, it would miss 99% of real-world Loguru code (`from loguru import logger; logger.info(...)`) unless import resolution maps `logger` $\to$ `loguru.logger`.

### 3.2 Clean Architectural Solution (If Option (b) or (c) Is Chosen)
If we implement the rule in Omni, we have a clean way to resolve `loguru` calls in `src/code_lint/ast/python.rs` (which also directly advances the `ROADMAP.md` item *"Import-Aware Qualified Call Resolution"*):

1. **Per-File Import-Aware Callee Resolution**:
   - Collect local import aliases in the Python file (`from loguru import logger` $\to$ `logger` maps to `loguru.logger`; `from loguru import logger as log` $\to$ `log` maps to `loguru.logger`; `import loguru as lg` $\to$ `lg.logger` maps to `loguru.logger`).
   - Normalize chained Loguru builder methods (`<logger>.bind(...)`, `<logger>.opt(...)`, `<logger>.patch(...)`) so `logger.bind(user=u).info("Msg %s", x)` resolves its base receiver to `loguru.logger.info`.
   - Match against `BANNED: ListOption` with default:
     ```rust
     base: &[
         "loguru.logger.trace",
         "loguru.logger.debug",
         "loguru.logger.info",
         "loguru.logger.success",
         "loguru.logger.warning",
         "loguru.logger.error",
         "loguru.logger.critical",
         "loguru.logger.exception",
         "loguru.logger.log",
     ]
     ```
   - **Why this is ideal**:
     - Out of the box, any file with `from loguru import logger` (or `import loguru`) resolves `logger.info(...)` $\to$ `"loguru.logger.info"`, which matches `BANNED`!
     - Any file using `import logging; logger = logging.getLogger(__name__)` (or no `loguru` import) leaves `logger.info` as `"logger.info"`, which is **not** in `BANNED`—guaranteeing **zero false positives** on stdlib `logging`!
     - If a project wraps Loguru in a custom internal module (`from mypkg.logging import app_logger`) without importing `loguru` in every file, the team can simply add `extend-banned = ["app_logger.info", ...]` in `.omnilint.toml`!

### 3.3 Exact Printf-in-Loguru Check (`D5` + `D6a`)
For each matched call:
1. Determine the message argument index:
   - `1` if the method name is `"log"` (`logger.log(level, message, *args, **kwargs)`),
   - `0` for all other logging methods (`logger.info(message, *args, **kwargs)`).
2. Extract the positional `message` argument at that index:
   - Must be a non-f-string Python `string` literal (f-strings are eager and already formatted by Python).
3. Check for format arguments:
   - Let `has_pos_format_args` be true if there is at least one positional argument after `message` (or a `*args` `list_splat` argument).
   - Let `has_kw_format_args` be true if there are keyword arguments (or `**kwargs` `dictionary_splat`) other than `loguru`'s reserved keyword options (or when `%(key)s` mapping specifiers are used).
   - Under **D6a**, require `has_pos_format_args || (has_printf_mapping_key && has_kw_format_args)`.
4. Check the string content of `message`:
   - Strip `%%` (escaped percent).
   - Check if `message` contains a printf conversion specifier:
     - Mapped: `%(key)[-+0 #]*(?:\d+|\*)?(?:\.(?:\d+|\*))?[diouxXeEfFgGcrsab]`
     - Unmapped: `%[-+0#]*(?:\d+|\*)?(?:\.(?:\d+|\*))?[diouxXeEfFgGcrsab]` (**no space `' '`** in flags!)
   - Check that `message` does not use `{}` placeholders that consume the arguments (avoiding false positives if a string logs both a `{}` replacement field and a literal `%s` token, such as `logger.info("Translated %s to {}", target)`).

---

## 4. Relationship with Sibling the legacy linter Logging Rules (`Q4`)

Looking at all three the legacy linter logging rules in [legacy_lints.py:2276-2412](../../../scratch/legacy_reference/legacy_lints.py#L2276-L2412) together:

| The legacy linter Rule | Lines | What It Checks | Ecosystem / Ruff Overlap |
| :--- | :--- | :--- | :--- |
| `LoggerPrintfFormatRule` | 2276–2320 | 1. Printf `%s` in `logger.<level>(...)`<br>2. `"..." % ...` in `logger.<level>(...)` | Branch 1 is a real silent bug in `loguru` (`"User %s".format(user)` drops `user`), but **mandatory** in stdlib `logging`.<br>Branch 2 is 100% covered by Ruff `UP031` and `G002`. |
| `LoggerRedundantExceptionRule` | 2322–2365 | Passing `e` to `logger.exception(f"... {e}")` or `logger.exception("...", e)` inside `except ... as e:` | Covered in stdlib `logging` by **Ruff `TRY401` (`verbose-log-message`)** (`flake8-tryceratops`), which flags logging the exception object inside `logging.exception(...)`. |
| `LoggerPositionalPlaceholderRule` | 2367–2412 | Using named `{user}` placeholders instead of positional `{}` in `logger.info("User {user}", user=u)` | Purely stylistic preference within `loguru` (Loguru officially supports both `logger.info("If {x}", x=1)` and `logger.info("If {}", 1)`). |

Notice the clear pattern:
- The legacy linter had three custom `Logger*` rules because Ruff's `G` (`flake8-logging-format`), `LOG` (`flake8-logging`), `PLE1205`/`PLE1206` (`pylint`), and `TRY401` (`tryceratops`) rules only recognize stdlib `logging` (unless `lint.logger-objects` is configured, which breaks `PLE1205` on Loguru's `{}` syntax).
- Of the three, `LoggerPrintfFormatRule` (Branch A: passing `%s` with format arguments to `loguru.logger`) is the **only** one that catches a **silent runtime data-loss bug** that no Ruff rule can catch without breaking on `{}`.

---

## 5. Final Decision Record: **VALIDATED — DROPPED (Option a)**

**Selected Path: Option (a) — DROP** (`LoggerPrintfFormatRule` is not ported to Omni now; tracked in [ROADMAP.md](../../../ROADMAP.md) under **Dependency-Aware Rule Activation (`pyproject.toml` / `Cargo.toml`) & Loguru Format Enforcement (`LoggerPrintfFormatRule`)**).

1. **Branch B (`logger.info("User %s" % user)`)** is 100% covered by Ruff `G002` (`logging-percent-format`) and `UP031` (`printf-string-formatting`).
2. **Branch A (`logger.info("User %s", user)`)** is the required stdlib `logging` idiom (enforced by Ruff `G001`–`G004` and Pylint `W1201`–`W1203`) and is supported by Omni's `quote-wrapped-placeholder` rule (`logger.error("Bad '%s'", v)` $\to$ `%r`). Even when gated on `import loguru`, file-local import detection fails when projects wrap or re-export their logger from an internal module (`from app.logging import logger`), while Omni's `unmatched-logger-placeholder` rule already catches `{name}` + positional argument mismatches across both stdlib `logging` and `loguru`.

### 5.1 Current Linter Alternatives & Configuration (`pyproject.toml`)

- **To ban `"..." % ...` modulo string formatting (the legacy linter Branch B) and eager `.format()` / `+` / f-strings in stdlib `logging` via Ruff**:
  ```toml
  [tool.ruff.lint]
  extend-select = [
      "UP031", # printf-string-formatting: flags `"User %s" % user` everywhere
      "G001",  # logging-string-format: flags `logger.info("...".format(...))`
      "G002",  # logging-percent-format: flags `logger.info("..." % ...)`
      "G003",  # logging-plus-format: flags `logger.info("..." + ...)`
      "G004",  # logging-f-string: flags `logger.info(f"...")`
  ]
  ```
- **To catch `logger.info("User %s", user)` on Loguru (the legacy linter Branch A) today via Pylint** (until Omni adds dependency-aware `loguru` rule activation):
  ```toml
  [tool.pylint."messages control"]
  enable = ["logging-too-many-args"] # E1205: flags `"User %s"` with positional args when format style is `new` (`{}`)

  [tool.pylint.logging]
  logging-format-style = "new" # `{}` str.format style used by Loguru
  logging-modules = ["loguru"]
  ```

*(Historical note: `D3`–`D7` in [01_understand.md](01_understand.md) and §3.2–§3.3 above document the `loguru`-scoped design evaluated under Option (b) before Option (a) was selected.)*

---

## 6. Sources
- Python Standard Library `logging` (`LogRecord.getMessage` & Logging Cookbook): https://docs.python.org/3/library/logging.html#logging.LogRecord.getMessage · https://docs.python.org/3/howto/logging-cookbook.html#using-particular-formatting-styles-throughout-your-application
- Python Library Reference (`printf`-style String Formatting): https://docs.python.org/3/library/stdtypes.html#printf-style-string-formatting
- Loguru documentation & `Logger._log` implementation: https://loguru.readthedocs.io/en/stable/api/logger.html
- Ruff `flake8-logging-format` (`G001`–`G004`): https://docs.astral.sh/ruff/rules/#flake8-logging-format-g
- Ruff `UP031` (`printf-string-formatting`): https://docs.astral.sh/ruff/rules/printf-string-formatting/
- Ruff `PLE1205` (`logging-too-many-args`): https://docs.astral.sh/ruff/rules/logging-too-many-args/
- Ruff `TRY401` (`verbose-log-message`): https://docs.astral.sh/ruff/rules/verbose-log-message/
- Pylint `logging-format-style`: https://pylint.readthedocs.io/en/stable/user_guide/configuration/all-options.html#logging-format-style
- `structlog` `PositionalArgumentsFormatter`: https://www.structlog.org/en/stable/api.html#structlog.stdlib.PositionalArgumentsFormatter
