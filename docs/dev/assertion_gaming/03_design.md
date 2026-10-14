# Test Assertion & Structure Rules (`packed-assertion`, `too-many-assertions`, `logic-in-tests`, `multi-phase-test`): 03 Design

> [!NOTE]
> **Status: IMPLEMENTED** — Minimal, orthogonal design with no duplicated logic between rules.

Markers: **[V]** verified in code, corpus, or a primary source; **[U]** not verified.

---

## 1. Summary of Changes

| Rule | What it checks |
| :--- | :--- |
| `packed-assertion` | Flags top-level `and`/`&&` assertions, comparisons against boolean literal collections of $\ge 2$ elements, and comparisons where **both operands** are composite literals (`Tuple`, `List`, `Set`, `Dict` in Python; `(a, b)` or `[a, b]` / `&[a, b]` in Rust) of $\ge 2$ elements (`assert (a, b) == (1, 2)`, `assert {"a": x, "b": y} == {"a": 1, "b": 2}`, `assert_eq!((a, b), (1, 2))`). |
| `too-many-assertions` | Counts behavioral assertions per test function (`max-assertions = 4`), leading `TEMPLATE.suggestion` with splitting scenarios and `@pytest.mark.parametrize` / `#[rstest]`, and exempting Python `assert <name> is not None` and `assert isinstance(<name>, ...)` when `<name>` is read in a later statement. |
| `logic-in-tests` | Flags control-flow statements and conditional expressions (`if`, `match`, `for`, `while`, `loop`, `try`, and `if`-filtered comprehensions) directly inside outermost test functions (`RuleTarget::TestsOnly`, `RuleOptions::code_rule(())`), without descending into nested functions, classes, or closures/lambdas, and without cascading into children of an already-flagged control-flow construct. |
| `multi-phase-test` | Flags outermost test functions (`RuleTarget::TestsOnly`, `RuleOptions::code_rule(())`) whose top-level statements interleave non-assertion statements between behavioral assertions ($\text{Assert} \to \text{NonAssert} \to \text{Assert}$), treating Python narrowing guards as neutral. |

---

## 2. Decisions (`D1..D6`)

- **D1 — No duplicate packing logic in `too-many-assertions`.**
  `packed-assertion` already flags packed assertions regardless of total assertion count. `too-many-assertions` remains a straightforward statement counter and does not re-implement `and` or container unpacking.

- **D2 — Minimal composite comparison check in `packed-assertion`.**
  - **Python**: In `Stmt::Assert` with top-level `Expr::Compare`, flag when both `left` and a comparator are composite literals (`Tuple`, `List`, `Set`, `Dict`) with $\ge 2$ elements (or when any operand is a boolean literal tuple/list with $\ge 2$ elements).
  - **Rust**: In `assert_eq!` / `assert_ne!` / `debug_assert_eq!` / `debug_assert_ne!`, split the macro `TokenTree` by top-level `,` into `(left, right, ...)`. Flag when both `left` and `right` (after optional `&`) are `(...)` or `[...]` `TokenTree`s with $\ge 2$ comma-separated items (or when either is a collection of $\ge 2$ boolean literals).

- **D3 — Simple Python narrowing-guard exemption in `too-many-assertions` and `multi-phase-test`.**
  - Skip counting `assert <name> is not None` and `assert isinstance(<name>, ...)` when `<name>` (`Expr::Name`) is loaded in a subsequent statement of the test body, and treat those guards as neutral in `multi-phase-test`.

- **D4 — Suggestion wording in `too-many-assertions`.**
  ```rust
  suggestion: {
      Python => "Split distinct scenarios into separate `test_*` functions, parameterize variations with `@pytest.mark.parametrize`, or compare one domain object directly.",
      Rust => "Split distinct scenarios into separate `#[test]` functions, parameterize cases with `#[rstest]`, or compare one domain struct directly.",
  }
  ```

- **D5 — `logic-in-tests` scope and non-cascading traversal.**
  - Inspects only outermost test functions (`is_test_function_def` in Python; `collect_outer_test_functions_rec` in Rust).
  - Stops at nested `def` / `class` / `lambda` in Python and nested `fn` / closure / `Item` in Rust so fake implementations or callbacks defined inside a test can use normal control flow.
  - Does not recurse into the body of a flagged `if` / `match` / `for` / `while` / `loop` / `try` construct so a single control-flow block produces one diagnostic rather than cascading diagnostics on inner branches.
  - Uses `RuleOptions::code_rule(())`; rare legitimate cases use `# omni:ignore[logic-in-tests] -- <reason>`.

- **D6 — `multi-phase-test` top-level statement state machine.**
  - Walks the direct top-level statements of each outermost test function body (plus the optional `tail_expr` in Rust).
  - Each statement is classified as **Neutral** (Python narrowing guard), **Assertion** (contains at least one behavioral assertion outside nested functions/classes), or **Non-assertion**.
  - Reports the test function once when an **Assertion** statement follows a **Non-assertion** statement that itself followed an earlier **Assertion** statement ($\text{Assert} \to \text{NonAssert} \to \text{Assert}$).

