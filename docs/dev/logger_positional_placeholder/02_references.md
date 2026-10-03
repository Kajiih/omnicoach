# Phase 2: Gather Resources and References — `LoggerPositionalPlaceholderRule`

This document records **Phase 2 (Gather Resources and References)** for evaluating the legacy linter's `LoggerPositionalPlaceholderRule` ([legacy_lints.py](../../../scratch/legacy_reference/legacy_lints.py) lines 2367–2412). It answers open questions `Q1`–`Q5` from [01_understand.md](01_understand.md).

> **Status**: **VALIDATED** (2026-10-03).
> Confidence markers: ✅ verified against official upstream source code and documentation this session.

---

## 1. Runtime Semantics Across Python Logging Libraries (`Q1`, `Q2`)

### 1.1 Loguru (`Delgan/loguru`, `loguru/_logger.py`) ✅
- **How Loguru formats messages**:
  - In `loguru/_logger.py` (`Logger._log`), Loguru inspects the call-site `*args` and `**kwargs` passed to `logger.<level>(message, *args, **kwargs)`.
  - **Only when `args or kwargs` is non-empty** does Loguru format `message` via `message.format(*args, **kwargs)` (or `Colorizer.prepare_message(message, args, kwargs)` when `opt(colors=True)` is active).
  - When **neither `args` nor `kwargs` is passed** at the call site (`logger.info("Route /orders/{order_id}")`), Loguru does **not** call `.format()` on `message`; the literal braces `{order_id}` are emitted verbatim without error.
- **Dual role of call-site `**kwargs` (Formatting + Structured Context)** ✅:
  - As documented in `loguru/_logger.py` (L454–456):
    > *"Note that while calling a logging method, the keyword arguments (if any) are automatically added to the `extra` dict for convenient contextualization (in addition to being used for formatting)."*
  - Specifically, when `opt(capture=True)` (the default), Loguru executes `record["extra"].update(kwargs)` **and** passes `**kwargs` to `message.format(*args, **kwargs)`.
  - Therefore:
    - `logger.info("Order {order_id} filled", order_id=123)` **both** formats `"Order 123 filled"` **and** attaches `extra={"order_id": 123}` for structured JSON sinks.
    - `logger.info("Order {} filled", 123)` formats `"Order 123 filled"`, but does **not** record `order_id` in `record["extra"]`.
    - `logger.info("Order {} filled", 123, user_id=456)` is **also valid Loguru**: `str.format` ignores extra unused keyword arguments (`user_id=456`), while `capture=True` still attaches `user_id=456` to `record["extra"]`!
- **`logger.bind(...)` vs. Call-Site `**kwargs`** ✅:
  - `logger.bind(order_id=123)` returns a cloned `Logger` with `order_id=123` stored in its internal `extra` dictionary (`record["extra"]`), intended for sink format templates (`format="{time} | {extra[order_id]} | {message}"`).
  - `logger.bind(...)` values are **not** passed as `**kwargs` to `message.format(*args, **kwargs)`!
    - Calling `logger.bind(order_id=123).info("Order {order_id}")` (with 0 call-site args/kwargs) skips `message.format` entirely and logs the unformatted literal `"Order {order_id}"`.
    - Calling `logger.bind(order_id=123).info("Order {order_id}: {}", status)` (with 1 positional arg) calls `"Order {order_id}: {}".format(status)` with empty `kwargs={}` and **crashes at runtime with `KeyError: 'order_id'`**!
- **Guaranteed Runtime Exceptions in Loguru**:
  1. `logger.info("Order {order_id} filled", 123)` $\to$ **`KeyError: 'order_id'`** (`args=(123,)`, `kwargs={}`).
  2. `logger.info("Order {order_id} filled", user_id=456)` $\to$ **`KeyError: 'order_id'`** (`args=()`, `kwargs={'user_id': 456}`).
  3. `logger.info("Order {} filled", order_id=123)` $\to$ **`IndexError: Replacement index 0 out of range for positional args tuple`** (`args=()`, `kwargs={'order_id': 123}`).
  4. `logger.info("Order {} filled at {}", 123)` $\to$ **`IndexError: Replacement index 1 out of range for positional args tuple`**.

---

### 1.2 Python Standard Library `logging` (`cpython/Lib/logging/__init__.py`) ✅
- **`LogRecord.getMessage()` Always Uses `%` Formatting**:
  - In Python's standard library `logging`, `LogRecord.getMessage()` is implemented as:
    ```python
    def getMessage(self):
        msg = str(self.msg)
        if self.args:
            msg = msg % self.args
        return msg
    ```
  - **Critical Distinction (`Formatter(style="{")` vs. `getMessage()`)** ✅:
    - Configuring `logging.Formatter("{asctime} {levelname} {message}", style="{")` or `logging.basicConfig(style="{")` **only** changes how `Formatter.formatMessage(record)` formats the outer `LogRecord` envelope attributes (`asctime`, `levelname`, `message`).
    - It does **not** change `record.getMessage()`! As documented in the official Python Logging Cookbook (*"Using particular formatting styles throughout your application"*), `logger.info("Order {}", 123)` and `logger.info("Order {order_id}", 123)` still execute `msg % self.args` inside `getMessage()`, raising **`TypeError: not all arguments converted during string formatting`** unless `msg` is wrapped in a custom `BraceMessage` object or `LoggerAdapter` that overrides `process()` / `__str__()`.
- **Keyword Arguments in Stdlib `logging`**:
  - `Logger._log(level, msg, args, exc_info=None, extra=None, stack_info=False, stacklevel=1)` only accepts the 4 reserved control keyword arguments (`exc_info`, `extra`, `stack_info`, `stacklevel`).
  - Passing any arbitrary keyword argument such as `logger.info("Order {order_id}", order_id=123)` raises **`TypeError: Logger._log() got an unexpected keyword argument 'order_id'`** immediately at the call site.

---

### 1.3 `structlog` (`hynek/structlog`) ✅
- **Structured Event Key-Value Pairs**:
  - In `structlog`, the first positional argument is the event name (`"order_filled"`), and arbitrary keyword arguments are structured event fields:
    ```python
    logger.info("order_filled", order_id=123, amount=49.95)
    ```
  - By default, `structlog` does **not** run `str.format` on the event string with `**kwargs` (unless a custom processor like `EventRenamer` or brace formatter is explicitly added).
  - If positional arguments are passed (`logger.info("Order %s filled", 123)`), `structlog.stdlib.PositionalArgumentsFormatter` formats them with `event % pos_args` (`%`-style).
  - Passing positional arguments to a brace-placeholder string (`logger.info("Order {order_id} filled", 123)`) fails in `PositionalArgumentsFormatter` (`TypeError: not all arguments converted during string formatting`) or drops/misformats the positional args.

---

### 1.4 Runtime Summary Matrix Across Python Logging Libraries

| Call Pattern | Loguru (`loguru.logger`) | Stdlib `logging` (`logging.Logger`) | `structlog` |
| :--- | :--- | :--- | :--- |
| `logger.info("Route /users/{id}")` *(0 format args)* | ✅ Logs literal `"Route /users/{id}"` | ✅ Logs literal `"Route /users/{id}"` | ✅ Logs event `"Route /users/{id}"` |
| `logger.info("Order {}", 123)` *(positional `{}` + pos arg)* | ✅ Logs `"Order 123"` | ❌ `TypeError: not all arguments converted` | ❌ `TypeError` (with `PositionalArgumentsFormatter`) |
| `logger.info("Order {id}", id=123)` *(named `{id}` + matching kwarg)* | ✅ Logs `"Order 123"` + sets `extra["id"] = 123` | ❌ `TypeError: unexpected keyword argument 'id'` | ⚠️ Logs literal `"Order {id}"` with structured key `id=123` |
| `logger.info("order_filled", id=123)` *(no `{}` + structured kwarg)* | ✅ Logs `"order_filled"` + sets `extra["id"] = 123` | ❌ `TypeError: unexpected keyword argument 'id'` | ✅ Standard idiomatic `structlog` |
| **`logger.info("Order {id}", 123)`** *(named `{id}` + pos arg, no `id=`)* | ❌ **`KeyError: 'id'`** | ❌ **`TypeError: not all arguments converted`** | ❌ **`TypeError` / unformatted** |
| **`logger.info("Order {}", id=123)`** *(positional `{}` + kwarg only, no pos arg)* | ❌ **`IndexError: Replacement index 0 out of range`** | ❌ **`TypeError: unexpected keyword argument 'id'`** | ⚠️ Unformatted `"Order {}"` with key `id=123` |
| **`logger.info("Order {} at {}", 123)`** *(2 `{}` + 1 pos arg)* | ❌ **`IndexError: Replacement index 1 out of range`** | ❌ **`TypeError: not all arguments converted`** | ❌ **`TypeError` / unformatted** |

---

## 2. External Linter State of the Art (`Q3`)

### 2.1 Ruff (`G`, `LOG`, `PLE12xx`, `F52x`) ✅
We inspected Ruff's upstream rule implementations (`crates/ruff_linter/src/rules/pylint/rules/logging.rs`, `flake8_logging_format`, and `pyflakes`):

1. **`flake8-logging-format` (`G001`–`G004`)**:
   - Flags eager string interpolation inside logging calls: `.format()` (`G001`), `%` (`G002`), `+` (`G003`), and f-strings (`G004`).
   - **Perverse Incentive / Interaction with `G004`**: When `G004` flags `logger.info(f"Order {order_id} filled")`, developers and LLMs frequently remove the `f` prefix and append `, order_id` positionally (`logger.info("Order {order_id} filled", order_id)`), creating a runtime `KeyError` / `TypeError` time-bomb.
2. **`pylint` Logging Rules in Ruff (`PLE1205` `logging-too-many-args`, `PLE1206` `logging-too-few-args`)** ✅:
   - Verified in [crates/ruff_linter/src/rules/pylint/rules/logging.rs:19](https://github.com/astral-sh/ruff/blob/main/crates/ruff_linter/src/rules/pylint/rules/logging.rs#L19): Ruff imports **only** `crate::rules::pyflakes::cformat::CFormatSummary` (`%`-style printf format parser).
   - Ruff does **not** parse `{}` or `{name}` PEP 3101 placeholders in `PLE1205` or `PLE1206`.
   - For `logger.info("Order {order_id} filled", order_id)`:
     - In a project using stdlib `logging`, `CFormatSummary` sees `0` `%`-specifiers and `1` positional argument, so `PLE1205` (`logging-too-many-args`) fires—**unless** `PLE1205` is disabled (which every Loguru project must do because `logger.info("Order {}", order_id)` also has `0` `%`-specifiers and triggers `PLE1205` false positives on every single Loguru call!).
     - And for `logger.info("Order {order_id} filled", user_id=123)` or `logger.info("Order {} filled", order_id=123)`, `PLE1205`/`PLE1206` **never fire at all** because there are `0` positional arguments and `0` `%`-specifiers!
3. **`pyflakes` `str.format` Rules in Ruff (`F521`–`F525`)** ✅:
   - `F521` (`string-dot-format-invalid-format`): invalid format string syntax.
   - `F522` (`string-dot-format-extra-named-arguments`): unused `k=v` in `.format()`.
   - `F523` (`string-dot-format-extra-positional-arguments`): unused positional arg in `.format()`.
   - `F524` (`string-dot-format-missing-argument`): placeholder `{name}` or `{}` in `.format()` has no matching argument (`KeyError` / `IndexError`).
   - `F525` (`string-dot-format-mixing-automatic`): mixes `{}` and `{0}` in `.format()` (`ValueError`).
   - **Critical SOTA Gap**: Ruff only runs `F521`–`F525` on `<str>.format(...)` AST nodes, **never** on `logger.<level>("<str>", ...)`.

### 2.2 Pylint (`checkers/logging.py`, `E1205`/`E1206`, `logging-format-style`) ✅
- Verified in [pylint/checkers/logging.py](https://github.com/pylint-dev/pylint/blob/main/pylint/checkers/logging.py):
  - Pylint supports `logging-format-style=old` (`%`, default) and `logging-format-style=new` (`{}`).
  - Under `logging-format-style=new`, Pylint's `parse_format_method_string` parses PEP 3101 `{}` placeholders and checks positional argument counts (`E1205` / `E1206`).
  - However, because Pylint assumes stdlib `logging` (where `Logger._log` rejects arbitrary keyword arguments), Pylint's `logging-format-style=new` treats any named placeholder `{name}` as an error unless custom `keywords` handling is added—making it unusable for Loguru's `logger.info("Order {order_id}", order_id=123)`.

### 2.3 Comparison Table (`R1`–`R7`)

| ID | Reference | What It Does | Adopt / Adapt / Reject | Why |
| :--- | :--- | :--- | :--- | :--- |
| **R1** | **The legacy linter `LoggerPositionalPlaceholderRule`** ([legacy_lints.py:2367-2412](../../../scratch/legacy_reference/legacy_lints.py#L2367-L2412)) ✅ | Flags any `{field_name}` where `field_name and not field_name.isdigit()` in `node.args[0]` of `logger.<method>(...)`. | **Reject Angle 1** (style ban on `{name}`).<br>**Adapt into Angle 2** (if kept as a reliability rule) or **Drop (Angle 3)**. | Flags zero-arg calls (`logger.info("Route /users/{id}")`), bans valid Loguru keyword formatting + `extra` capture, misidentifies `{0.attr}` as named, and prescribes `{}` which breaks stdlib `logging`. |
| **R2** | **Loguru `Logger._log`** (`Delgan/loguru`) ✅ | Formats `message.format(*args, **kwargs)` iff `args or kwargs` is non-empty; captures `kwargs` into `record["extra"]`. | **Adopt** as the ground-truth execution model for brace-formatted logger calls. | Proves that: (1) zero-arg calls never format; (2) `kwargs` are valid both for `{name}` and for `extra`; (3) unsatisfied `{name}` or `{}` when `args or kwargs` is non-empty is a guaranteed `KeyError` / `IndexError`. |
| **R3** | **Python Stdlib `logging`** (`LogRecord.getMessage`) ✅ | Formats `msg % self.args` iff `self.args` is non-empty; `Formatter(style="{")` only affects handler envelope. | **Adopt** as a hard constraint on rule diagnostics and false positives. | Any rule in Omni must never tell stdlib `logging` users to replace `%s` with `{}`, and must not break stdlib `logging` calls. |
| **R4** | **`structlog`** (`hynek/structlog`) ✅ | Uses `logger.info("event", k=v)` for structured fields without `{k}` placeholders in the event string. | **Adopt** (**D5**): never flag unused keyword arguments in logger calls. | In both `structlog` and Loguru (`record["extra"]`), keyword arguments without matching `{k}` placeholders are valid structured metadata. |
| **R5** | **Ruff `G004` (`logging-f-string`) & `PLE1205`/`PLE1206`** ✅ | `G004` bans `f"..."` in logger calls; `PLE1205`/`PLE1206` check `%` arg counts only (disabled in Loguru repos). | **Identify SOTA Gap**: Ruff has zero placeholder validation for `{}` / `{name}` in `logger.*` calls. | Fixing `G004` by stripping `f` and adding `, x` produces `logger.info("Order {x}", x)`, which Ruff silently passes and which crashes at runtime. |
| **R6** | **Ruff Pyflakes `F521`–`F525` (`string-dot-format-*`)** ✅ | Validates PEP 3101 `{}` and `{name}` placeholders against `*args` and `**kwargs` on `.format()` calls. | **Adapt** `F524` (missing argument for placeholder) & `F525` (mixed auto/manual numbering) if Angle 2 is chosen; **Reject** `F522` (extra named args). | `F524` is the exact reliability check needed when format arguments are passed to a brace-formatted logger call. |
| **R7** | **Pylint `E1205`/`E1206` (`logging-format-style=new`)** ✅ | Checks `{}` placeholder counts in `logging` calls when `logging-format-style=new`. | **Adapt** PEP 3101 root-field parsing (`{0.attr}` $\to$ index `0`). | Avoids the legacy linter's `"0.id".isdigit() == False` bug. |

---

## 3. Detailed Evaluation of the Three Angles (`Q4`, `Q5`)

### Angle 1: Blanket Style Ban on Named Placeholders `{name}` in Logger Calls
- **What it checks**: Flags any `{name}` placeholder in `logger.<level>(msg, ...)` (even when `name=val` is passed as a keyword argument).
- **Assessment against Omni's Guides**:
  1. **Breaks valid, idiomatic code with no runtime hazard**: `logger.info("Order {order_id} filled", order_id=123)` is official Loguru style that simultaneously interpolates `{order_id}` and records `extra["order_id"] = 123`.
  2. **Creates a perverse incentive ([rule_design_guide.md §3](../rule_design_guide.md))**: To satisfy a ban on `{order_id}` while preserving structured logging in Loguru, developers must write `logger.bind(order_id=123).info("Order {} filled", 123)`, duplicating both the key and the value—or drop structured `extra` fields entirely.
  3. **Contradicts `Topic::POSITIONAL_MEANING`**: When logging 3+ values (`logger.info("Moved {qty} of {sku} from {src} to {dst}", ...)`), named placeholders are strictly clearer and less error-prone than 4 anonymous `{}` placeholders.
  4. **Prescribes a broken fix for stdlib `logging`**: Suggesting `{}` in stdlib `logging` causes `TypeError` at runtime.
- **Verdict on Angle 1**: **Reject unconditionally.**

---

### Angle 2: Mismatched Logger Placeholders (Reliability Check)
- **Core Insight**:
  Instead of banning named placeholders as a style preference, what if we check for **unsatisfied PEP 3101 placeholders in logger calls that pass format arguments** (the `logger.*` counterpart of Ruff `F524` `string-dot-format-missing-argument`)?
- **Let's trace the exact conditions of Angle 2**:
  Suppose `logger.<level>(msg, *pos_args, **kw_args)` is called where:
  1. `msg` is a string literal (`args[0]` for `trace`/`debug`/`info`/`success`/`warning`/`error`/`critical`/`exception`, or `args[1]` for `log`).
  2. At least one **format argument** is passed at the call site:
     - `pos_args`: positional arguments after `msg`, or
     - `format_kw_args`: keyword arguments excluding stdlib `logging` control kwargs (`exc_info`, `stack_info`, `stacklevel`, `extra`).
     - *(Note: If zero format arguments are passed—e.g. `logger.info("Route /users/{id}")` or `logger.error("Failed: {id}", exc_info=True)`—nothing is formatted at runtime, so the call is **never** flagged!)*
  3. No `*args` unpacking prevents positional counting, and no `**kwargs` unpacking prevents keyword lookup.
  4. `msg` contains a valid PEP 3101 replacement field `{field_name...}` (ignoring escaped `{{` and `}}`) whose root field (`field_name.split('.')[0].split('[')[0]`) is **unsatisfied** by the call-site arguments:
     - **Case A (Unsatisfied Named Placeholder — e.g. `logger.info("Order {order_id}", order_id)` or `logger.info("Order {order_id}", other_id=1)`)**:
       - Root field is an identifier `name` (`!root.is_empty() && !root.chars().all(|c| c.is_ascii_digit())`), there is no `**kwargs` splat at the call site, and `name` is **not** among the call-site keyword argument names!
       - **Runtime outcome**: Raises `KeyError: 'order_id'` in Loguru / `BraceMessage`, and raises `TypeError: not all arguments converted` in stdlib `logging` (when `pos_args` is non-empty) or `TypeError: unexpected keyword argument` (when `format_kw_args` is non-empty).
       - **Zero false positives** across Loguru, stdlib `logging`, and `structlog`! (Wait: what if a `structlog` user writes `logger.info("Route /users/{order_id}", user_id=123)` where `"Route /users/{order_id}"` is a literal event name with braces and `user_id=123` is a structured kwarg? Wait—Look closely at that! If we only flag Case A when **positional format arguments** are present—i.e., `logger.info("Order {order_id}", order_id)`—then `structlog` keyword calls `logger.info("Route /users/{order_id}", user_id=123)` are 100% exempt too!)
     - **Case B (Unsatisfied Positional Placeholder — e.g. `logger.info("Order {}", order_id=123)` or `logger.info("Order {} at {}", order_id)`)**:
       - Root field is empty `{}` (auto-numbered `0, 1, ...`) or integer `{i}`, there is no `*args` splat at the call site, and the required positional index $\ge \text{len}(\text{pos\_args})$.
       - *(Note on `structlog` / stdlib `logging` for Case B)*: If `pos_args` is non-empty (`logger.info("Order {} at {}", order_id)`), both Loguru (`IndexError`) and stdlib `logging` (`TypeError`) fail at runtime! If `pos_args` is empty and only `format_kw_args` are passed (`logger.info("Order {}", order_id=123)`), Loguru raises `IndexError` and stdlib `logging` raises `TypeError`.

- **Wait: Let's compare the two scopes of Angle 2!**
  - **Scope 2A (Focused: Named placeholder with positional format arguments — `logger.info("Order {order_id}", order_id)`)**:
    - Condition: `msg` has a named placeholder `{name}`, the call passes $\ge 1$ **positional** format argument (`len(pos_args) >= 1`), and `name=` is not passed as a keyword argument (and no `**kwargs` splat).
    - Why Scope 2A is razor-sharp:
      1. Directly catches the #1 real-world bug that motivated the legacy linter's rule: stripping `f` from `logger.info(f"Order {order_id}")` and appending `, order_id` positionally (`logger.info("Order {order_id}", order_id)`).
      2. **100% broken in EVERY Python logging library** (`KeyError` in Loguru/BraceMessage; `TypeError` in stdlib `logging` and `structlog` because positional args are passed without `%` specifiers).
      3. **Zero false positives in `structlog`**, even if a `structlog` event string contains literal `{braces}` alongside structured `key=val` kwargs (`logger.info("GET /users/{id}", status=200)` has `0` positional format args, so Scope 2A does not touch it!).
      4. **Zero false positives in Loguru** on `logger.info("Order {order_id}", order_id=123)` (has matching `order_id=` kwarg) or `logger.info("Route /users/{id}")` (has `0` positional format args).
      5. **Clear, single antipattern ([rule_design_guide.md §1](../rule_design_guide.md))**:
         - **Rule name**: `unmatched-logger-placeholder` or `named-placeholder-with-positional-log-arg` (or `mismatched-logger-placeholder`).
         - **Summary**: ``Logger call passes positional arguments to a message with named placeholder `{ `{token}` }`.``
         - **Rationale**: `Positional arguments do not bind to named format placeholders, so formatting raises `KeyError` or `TypeError` at runtime.`
         - **Suggestion**: `Replace `{ `{token}` }` with positional `{}` (or `%s` for `logging`), or pass `{token}=...` as a keyword argument.`
  - **Scope 2B (Broader: All missing format arguments in brace-formatted logger calls)**:
    - Also flags `logger.info("Order {} at {}", a)` (too few positional args for `{}`) and `logger.info("Order {}", order_id=123)` (`{}` with kwarg only).
    - Trade-off: Under [rule_design_guide.md §1](../rule_design_guide.md) (*The Split Test: If two violations differ in why they are bad or how to fix them, they are separate rules*), `logger.info("Order {} at {}", a)` (missing 2nd argument) has a different fix from `logger.info("Order {order_id}", order_id)` (where the argument *is* present, just passed positionally instead of by keyword or `{}`). Furthermore, in `structlog`, `logger.info("Empty dict {}", user_id=123)` logs literal `"Empty dict {}"` with structured field `user_id=123` without error, so flagging `{}` when only kwargs are present could false-positive on `structlog`.

---

### Angle 3: Drop the Rule (Do Not Port `LoggerPositionalPlaceholderRule`)
- **Arguments for Dropping**:
  1. The legacy linter's original rule (`LoggerPositionalPlaceholderRule`) was a house-style rule written specifically for the legacy linter's Loguru conventions (`"Enforce positional '{}' placeholders to align"`), and as a style rule (Angle 1) it is harmful outside that convention.
  2. Omni currently has 30 high-signal rules; every rule in Omni must justify its maintenance and cognitive surface area.
  3. If Angle 2 (catching `logger.info("Order {order_id}", order_id)`) is considered too narrow or best tracked as a future candidate in `ROADMAP.md` alongside other logging rules (like `LoggerPrintfFormatRule` and `LoggerRedundantExceptionRule`), dropping `LoggerPositionalPlaceholderRule` now and recording the Angle 2 design in `ROADMAP.md` keeps the rule set lean.
- **Arguments Against Dropping (in favor of Angle 2A)**:
  1. `logger.info("Order {order_id}", order_id)` is a classic LLM and developer bug triggered directly by fixing Ruff `G004` (`logging-f-string`).
  2. Ruff has a verified blind spot here: `PLE1205` only checks `%` strings (and must be disabled in Loguru repos), and `F524` only checks `.format()` calls.
  3. Scope 2A is tiny (~60 lines of AST logic), `Precision::Exact`, `Consensus::Unopinionated`, `ImpactedQuality::Reliability`, and has zero false positives across `logging`, `loguru`, and `structlog`.

---

## 4. Comparison Summary: Angle 1 vs. Angle 2A vs. Angle 3

| Criterion | Angle 1: Blanket Style Ban on `{name}` (the legacy linter) | Angle 2A: Named Placeholder with Positional Format Arg (`unmatched-logger-placeholder`) | Angle 3: Drop the Rule |
| :--- | :--- | :--- | :--- |
| **Catches `logger.info("Order {id}", id)` (`KeyError` / `TypeError`)** | Yes (by accident) | **Yes (directly & exclusively)** | No |
| **Allows `logger.info("Route /users/{id}")` (0 format args)** | ❌ False positive in the legacy linter | ✅ Passes | ✅ N/A |
| **Allows `logger.info("Order {id}", id=123)` (valid Loguru + `extra`)** | ❌ False positive (bans valid Loguru) | ✅ Passes | ✅ N/A |
| **Allows `logger.info("GET /users/{id}", status=200)` (`structlog`)** | ❌ False positive | ✅ Passes (0 positional format args) | ✅ N/A |
| **Allows `logger.info("Order {0.id}", order)` (positional attr)** | ❌ False positive in the legacy linter | ✅ Passes (root field `"0"` is digits) | ✅ N/A |
| **Handles `logger.log(level, "Order {id}", id)` (`args[1]`)** | ❌ Missed in the legacy linter | ✅ Checks `args[1]` | ✅ N/A |
| **Facet Classification ([tag_guide.md](../tag_guide.md))** | `Maintainability` · `Opinionated` · `Heuristic` | `Reliability` · `Unopinionated` · `Exact` | N/A |

---

## 5. Numbered Recommendations for User Decision (`R-D1`–`R-D4`)

| ID | Recommendation | Rationale |
| :--- | :--- | :--- |
| **R-D1** | **Reject Angle 1** (the legacy linter's blanket ban on named `{name}` placeholders in logger calls). | Breaks valid Loguru keyword formatting and structured `record["extra"]` capture, conflicts with `structlog` and stdlib `logging`, and contradicts `Topic::POSITIONAL_MEANING` when logging multiple values. |
| **R-D2** | **Choose between Angle 2A (Implement `unmatched-logger-placeholder`) and Angle 3 (Drop / Defer to `ROADMAP.md`)**:<br>- **Option A (Recommended if we want to close the Ruff `G004`/`F524` reliability gap)**: Pivot the rule to **Angle 2A** (`unmatched-logger-placeholder`), flagging logger calls that pass positional format arguments when the message string contains a named placeholder `{name}` with no matching `name=` keyword argument (or `**kwargs` splat).<br>- **Option B (Recommended if we only want to port the legacy linter rules whose original intent holds)**: **Drop `LoggerPositionalPlaceholderRule` (Angle 3)** and optionally record Angle 2A in `ROADMAP.md` under Candidate Rules. | Angle 2A transforms a flawed Loguru house-style rule into an `Exact`, `Unopinionated`, `Reliability` bug detector that catches broken `f`-string conversions (`logger.info("Order {id}", id)`) across all Python logging frameworks with zero false positives. |
| **R-D3** | **If Angle 2A is selected — Callee & Argument Contract**:<br>- Match configurable logger methods (`trace`, `debug`, `info`, `success`, `warning`, `error`, `critical`, `exception`, `log`) on `logger` / `log` / `logging` receivers (or `*.logger` / `*.log`).<br>- For `log(...)`, inspect `args[1]` as the format string and `args[2..]` as positional format args; for all other methods, inspect `args[0]` and `args[1..]`.<br>- Extract the PEP 3101 root field name (`field.split('.')[0].split('[')[0]`) while skipping `{{` and `}}` escapes; flag when a root field is a non-numeric identifier, `pos_args` is non-empty, no `**kwargs` splat is present, and no keyword argument matches that root field name. | Fixes all 5 defects in the legacy linter's original implementation while remaining 100% single-file CST-based. |
| **R-D4** | **If Angle 2A is selected — Classification & Wording**:<br>- **Name**: `unmatched-logger-placeholder` (or `mismatched-logger-placeholder`).<br>- **Classification**: `topics: &[Topic::LOGGING]`, `precision: Precision::Exact`, `consensus: Consensus::Unopinionated`, `impacted_quality: ImpactedQuality::Reliability`, `target: RuleTarget::All`. | Every flagged call is guaranteed to fail at runtime (`KeyError` in Loguru / `str.format`, `TypeError` in stdlib `logging`) regardless of whether it is in source or test code. |

---

## 6. Sources
- Loguru `_logger.py` (`Logger._log`, keyword argument formatting & `extra` capture): https://github.com/Delgan/loguru/blob/master/loguru/_logger.py
- Python Standard Library `logging` (`LogRecord.getMessage`) & Logging Cookbook (*Using particular formatting styles throughout your application*): https://docs.python.org/3/howto/logging-cookbook.html#using-particular-formatting-styles-throughout-your-application
- Ruff `PLE1205` (`logging-too-many-args`) & `PLE1206` (`logging-too-few-args`) implementation: https://github.com/astral-sh/ruff/blob/main/crates/ruff_linter/src/rules/pylint/rules/logging.rs
- Ruff `F524` (`string-dot-format-missing-argument`): https://docs.astral.sh/ruff/rules/string-dot-format-missing-argument/
- Ruff `G004` (`logging-f-string`): https://docs.astral.sh/ruff/rules/logging-f-string/
- Pylint `checkers/logging.py` (`E1205`, `E1206`, `logging-format-style`): https://github.com/pylint-dev/pylint/blob/main/pylint/checkers/logging.py
- `structlog` documentation: https://www.structlog.org/
