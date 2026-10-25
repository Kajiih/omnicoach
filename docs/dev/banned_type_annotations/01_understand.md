# Phase 1: Understand — `BannedTypeAnnotationsRule` & Adjacent Type/Decimal Checks

> **Status:** VALIDATED — DROPPED (user decision: no rule; see 03_design.md §0).

## 1. Original Checks in the Legacy Linter

This investigation covers the legacy linter's `BannedTypeAnnotationsRule` (`legacy_lints.py:3165–3387`) along with three adjacent rules built on the same `AnnotationInspectorRule` base class (`legacy_lints.py:1189–1272`) or adjacent `Decimal` checks (`legacy_lints.py:1274–1341`):

1. **`BannedTypeAnnotationsRule`** (`banned-type-annotations`, `Severity.ERROR`, lines `3165–3387`)
2. **`PrimitiveTypeSuggestionsRule`** (`primitive-type-suggestions`, `Severity.WARNING`, lines `1274–1291`, **disabled/commented out** at line `3671`)
3. **`FloatToDecimalSuggestionsRule`** (`float-to-decimal-suggestions`, `Severity.WARNING`, lines `1293–1310`)
4. **`DecimalZeroFormattingRule`** (`decimal-zero-formatting`, `Severity.ERROR`, lines `1312–1341`)

---

### 1.1 Shared Base Class: `AnnotationInspectorRule` (`lines 1189–1272`)

`BannedTypeAnnotationsRule`, `PrimitiveTypeSuggestionsRule`, and `FloatToDecimalSuggestionsRule` all inherit from `AnnotationInspectorRule`, which inspects type annotations at three AST sites:

- **Function parameters**: `posonlyargs`, `args`, `kwonlyargs`, `vararg`, and `kwarg` on `ast.FunctionDef` and `ast.AsyncFunctionDef`.
- **Function returns**: `node.returns` on `ast.FunctionDef` and `ast.AsyncFunctionDef`.
- **Attributes**: `ast.AnnAssign` where `self._is_attribute(node, parent_map)` is true:
  - `self.<attr>: <annotation>` inside any function or method (not just `__init__`), or
  - `<attr>: <annotation>` directly inside an `ast.ClassDef` body (including private attributes and `TypedDict` fields).
- **Exemptions in `AnnotationInspectorRule`**:
  - Test files (`is_test_file(file_path)`).
  - Functions decorated with `@override` (`is_overridden_method(node)`).

---

### 1.2 `BannedTypeAnnotationsRule` (`lines 3165–3387`)

For every inspected parameter, return, and attribute annotation, `BannedTypeAnnotationsRule._check_annotation` runs `ast.walk(annotation)` and checks every `ast.Subscript` subnode (at any nesting depth) against six hardcoded `_SubscriptMatcher` predicates. The domain type names in patterns 1–5 are anonymized placeholders for the downstream project's own types, and their messages are paraphrased; the shapes (arity, unions, nesting) match the legacy code.

| # | Matcher | AST Pattern Matched | Replacement Message |
|---|---|---|---|
| 1 | `_is_dimensions` | `tuple[Width, Height]` | `"Use 'Dimensions' instead of 'tuple[Width, Height]'."` |
| 2 | `_is_value_range` | `tuple[Bound \| None, Bound \| None]` | `"Use 'ValueRange' instead of 'tuple[Bound \| None, Bound \| None]'."` |
| 3 | `_is_merge_inputs` | `tuple[Iterator[Dimensions], Iterator[Dimensions], SortOrder, SortOrder]` | `"Use 'MergeInputs' instead of 'tuple[Iterator[Dimensions], ...]'."` |
| 4 | `_is_client_setup` | `tuple[HttpClient \| FakeClient, Credentials \| None]` | `"Use 'ClientSetup' instead of 'tuple[HttpClient \| FakeClient, Credentials \| None]'."` |
| 5 | `_is_pending_request` | `tuple[anyio.Event, list[IPCResponse \| BaseException]]` | `"Use 'PendingRequest' instead of 'tuple[anyio.Event, list[IPCResponse \| BaseException]]'."` |
| 6 | `_is_coroutine_object` | `Coroutine[object, object, ...]` (`len(elts) >= 2` with `elts[0] == Name("object")` and `elts[1] == Name("object")`) | `"Avoid 'Coroutine[object, object, ...]'. Consider 'Awaitable[...]', 'Coroutine[None, None, ...]', or refactoring."` |

---

### 1.3 `PrimitiveTypeSuggestionsRule` (`lines 1274–1291`, disabled at `line 3671`)

- **Trigger**: Any parameter, return, or attribute whose top-level annotation is `ast.Name(id="str" | "int" | "float")` (or `X[...]`).
- **Message**: `"Parameter 'x' uses primitive type 'int'. Consider a semantic type (NewType, TypeAlias, Enum, Literal, Dataclass) if it represents a domain concept."`
- **Status in the legacy linter**: Explicitly commented out (`# PrimitiveTypeSuggestionsRule(),`) in `get_all_rules()` because flagging every `str`, `int`, and `float` parameter, return, and attribute across a codebase produces near-100% false-positive noise.

---

### 1.4 `FloatToDecimalSuggestionsRule` (`lines 1293–1310`)

- **Trigger**: Any parameter, return, or attribute whose top-level annotation name is `"float"`.
- **Message**: `"Parameter 'x' uses 'float'. Consider 'Decimal' for precision in monetary calculations."`
- **Severity in the legacy linter**: `Severity.WARNING` (advisory only, even inside a codebase centered on monetary calculations, because latencies, timestamps, probabilities, and ratios legitimately use `float`).

---

### 1.5 `DecimalZeroFormattingRule` (`lines 1312–1341`)

- **Trigger**: Walks all `ast.Call` nodes where `func` is `Name("Decimal")` or `Attribute(attr="Decimal")` (unresolved), `len(args) == 1` (keyword arguments are not checked), and `args[0]` is a string `ast.Constant` that `decimal.Decimal(...)` parses to a value equal to zero. Every zero spelling is flagged: `"0"`, `"0.00"`, `"-0"`, `"0E+3"`, `" 0 "`.
- **Message**: ``f"Use `Decimal(0)` instead of `Decimal({arg.value!r})` to represent zero."``

---

## 2. Genuine Problems vs. Noise and Defects

### 2.1 Patterns 1–5 of `BannedTypeAnnotationsRule`: Project-Specific Domain Tuples

- **What problem they solved in the legacy linter**: The legacy linter lacked a general rule against unnamed positional tuple returns. Developers and LLM agents repeatedly returned ad-hoc tuples like `tuple[Width, Height]` or `tuple[HttpClient | FakeClient, Credentials | None]` instead of existing domain dataclasses/NamedTuples (`Dimensions`, `ValueRange`, `MergeInputs`, `ClientSetup`, `PendingRequest`).
- **Why they cannot be ported as-is**: Every single type name (`Width`, `Height`, `Bound`, `Dimensions`, `ValueRange`, `SortOrder`, `HttpClient`, `FakeClient`, `Credentials`, `IPCResponse`, `PendingRequest`) is proprietary to the legacy linter's project domain.
- **What Omni already covers**: [positional_tuple_return.rs](../../../src/code_lint/rules/positional_tuple_return.rs) already bans **all** fixed-length `tuple[T1, T2, ...]` (`arity >= 2`) in return and generator yield/return positions—catching all five shapes (and any other multi-element tuple return) structurally without hardcoding project-specific class names.

---

### 2.2 Pattern 6 of `BannedTypeAnnotationsRule`: `Coroutine[object, object, T]`

- **Why this pattern appeared in the legacy linter**:
  1. In Python's type system (`typeshed`, `mypy`, `pyright`), calling an `async def f() -> T` function produces an inferred type of `Coroutine[Any, Any, T]`.
  2. When a developer or LLM writes a synchronous function that returns an unawaited coroutine (`def fetch() -> Coroutine[Any, Any, Response]: return _async_fetch()`), a callback type (`Callable[[], Coroutine[Any, Any, None]]`), or a coroutine parameter, they copy `Coroutine[Any, Any, T]` from the type checker.
  3. When `Any` is banned by a linter (such as Ruff `ANN401`), the LLM mechanically substitutes `object` for `Any`, producing `Coroutine[object, object, T]`.
- **Defects in the legacy linter's check**:
  1. **Confused remediation**: The legacy linter's message says `"Consider 'Awaitable[...]', 'Coroutine[None, None, ...]', or refactoring."` Suggesting `Coroutine[None, None, T]` over `Coroutine[object, object, T]` is pure bike-shedding: native `async def` coroutines never yield values (`YieldT`) or accept `.send(value)` (`SendT`). Both `object` and `None` are arbitrary placeholders for two unused legacy generator type parameters.
  2. **Type-checker incompatibility with `asyncio.create_task` and `asyncio.run`**: In Python's `typeshed` and at CPython runtime, `asyncio.create_task(coro)`, `TaskGroup.create_task(coro)`, `loop.create_task(coro)`, and `asyncio.run(main)` **require `Coroutine[Any, Any, _T]` and reject `Awaitable[_T]`** (`Argument 1 to "create_task" has incompatible type "Awaitable[T]"; expected "Coroutine[Any, Any, T]"`). If a helper function accepts or returns a coroutine object specifically to pass it to `create_task()`, replacing `Coroutine[..., ..., T]` with `Awaitable[T]` breaks `mypy` and `pyright`.
  3. **Incomplete trigger**: The legacy linter only matched `Coroutine[object, object, ...]`, ignoring `Coroutine[Any, Any, ...]`, `Coroutine[None, None, ...]`, and qualified `collections.abc.Coroutine[...]` or `typing.Coroutine[...]`.
  4. **Missing exemptions**: `AnnotationInspectorRule` only exempted `@override`, missing `@overload`, `@abstractmethod`, `Protocol`, `ABC`, and `TypedDict`.

---

### 2.3 `PrimitiveTypeSuggestionsRule`: Unviable Noise, Already Replaced

- Flagging every `str`, `int`, and `float` annotation in a Python codebase has a near-100% false-positive rate and was already disabled in the legacy linter.
- The high-signal, structurally verifiable subsets of "primitive obsession" are already implemented in Omni:
  - [primitive_duration.rs](../../../src/code_lint/rules/primitive_duration.rs): flags `int` / `float` when the identifier names a time or duration unit (`timeout_ms`, `ttl_seconds`, etc.).
  - [identical_positional_types.rs](../../../src/code_lint/rules/identical_positional_types.rs): flags functions with `>= 2` positional parameters sharing the same type (`def transfer(from_id: str, to_id: str)`), which is where unbranded primitives actually cause silent argument-transposition bugs.
  - [positional_tuple_return.rs](../../../src/code_lint/rules/positional_tuple_return.rs): flags unnamed multi-element tuple returns (`tuple[int, str]`).

---

### 2.4 `FloatToDecimalSuggestionsRule`: Domain-Specific Advisory

- Banning `float` in favor of `Decimal` is specific to monetary domain logic. In general-purpose Python (and even in non-monetary modules of a money-handling system: metrics, latencies, ML weights, `time.monotonic()`, probabilities), `float` is the correct standard type.
- Also had an AST bug in the legacy linter: `_get_type_name` only matched top-level `ast.Name("float")`, missing `float | None`, `Optional[float]`, and `list[float]`.

---

### 2.5 `DecimalZeroFormattingRule`: Semantically Unsound in Python `decimal`

- **Critical semantic bug**: In Python's `decimal` module (IEEE 754 decimal floating-point arithmetic), trailing zeros in a string literal encode the number's **exponent (quantum/scale)**:
  - `Decimal("0.00").as_tuple().exponent == -2` (scale of 2 decimal places).
  - `Decimal(0).as_tuple().exponent == 0` (integer scale).
  - Although `Decimal("0.00") == Decimal(0)` evaluates to `True`, they are **not** interchangeable:
    - `str(Decimal("0.00"))` is `"0.00"`, whereas `str(Decimal(0))` is `"0"`.
    - `amount.quantize(Decimal("0.00"))` rounds `amount` to 2 decimal places, whereas `amount.quantize(Decimal(0))` rounds `amount` to a whole integer!
- Advising developers or LLMs to replace `Decimal("0.00")` with `Decimal(0)` introduces silent precision/rounding bugs in `.quantize(...)` and fixed-scale serialization.
- Even restricted to integer-scale `Decimal("0")`, choosing `Decimal(0)` over `Decimal("0")` is arbitrary micro-style for a single standard-library constructor.

---

## 3. Sub-Pattern Triage Summary

| Sub-Pattern | Source Rule | Triage Decision | Reason |
|---|---|---|---|
| 5 domain tuple shapes (`Dimensions`, `ValueRange`, `MergeInputs`, `ClientSetup`, `PendingRequest`) | `BannedTypeAnnotationsRule` | **Drop (already covered)** | Domain-specific to the legacy linter's project; general structural principle is already enforced by `positional-tuple-return`. |
| User-configurable banned AST type matcher | `BannedTypeAnnotationsRule` | **Drop** | Empty default list violates `every_tag_has_a_rule` and `Simplicity First` (no speculative configurability); `RuleOptions` does not support `(pattern -> message)` maps. |
| `Coroutine[object, object, T]` annotation check | `BannedTypeAnnotationsRule` | **Drop (or narrow to return/callback `Awaitable[T]` only if requested)** | Replacing `Coroutine[_, _, T]` with `Awaitable[T]` breaks `mypy`/`pyright` whenever the coroutine is passed to `asyncio.create_task()` or `asyncio.run()`, which require `Coroutine[Any, Any, T]`. |
| Bare `str` / `int` / `float` warning | `PrimitiveTypeSuggestionsRule` | **Drop (already disabled & superseded)** | Disabled in the legacy linter due to noise; high-signal cases already covered by `primitive-duration`, `identical-positional-types`, and `positional-tuple-return`. |
| `float` → `Decimal` suggestion | `FloatToDecimalSuggestionsRule` | **Drop** | Monetary domain-specific; `float` is standard for non-monetary data. |
| `Decimal("0")` / `Decimal("0.00")` → `Decimal(0)` | `DecimalZeroFormattingRule` | **Drop** | Semantically unsound for `Decimal("0.00")` (destroys IEEE 754 decimal exponent `-2` used by `.quantize()` and `str()`); micro-style for `Decimal("0")`. |
