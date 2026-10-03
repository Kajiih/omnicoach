# Phase 1: Understand — `LoggerPositionalPlaceholderRule`

This document records **Phase 1 (Understand)** for evaluating the legacy linter's `LoggerPositionalPlaceholderRule` ([legacy_lints.py](../../../scratch/legacy_reference/legacy_lints.py) lines 2367–2412).

> **Status**: **VALIDATED** (2026-10-03). Covers exact analysis of the legacy linter's rule, runtime semantics across Python logging libraries, and the three candidate design angles (**Angle 1**: Style ban on named placeholders; **Angle 2**: Mismatched logger placeholders; **Angle 3**: Drop the rule).

---

## 1. What `LoggerPositionalPlaceholderRule` Checks in the legacy linter

In [legacy_lints.py:2367-2412](../../../scratch/legacy_reference/legacy_lints.py#L2367-L2412), `LoggerPositionalPlaceholderRule` is defined alongside two other Loguru-specific rules (`LoggerPrintfFormatRule` at L2276–2320 and `LoggerRedundantExceptionRule` at L2322–2365):

1. **Matched Callees (`_is_logger_call`, L2405–2411)**:
   - Matches `ast.Call` where `node.func` is an `ast.Attribute` with `func.attr` in:
     `{"trace", "debug", "info", "success", "warning", "error", "critical", "exception", "log"}`
   - Requires the receiver `func.value` to be either:
     - `ast.Name` with `id in {"logger", "log"}` (for example, `logger.info(...)`, `log.warning(...)`), or
     - `ast.Attribute` with `attr in {"logger", "log"}` (for example, `self.logger.info(...)`, `cls.log.error(...)`).
   - Note that `"trace"` and `"success"` are **Loguru-specific** severity levels, and the sibling rules explicitly state `"Loguru uses '{}' formatting"` (L2310) and `"Loguru automatically captures the traceback"` (L2353).

2. **Format String Inspection (`check`, L2384–2403)**:
   - Requires `node.args` to be non-empty (`len(node.args) >= 1`) and inspects `first_arg = node.args[0]`.
   - If `first_arg` is a string constant (`isinstance(first_arg, ast.Constant) and isinstance(first_arg.value, str)`), it iterates over `string.Formatter().parse(first_arg.value)`.
   - For the first replacement field where `field_name and not field_name.isdigit()`, it reports:
     `"Logger call uses named placeholder '{field_name}'. Enforce positional '{}' placeholders to align."`
   - Catches and ignores `ValueError` on malformed brace strings (such as unmatched single `{` or `}`).

3. **What the legacy linter Does *Not* Inspect**:
   - It never inspects `node.args[1:]` (positional format arguments) or `node.keywords` (keyword format arguments).
   - Consequently, all three of the following calls receive the **exact same diagnostic**:
     ```python
     # 1. Zero format arguments (literal braces in message, e.g. route path or JSON/template snippet)
     logger.info("Registered FastAPI route /orders/{order_id}")

     # 2. Valid Loguru keyword formatting + structured extra context capture
     logger.info("Order {order_id} filled", order_id=123)

     # 3. Broken f-string conversion (named placeholder + positional arg -> runtime KeyError!)
     logger.info("Order {order_id} filled", 123)
     ```

---

## 2. Why the legacy linter Had This Rule & Its Structural Defects

### 2.1 Conflated Motivations in the legacy linter
The legacy linter's rule conflates two fundamentally different concerns under a single check:

1. **Reliability Bug (Incomplete f-string conversion $\to$ runtime `KeyError`)**:
   - When developers or AI coding agents write `logger.info(f"Order {order_id} filled")`, linters such as Ruff `G004` (`logging-f-string`) or Pylint `W1203` (`logging-fstring-interpolation`) flag the eager f-string.
   - A very common human and LLM mistake when fixing `G004` is to strip the `f` prefix and append `, order_id` positionally without emptying `{order_id}`:
     ```python
     logger.info("Order {order_id} filled", order_id)
     ```
   - In **Loguru**, this executes `"Order {order_id} filled".format(order_id)` at runtime, which immediately raises **`KeyError: 'order_id'`**.
   - In **stdlib `logging`**, this executes `"Order {order_id} filled" % (order_id,)` inside `LogRecord.getMessage()`, which raises **`TypeError: not all arguments converted during string formatting`**.
   - Crucially, **Ruff misses this bug completely**: `G004` no longer fires (it is not an f-string), `PLE1205`/`PLE1206` only parse `%`-style format strings, and `F522`–`F525` only inspect `.format(...)` method calls.

2. **House Style Preference (Positional `{}` vs. Named `{order_id}`)**:
   - The legacy linter also wanted every Loguru call to use positional `logger.info("Order {} filled", order_id)` rather than `logger.info("Order {order_id} filled", order_id=order_id)`.

### 2.2 Five Concrete Defects in the legacy linter's Rule as Written

1. **False Positives on Zero-Argument Log Calls with Literal Braces**:
   - Because `check()` only checks `if not node.args: return`, any single-argument call `logger.info("Route /users/{user_id} matched")` or `logger.debug("Invalid template variable {foo}")` has `len(node.args) == 1` and is flagged.
   - Neither Loguru (`if args or kwargs:`) nor stdlib `logging` (`if self.args:`) performs string interpolation when zero format arguments are passed. Changing `"/users/{user_id}"` to `"/users/{}"` corrupts the logged string.
2. **Breaks Valid Loguru Keyword Formatting and Structured Context Capture**:
   - In Loguru (`loguru/_logger.py`), keyword arguments passed to `logger.info("Order {order_id}", order_id=123)` serve a **dual purpose**: they format `{order_id}` in the message **and** automatically populate `record["extra"]["order_id"] = 123` for structured JSON sinks.
   - Positional arguments (`logger.info("Order {}", 123)`) are **not** added to `record["extra"]`. Banning `{order_id}` forces developers who need structured context to write `logger.bind(order_id=123).info("Order {}", 123)`, duplicating the value.
3. **Dangerous Suggestion for Standard Library `logging` and `structlog`**:
   - In Python's stdlib `logging`, `LogRecord.getMessage()` unconditionally evaluates `msg = msg % self.args` (even when `logging.Formatter(..., style="{")` is configured on handlers, because `style="{"` only controls the handler envelope `{asctime} {message}`, not `getMessage()`).
   - Telling a stdlib `logging` user to use `{}` (`logger.info("Order {}", order_id)`) causes a runtime `TypeError` inside `logging`!
   - In `structlog`, keyword arguments are structured event attributes (`logger.info("order_filled", order_id=123)`), not `str.format` placeholders.
4. **False Positive on Positional Compound Fields (`{0.attr}`, `{0[key]}`)**:
   - `string.Formatter().parse("Order {0.id}")` returns `field_name = "0.id"`.
   - Because `"0.id".isdigit()` is `False`, the legacy linter falsely flags `{0.id}` and `{0[0]}` as named placeholders even though their root argument index `0` is positional.
5. **Misses `logger.log(level, msg, ...)`**:
   - `_LOGGER_METHODS` includes `"log"`, where `node.args[0]` is the level (`"INFO"` or `logging.INFO`) and `node.args[1]` is the message string. Inspecting `node.args[0]` never checks the format string of `logger.log(...)`.

---

## 3. The Three Candidate Design Angles

We evaluate three candidate directions across Phase 1 and Phase 2:

| Angle | Description | Target Antipattern | Quality & Consensus |
| :--- | :--- | :--- | :--- |
| **Angle 1: Style Ban on Named Placeholders** | Ban `{name}` placeholders in logger format strings (either unconditionally like the legacy linter, or when format arguments are present), enforcing positional `{}`. | Mixing `{name}` and `{}` placeholder styles in a Loguru-only codebase. | `Maintainability` · `Opinionated` · `Heuristic` |
| **Angle 2: Mismatched Logger Placeholders** | Flag logger calls where a brace-formatted message string and its format arguments cannot succeed at runtime—specifically, a named placeholder `{name}` when format arguments are passed without a matching `name=` keyword argument (or `**kwargs`), or positional `{}` / `{i}` placeholders with insufficient positional arguments when keyword/positional format arguments are passed. | Guaranteed runtime `KeyError` / `IndexError` / `TypeError` in logger calls (such as `logger.info("Order {order_id}", order_id)` after stripping `f` from an f-string). | `Reliability` · `Unopinionated` · `Exact` (or low-noise `Heuristic`) |
| **Angle 3: Drop the Rule** | Do not port `LoggerPositionalPlaceholderRule` to Omni. | N/A — reject Angle 1 as hostile to Loguru/`logging`/`structlog`, and leave Angle 2 either unimplemented or deferred to `ROADMAP.md`. | N/A |

---

## 4. Goals & Explicit Non-Goals

### 4.1 Goals

| ID | Goal | Reason |
| :--- | :--- | :--- |
| **G1** | Establish the exact runtime behavior of `{name}` and `{}` placeholders across **Loguru**, **stdlib `logging`**, **`structlog`**, and **Rust (`log` / `tracing`)**. | Prevents designing a rule around false assumptions about how Python/Rust loggers interpolate strings. |
| **G2** | Audit SOTA coverage in **Ruff** (`G`, `LOG`, `PLE1205`/`PLE1206`, `F521`–`F525`) and **Pylint** (`W1201`–`W1203`, `E1200`–`E1206`). | Identifies whether a genuine static-analysis gap exists or if existing linters already cover the problem. |
| **G3** | Rigorously compare **Angle 1** (style ban), **Angle 2** (mismatched placeholders), and **Angle 3** (drop) against [rule_design_guide.md](../rule_design_guide.md) and [tag_guide.md](../tag_guide.md). | Ensures any rule admitted to Omni has a clear failure mode, zero perverse incentives, and orthogonal `summary` / `rationale` / `suggestion` fields. |

### 4.2 Explicit Non-Goals

| ID | Non-Goal | Reason |
| :--- | :--- | :--- |
| **NG1** | Rust support (`log::info!`, `tracing::info!`). | `rustc` validates `format_args!` placeholders and arguments at compile time, and Rust 2021+ `{ident}` implicit capture is idiomatic standard Rust. |
| **NG2** | Duplicating Ruff's `G004` (`logging-f-string`) or `PLE1205`/`PLE1206` (`%`-format arg count). | Already implemented and enabled by default in Ruff's `G` and `PLE` rule sets. |
| **NG3** | Cross-file or whole-program logger type inference. | Omni operates per-file on Tree-sitter CSTs (`ParsedFile`). |

---

## 5. Numbered Decisions (`D1`–`D5`)

| ID | Decision | Rationale |
| :--- | :--- | :--- |
| **D1** | **Reject the legacy linter's naive zero-argument check (`len(node.args) >= 1`) under all angles.** | Flagging `logger.info("Route /users/{user_id}")` when zero format arguments are passed is an unconditional false positive across every Python logging library. |
| **D2** | **Reject Angle 1 (blanket style ban on `{name}` in logger calls).** | Even if restricted to calls with arguments, banning `logger.info("Order {order_id}", order_id=123)` breaks valid Loguru keyword formatting and structured `record["extra"]` capture, contradicts Omni's `positional-meaning` principle when logging multiple values, and prescribes `{}` which is broken in stdlib `logging`. |
| **D3** | **Evaluate Angle 2 (Mismatched Logger Placeholders) vs. Angle 3 (Drop) as the core decision.** | `logger.info("Order {order_id}", order_id)` is a high-frequency real-world bug (especially after fixing `G004` `logging-f-string`) that crashes at runtime in **both** Loguru (`KeyError`) and stdlib `logging` (`TypeError`) and is completely missed by Ruff (`PLE1205`/`PLE1206` only check `%`; `F522`–`F525` only check `.format()`). |
| **D4** | **Correctly parse PEP 3101 field names (`field_name.split('.')[0].split('[')[0]`) and `logger.log(level, msg, ...)` message index.** | If Angle 2 is adopted, `{0.attr}` and `{0[key]}` have root field `"0"` (positional index `0`, not a named field), and `logger.log(level, msg, *args)` has its message at `args[1]`. |
| **D5** | **Do not flag extra keyword arguments in logger calls.** | Unlike `str.format()` (`F522`), logger calls in `structlog` (`logger.info("event", k=v)`), Loguru (`logger.info("Order {}", x, extra_ctx=y)`), and stdlib `logging` (`exc_info=True`, `extra={...}`, `stack_info=True`, `stacklevel=2`) legitimately pass keyword arguments that are not referenced as `{k}` in the message string. |

---

## 6. Open Questions for Phase 2 (`Q1`–`Q5`)

- **Q1 (Loguru Internals)**: How does Loguru's `_logger.py` handle `*args`, `**kwargs`, and `.bind()` during message formatting? Specifically, can `logger.bind(order_id=123).info("Order {order_id}", other_arg)` resolve `{order_id}` from `bind()`, or does `message.format(*args, **kwargs)` only see call-site `*args` and `**kwargs`?
- **Q2 (Stdlib `logging` & `structlog` Internals)**: What happens at runtime in stdlib `logging` and `structlog` when `logger.info("Order {order_id}", order_id)` or `logger.info("Order {}", order_id=123)` is called? Does `Formatter(style="{")` make `logger.info("Order {}", 123)` work in stdlib `logging`?
- **Q3 (Ruff & Pylint SOTA Audit)**: What exact rules exist in Ruff (`G`, `LOG`, `PLE12xx`, `F52x`) and Pylint (`E12xx`, `W12xx`), and why does Ruff miss `logger.info("Order {order_id}", order_id)`?
- **Q4 (Angle 2 Scope & Edge Cases)**: If we adopt **Angle 2** (`mismatched-logger-placeholder` or `unsatisfied-logger-placeholder`), what exact mismatch conditions are 100% false-positive-free across Loguru, stdlib `logging`, and `structlog`?
- **Q5 (Final Recommendation — Angle 2 vs. Angle 3)**: Should Omni implement Angle 2 now, defer it to `ROADMAP.md`, or drop `LoggerPositionalPlaceholderRule` entirely?
