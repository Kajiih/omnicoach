# Assertion-Count Gaming (`packed-assertion` & `too-many-assertions`): 03 Design

> [!NOTE]
> **Status: PENDING** — Minimal, orthogonal design with no duplicated logic between rules.

Markers: **[V]** verified in code, corpus, or a primary source; **[U]** not verified.

---

## 1. Summary of Changes

| Rule | What changes |
| :--- | :--- |
| `packed-assertion` | Generalize the existing boolean-collection check (`assert (a, b) == (True, False)`) to flag comparisons where **both operands** are composite literals (`Tuple`, `List`, `Set`, `Dict` in Python; `(a, b)` or `[a, b]` / `&[a, b]` in Rust) of $\ge 2$ elements (`assert (a, b) == (1, 2)`, `assert {"a": x, "b": y} == {"a": 1, "b": 2}`, `assert_eq!((a, b), (1, 2))`), while continuing to flag single-sided boolean literal collections (`assert flags == (True, False)`). Single-value comparisons (`assert dimensions == (1920, 1080)`, `assert event["data"] == {"id": 1, "ok": True}`) have one expression on the left and pass. |
| `too-many-assertions` | Keep the assertion counter simple (no duplicate packing logic):<br>1. Reorder `TEMPLATE.suggestion` to lead with splitting scenarios and `@pytest.mark.parametrize` / `#[rstest]` before single-object comparison.<br>2. Exempt Python `assert <name> is not None` and `assert isinstance(<name>, ...)` when `<name>` is read in a later statement. |

---

## 2. Decisions (`D1..D4`)

- **D1 — No duplicate packing logic in `too-many-assertions`.**
  `packed-assertion` already flags packed assertions regardless of total assertion count. `too-many-assertions` remains a straightforward statement counter and does not re-implement `and` or container unpacking.

- **D2 — Minimal composite comparison check in `packed-assertion`.**
  - **Python**: In `Stmt::Assert` with top-level `Expr::Compare`, flag when both `left` and a comparator are composite literals (`Tuple`, `List`, `Set`, `Dict`) with $\ge 2$ elements (or when any operand is a boolean literal tuple/list with $\ge 2$ elements).
  - **Rust**: In `assert_eq!` / `assert_ne!` / `debug_assert_eq!` / `debug_assert_ne!`, split the macro `TokenTree` by top-level `,` into `(left, right, ...)`. Flag when both `left` and `right` (after optional `&`) are `(...)` or `[...]` `TokenTree`s with $\ge 2$ comma-separated items (or when either is a collection of $\ge 2$ boolean literals).

- **D3 — Simple Python narrowing-guard exemption in `too-many-assertions`.**
  - Skip counting `assert <name> is not None` and `assert isinstance(<name>, ...)` when `<name>` (`Expr::Name`) is loaded in a subsequent statement of the test body.

- **D4 — Suggestion wording in `too-many-assertions`.**
  ```rust
  suggestion: {
      Python => "Split distinct scenarios into separate `test_*` functions, parameterize variations with `@pytest.mark.parametrize`, or compare one domain object directly.",
      Rust => "Split distinct scenarios into separate `#[test]` functions, parameterize cases with `#[rstest]`, or compare one domain struct directly.",
  }
  ```
