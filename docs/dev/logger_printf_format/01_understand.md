# Phase 1: Understand — `LoggerPrintfFormatRule` (`logger-printf-format`)

This document records **Phase 1 (Understand)** for the legacy candidate rule `LoggerPrintfFormatRule` ([legacy_lints.py:2276-2320](../../../scratch/legacy_reference/legacy_lints.py#L2276-L2320)).

> Status: **VALIDATED — DROPPED (Option a)** (2026-10-03). `LoggerPrintfFormatRule` is not ported to Omni:
> 1. **Branch B (`logger.info("User %s" % user)`)** is 100% covered by Ruff `G002` (`logging-percent-format`) and `UP031` (`printf-string-formatting`).
> 2. **Branch A (`logger.info("User %s", user)`)** is the required stdlib `logging` idiom (enforced by Ruff `G001`–`G004` and Pylint `W1201`–`W1203`) and is supported by Omni's `quote-wrapped-placeholder` rule (`logger.error("Bad '%s'", v)` $\to$ `%r`). Even when gated on `import loguru`, file-local import detection fails when projects wrap or re-export their logger from an internal module (`from app.logging import logger`), while Omni's `unmatched-logger-placeholder` rule already catches `{name}` + positional argument mismatches across both stdlib `logging` and `loguru`.

---

## 1. Problem Statement & Why the legacy linter Had `LoggerPrintfFormatRule`

### 1.1 Origin in the legacy linter
In [legacy_lints.py:2276-2320](../../../scratch/legacy_reference/legacy_lints.py#L2276-L2320), the legacy linter defined three consecutive logging rules tailored to a single-repository application that standardized exclusively on **[Loguru](https://github.com/Delgan/loguru)** (`from loguru import logger`):
1. `LoggerPrintfFormatRule` (lines 2276–2320): flags printf specifiers (`"%s"`, `"%d"`) and `%` modulo interpolation (`"..." % ...`) in `logger.<method>(...)` calls.
2. `LoggerRedundantExceptionRule` (lines 2322–2365): flags redundant exception variables in `logger.exception(...)`, explicitly stating `"Loguru automatically captures the traceback."` (line 2353).
3. `LoggerPositionalPlaceholderRule` (lines 2367–2412): flags named `{field}` placeholders in `logger.<method>(...)` calls.

Evidence within `LoggerPrintfFormatRule` itself confirms its Loguru-specific design:
- `_LOGGER_METHODS` (lines 2280–2290) includes `"trace"` and `"success"`, which are **Loguru-exclusive** log levels (`logging.Logger` in the Python standard library only provides `debug`, `info`, `warning`, `error`, `critical`, `exception`, and `log`).
- Line 2310 explicitly states: `"Logger call uses '%' string interpolation. Loguru uses '{}' formatting."`
- Line 2305 states: `"Logger message contains '%s' or printf specifiers. Use '{}' formatting."`

### 1.2 The Actual Bug in `loguru` Codebases
Why did developers and AI coding assistants keep writing `logger.info("User %s logged in", user)` in the legacy linter?
1. **Ecosystem & LLM training bias**: Python's standard library `logging` module uses printf-style `%s` lazy formatting by default, and Ruff's `flake8-logging-format` (`G001`–`G004`) actively enforces `%s` in stdlib `logging`. Developers and LLMs trained on standard Python write `logger.info("User %s", user)` out of muscle memory even when the file starts with `from loguru import logger`.
2. **Silent runtime data loss in `loguru`**:
   - When positional or keyword arguments are passed to `loguru.logger.<level>(message, *args, **kwargs)`, Loguru formats the record via `message.format(*args, **kwargs)` (`str.format`).
   - Crucially, Python's `str.format()` **does not raise `TypeError` or `IndexError` when extra positional arguments are unused** (`"User %s logged in".format("alice")` evaluates to `"User %s logged in"` without error).
   - Consequently, `logger.info("User %s logged in", user)` in Loguru **silently discards `user`** and writes the literal text `"User %s logged in"` to logs in production.
3. **Why Ruff did not catch it in the legacy linter**:
   - Ruff's `G001`–`G004` (`flake8-logging-format`) and `PLE1205`/`PLE1206` (`logging-too-many-args` / `logging-too-few-args`) are hardcoded for stdlib `logging`'s `%` formatting.
   - By default, Ruff does not treat `from loguru import logger` as a stdlib logger. If a user adds `"loguru.logger"` to Ruff's `lint.logger-objects`, Ruff's `PLE1205` flags **every valid Loguru call** `logger.info("User {}", user)` as `Too many positional arguments for logging format string` (because `"User {}"` has zero `%` specifiers and one positional argument).

---

## 2. The Ecosystem Conflict: Stdlib `logging` vs. `loguru` vs. `structlog`

Flagging `logger.info("User %s", user)` globally across Python files creates an irreconcilable conflict with Python's standard library:

| Logging Library | Idiomatic Lazy Syntax | What Happens on `logger.info("User %s", user)` | What Happens on `logger.info("User {}", user)` |
| :--- | :--- | :--- | :--- |
| **Python stdlib `logging`** (`import logging; logger = logging.getLogger(__name__)`) | `logger.info("User %s", user)` | **100% Correct & Required** (`LogRecord.getMessage()` runs `msg % self.args`). Enforced by Ruff `G001`–`G004`, `PLE1205`–`PLE1206`. | **Runtime Bug**: raises `TypeError: not all arguments converted during string formatting` inside `LogRecord.getMessage()`. |
| **`structlog`** (`structlog.get_logger()`) | `logger.info("event", user=user)` or `logger.info("User %s", user)` (via `PositionalArgumentsFormatter`) | **Valid** when `PositionalArgumentsFormatter` (`event % args`) is configured. | **Broken**: positional `{}` arguments are not formatted by `PositionalArgumentsFormatter`. |
| **`loguru`** (`from loguru import logger`) | `logger.info("User {}", user)` | **Silent Runtime Bug**: `"User %s".format(user)` silently drops `user` and outputs literal `"User %s"`. | **100% Correct & Required** (`message.format(*args, **kwargs)`). |

> [!IMPORTANT]
> Even within **Omni's own codebase**, standard library `logging` with `%s` is the documented canonical Python pattern:
> - [suppressed_exception.rs:78](../../../src/code_lint/rules/suppressed_exception.rs#L78) (`RuleDoc.examples` `fixed` code):
>   `logger.debug("Lock file %s was already removed by the cleanup job.", lock_path)`
> - [error_log_in_except.rs:166](../../../src/code_lint/rules/error_log_in_except.rs#L166):
>   `logging.error("invalid value: %s", err)`
>
> An unscoped rule matching `logger.<method>("%s", ...)` would immediately flag standard Python code—and would even contradict Omni's own `suppressed-exception` documented fix!

---

## 3. Five Defects in the legacy linter's `LoggerPrintfFormatRule`

Inspecting [legacy_lints.py:2276-2320](../../../scratch/legacy_reference/legacy_lints.py#L2276-L2320) reveals five distinct design and implementation defects:

1. **Defect 1 — Unconditional receiver heuristic (`logger` / `log`) without import awareness**:
   - `_is_logger_call` matches any call where the receiver is `logger`, `log`, `<expr>.logger`, or `<expr>.log`, regardless of whether the file imports `logging` or `loguru`.
   - In any project using stdlib `logging`, every `logger.info("User %s", user)` call is falsely flagged, and following the rule's suggestion (`Use '{}' formatting`) introduces a runtime `TypeError`.

2. **Defect 2 — Fuses two distinct antipatterns into one rule (violates [rule_design_guide.md §1](../rule_design_guide.md))**:
   - **Branch A (lines 2300–2306)**: `first_arg` is a `str` constant containing printf specifiers (`logger.info("User %s", user)`).
     - *Why it is bad (in Loguru)*: Loguru's `str.format` does not interpret `%s` and silently discards positional arguments, emitting literal `"%s"` (`ImpactedQuality::Reliability`).
     - *How to fix*: Replace `%s` specifiers with `{}` placeholders in the format string.
   - **Branch B (lines 2307–2311)**: `first_arg` is a `BinOp` with `ast.Mod` (`logger.info("User %s" % user)`).
     - *Why it is bad*: Evaluates string interpolation eagerly before the logger's level check and uses legacy `%` string formatting.
     - *Overlap*: **100% covered by Ruff `UP031` (`printf-string-formatting`)** (which flags `"..." % ...` on every string literal in Python, regardless of logger detection) and **Ruff `G002` (`logging-percent-format`)** / **Pylint `PLW1201` (`logging-not-lazy`)**.
   - Notice that the legacy linter itself emitted **two different messages** on lines 2305 and 2310 because the two branches are separate antipatterns.

3. **Defect 3 — Catastrophic false-positive regex bug on ordinary English percentages (`% ` space flag)**:
   - Line 2301 uses:
     ```python
     pattern = r"%[-+0 #]*(?:\d+|\*)?(?:\.(?:\d+|\*))?[diouxXeEfFgGcrs]"
     ```
   - Because the literal space character `' '` is included inside the printf conversion flag character class `[-+0 #]*`, any `%` followed by a space and a word starting with **`c, d, e, f, g, i, o, r, s, u, x`** (11 common English initial letters!) matches the regex as `%` + space flag + conversion specifier!
   - Furthermore, the regex does not strip escaped `%%` first.
   - As a result, the legacy linter falsely flags ordinary English log messages with no printf intent whatsoever:
     - `logger.info("Task 100% complete")` → matches `"% c"` (`%c` character specifier)
     - `logger.info("Applied 50% discount")` → matches `"% d"` (`%d` integer specifier)
     - `logger.info("Reached 99% success rate")` → matches `"% s"` (`%s` string specifier)
     - `logger.info("Processed 80% of batch")` → matches `"% o"` (`%o` octal specifier)
     - `logger.info("Observed 20% error rate")` → matches `"% e"` (`%e` float specifier)
     - `logger.info("Traffic 10% in region")` → matches `"% i"` (`%i` integer specifier)
     - `logger.info("Job ran 75% faster")` → matches `"% f"` (`%f` float specifier)
     - `logger.info("Latency 60% reduced")` → matches `"% r"` (`%r` repr specifier)
     - `logger.info("Memory at 40% utilization")` → matches `"% u"` (`%u` unsigned specifier)

4. **Defect 4 — Flags single-argument log calls where no format arguments exist**:
   - Line 2294 only checks `if not node.args: return`, so a 1-argument call `logger.info("100% complete")` or `logger.info("Config contains '%s' token")` is flagged even though no format arguments were passed (and Loguru does not even call `.format()` when `*args` and `**kwargs` are empty).

5. **Defect 5 — Broken argument index on `logger.log(level, message, *args)`**:
   - `_LOGGER_METHODS` includes `"log"` (line 2289), where the signature in both `logging` and `loguru` is `logger.log(level, message, *args)`.
   - However, line 2299 unconditionally checks `first_arg = node.args[0]` (which is `level`, e.g., `"INFO"` or `20`), so `logger.log("INFO", "User %s", user)` is never inspected.

---

## 4. Strategic Options Analysis: (a) vs. (b) vs. (c)

### Option (a): Drop `LoggerPrintfFormatRule` Entirely
Do not port `LoggerPrintfFormatRule` to Omni; document it under *Not pursued* in `ROADMAP.md`.

- **Pros**:
  1. **Zero conflict with stdlib `logging`**: Eliminates any risk of contradicting Python's standard `logging` module or Ruff's `G001`–`G004` / `PLE1205`–`PLE1206` rules.
  2. **Branch B (`"..." % ...`) is already solved**: Ruff `UP031` (`printf-string-formatting`) and `G002` (`logging-percent-format`) already catch eager `%` formatting in Python.
  3. **Avoids single-third-party-library creep**: Every existing rule in Omni targets language-level semantics or standard-library/core-ecosystem APIs (`logging`, `unittest.mock`, `typing`, `asyncio`, `tokio`). Dropping a Loguru-specific rule keeps Omni's rule catalog focused on universal software engineering antipatterns ([rule_design_guide.md §4](../rule_design_guide.md)).
- **Cons**:
  1. **Leaves a real, silent reliability bug unguarded in `loguru` codebases**: For teams using `loguru` (including the legacy linter, the origin of Omni's custom lint suite), `logger.info("User %s", user)` silently drops `user` and logs literal `"%s"`, and Ruff provides no rule or setting to catch it.

---

### Option (b): Scope Specifically to `loguru` Imports (Recommended if Ported)
Port **only Branch A** (printf specifiers in lazy format strings) as a Loguru-scoped rule (e.g. `printf-log-format` or `loguru-printf-format`), gated on `loguru` being imported in the file (or explicit `loguru.logger` callees), and drop Branch B (`"..." % ...` covered by `UP031`/`G002`).

- **How it avoids all stdlib `logging` false positives**:
  1. **Import-aware activation**: Because Loguru has a single global `logger` imported via `from loguru import logger` (or `import loguru`), a single-file AST check in `src/code_lint/ast/python.rs` can verify whether `loguru` is imported in the file and track the local binding name(s) (`logger`, aliased `from loguru import logger as log`, or `loguru.logger`), including chained calls (`logger.bind(...).info(...)`, `logger.opt(...).info(...)`).
  2. **Zero hits on stdlib `logging`**: Files using `import logging; logger = logging.getLogger(__name__)` (or bare `logger.debug("Lock file %s...", path)` without `loguru` imported, as in `suppressed_exception.rs`) are **never flagged**.
  3. **Fix all 4 technical bugs from the legacy linter**:
     - Strip `%%` before scanning for printf specifiers.
     - Exclude the space flag `' '` from unparenthesized specifiers (or require specifiers without spaces unless preceded by `%(key)` or followed by digits) so `"100% complete"` and `"50% discount"` never match.
     - Require at least one format argument (`*args` after the message argument, or `**kwargs` when `%(key)s` is present)—or evaluate whether 1-arg `%s` without spaces should also be flagged (see `D6` / `Q3`).
     - Inspect `args[1]` for `.log(level, msg, *args)` and `args[0]` for `.trace`, `.debug`, `.info`, `.success`, `.warning`, `.error`, `.critical`, `.exception`.

---

### Option (c): Generalize to "Unused Positional Arguments in `{}`-Formatted Logger Calls" (`loguru`)
Instead of regex-matching `%s`, check `loguru` logger calls that pass positional format arguments (`logger.info(msg, arg1, ...)`) where the string literal `msg` contains **no `{}` replacement fields** (or fewer `{}` fields than positional arguments), similar to Ruff `PLE1205` (`logging-too-many-args`) but for `{}`-style `loguru` loggers.

- **Pros**:
  1. **100% Exact (`Precision::Exact`, `Consensus::Unopinionated`, `ImpactedQuality::Reliability`)**: No regex heuristics needed! In Loguru, `message.format(*args, **kwargs)` is guaranteed to ignore positional `*args` if `message` contains no positional `{}` / `{0}` replacement fields (whether the developer wrote `"User %s", user`, `"User $1", user`, or `"User logged in", user` expecting `print`-style comma-separated joining!).
  2. **Catches a strict superset of real Loguru bugs**:
     - Catches `logger.info("User %s logged in", user)` (printf habit).
     - Also catches `logger.info("User logged in:", user)` (`print()` habit where developers pass comma-separated args without `{}` placeholders—which in Loguru silently drops `user`!).
- **Cons / Nuances**:
  1. Does not catch a 1-argument call `logger.info("User %s logged in")` where the developer forgot to pass `user` (though in practice, 1-argument `"%s"` without args is rare and prone to false positives on literal strings containing `%s`).
  2. If scoped to `printf` specifiers vs. any unused positional argument in `loguru`, the rule name and diagnostic message differ (`printf-log-format` vs. `unused-log-argument` / `unformatted-log-argument`).

---

## 5. Numbered Decisions & Proposals (`D1`–`D7`)

| ID | Decision Area | Proposed Position / Options for Confirmation | Rationale |
| :--- | :--- | :--- | :--- |
| **D1** | **Keep vs. Drop (`Option (a)` vs. `Option (b)` vs. `Option (c)`)** | **VALIDATED: Option (a) — DROP** (do not port to Omni; record under *Not pursued* in `ROADMAP.md`). | 1. **Branch B (`"..." % ...`)** is 100% covered by Ruff `G002` (`logging-percent-format`) and `UP031` (`printf-string-formatting`).<br>2. **Branch A (`"User %s", user`)** is the required stdlib `logging` idiom (enforced by Ruff `G001`–`G004` and Pylint `W1201`–`W1203`) and is supported by Omni's `quote-wrapped-placeholder` rule (`logger.error("Bad '%s'", v)` $\to$ `%r`).<br>3. File-local `import loguru` detection fails when projects wrap or re-export their logger from an internal module (`from app.logging import logger`), while Omni's `unmatched-logger-placeholder` rule already catches `{name}` + positional argument mismatches across both stdlib `logging` and `loguru`. |
| **D2** | **Drop Branch B (`"..." % ...` modulo interpolation)** | **Adopt unconditionally** (under all options). | 100% redundant with Ruff `UP031` (`printf-string-formatting`) and `G002` (`logging-percent-format`), and violates [rule_design_guide.md §1](../rule_design_guide.md) (The Split Test). |
| **D3** | **Rule Name (if implemented under Option (b))** | `printf-log-format` (file `src/code_lint/rules/printf_log_format.rs`, const `RULE`). | Follows [naming_and_message_style_guide.md §1](../naming_and_message_style_guide.md): kebab-case, $\le 4$ words, singular noun phrase naming the flagged pattern, no polarity prefix, avoids library name unless strictly necessary. |
| **D4** | **Receiver & Import Resolution (if implemented)** | Match logger calls only when:<br>1. The file imports `loguru` (`from loguru import logger [as alias]` or `import loguru [as alias]`) and the call receiver starts with that binding (`logger.info(...)`, `logger.bind(...).info(...)`, `logger.opt(...).info(...)`, `loguru.logger.info(...)`), **OR**<br>2. The call matches a user-configured entry in `ListOption` (`BANNED`, defaulting to `loguru.logger.<level>`). | Guarantees **0 false positives** on stdlib `logging` (`logger = logging.getLogger(__name__)`) while working out of the box on every file with `from loguru import logger` and allowing custom wrapper modules via `extend-banned`. |
| **D5** | **Printf Specifier Detection (fixing the legacy linter Defect 3)** | Replace the legacy linter's broken regex (`[-+0 #]*` with space) with a dedicated scanner in `src/code_lint/ast/python.rs` that:<br>1. Skips escaped `%%`.<br>2. Matches `%(name)[-+0#]*(?:\d+|\*)?(?:\.(?:\d+|\*))?[diouxXeEfFgGcrsab]` (mapping key) or `%[-+0#]*(?:\d+|\*)?(?:\.(?:\d+|\*))?[diouxXeEfFgGcrsab]` (**no space flag `' '`** in unmapped specifiers!). | Removing `' '` from unmapped flags eliminates 100% of the `"100% complete"` / `"50% discount"` / `"99% success"` false positives while matching every real printf specifier (`%s`, `%r`, `%d`, `%i`, `%f`, `%.2f`, `%04d`, `%x`, `%(user)s`). |
| **D6** | **Format Argument Requirement (fixing the legacy linter Defect 4)** | **Sub-option D6a (Recommended — Exact)**: Require at least one format argument to be passed (`args.len() >= 2` for standard methods, `args.len() >= 3` for `.log()`, or `**kwargs` for `%(key)s`), and no `{}` placeholders consuming it.<br>**Sub-option D6b (Heuristic)**: Flag any string literal with a non-space printf specifier (`%s`, `%d`, `%.2f`, `%(key)s`) in a `loguru` call even with 1 argument. | Under **D6a**, the rule has `Precision::Exact` and `Consensus::Unopinionated`: passing a positional argument to a `loguru` call with `%s` and no `{}` is provably a runtime bug where the argument is silently discarded. Under **D6b**, `logger.info("Literal %s")` (logging a string that legitimately contains `%s`, such as a cron/strftime/SQL pattern) would be a false positive (`Precision::Heuristic`). |
| **D7** | **Classification (if implemented under D6a)** | `topics: &[Topic::LOGGING]`, `precision: Precision::Exact` (`D6a`) or `Precision::Heuristic` (`D6b`), `consensus: Consensus::Unopinionated`, `impacted_quality: ImpactedQuality::Reliability`, `target: RuleTarget::All`. | Silently dropping logged variables and emitting literal `"%s"` in logs is a concrete runtime defect (`Reliability`). |

---

## 6. Questions for Phase 2 (`Q1`–`Q4`)

- **Q1 (Ruff, Pylint, Flake8 & Ecosystem SOTA)**: How do Ruff (`flake8-logging-format` `G`, `flake8-logging` `LOG`, `pylint` `PLE1205`/`PLE1206`/`PLW1201`–`PLW1203`, `pyupgrade` `UP031`), Pylint (`logging-format-style`), and `loguru` / `structlog` handle format-string linting, and why has Ruff not added Loguru rules?
- **Q2 (Cross-Language Comparison — Rust `log` / `tracing`)**: Does an analogous problem exist in Rust (`tracing::info!`, `log::info!`), or does `rustc`'s compile-time `format_args!` checking already reject `%s` and unused format arguments?
- **Q3 (AST & Option Mechanics in Omni)**: How does import-gated `loguru` receiver matching fit with Omni's `CodeRule<ListOption>`, `calls::find_banned_calls`, and the `ROADMAP.md` item *"Import-Aware Qualified Call Resolution"*?
- **Q4 (Interaction with Sibling the legacy linter Logging Rules)**: How does the decision on `LoggerPrintfFormatRule` relate to the legacy linter's other two logging rules (`LoggerRedundantExceptionRule` and `LoggerPositionalPlaceholderRule`) and Omni's existing `error-log-in-except` rule?
