# Assertion-Count Gaming (`packed-assertion` & `too-many-assertions`): 01 Understand

> [!NOTE]
> **Status: PENDING** (awaiting user review). Scope: keep `packed-assertion` and `too-many-assertions` simple and orthogonal without duplicating logic between them.

Markers: **[V]** verified in code, corpus, or a primary source; **[U]** not verified.

---

## 1. Current Behavior on `main`

- **`packed-assertion`** ([packed_assertion.rs](../../../src/code_lint/rules/packed_assertion.rs)) [V]:
  - Flags top-level `and` / `&&` in `assert` / `assert!` and comparisons against a tuple/list/array of $\ge 2$ **boolean literals** (`assert (a, b) == (True, False)`).
  - Does **not** flag non-boolean tuple/list/dict comparisons (`assert (a, b) == (1, 2)`, `assert {"a": x, "b": y} == {"a": 1, "b": 2}`).
- **`too-many-assertions`** ([too_many_assertions.rs](../../../src/code_lint/rules/too_many_assertions.rs)) [V]:
  - Counts `1` per assertion in a test function (default `max-assertions = 4`).
  - Its suggestion lists `"Assert on one expected value or object"` first, before splitting scenarios or parameterizing variations.

---

## 2. What Needs to Change (Keeping Each Rule Simple and Orthogonal)

1. **`packed-assertion` owns packed assertions (regardless of assertion count)**:
   - Packing multiple checks into `assert (a, b) == (1, 2)` or `assert {"a": x, "b": y} == {"a": 1, "b": 2}` is bad even when a test only has 2 checks, and is the main way authors/agents bypass `too-many-assertions`.
   - Because `packed-assertion` already exists for this exact purpose, `too-many-assertions` should **not** duplicate packed-assertion detection by counting tuple/dict/conjunction elements.
   - Instead, `packed-assertion` simply checks whether **both sides** of `==` / `!=` (or `assert_eq!` / `assert_ne!`) are composite literals (`Tuple`, `List`, `Set`, `Dict` in Python; tuple or array/slice in Rust) of $\ge 2$ elements. Single-value comparisons (`assert result == (1920, 1080)`, `assert event["data"] == {"id": 1, "ok": True}`) have a single expression on one side and pass with zero special cases.
2. **`too-many-assertions` stays a simple counter**:
   - Reorder `TEMPLATE.suggestion` so splitting distinct scenarios into separate test functions and parameterizing variations (`@pytest.mark.parametrize` / `#[rstest]`) come first.
   - Optionally exempt simple Python type-narrowing guards (`assert x is not None`, `assert isinstance(x, T)`) so type narrowing is not penalized.
