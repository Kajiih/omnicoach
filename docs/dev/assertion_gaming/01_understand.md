# Test Assertion & Structure Rules (`packed-assertion`, `too-many-assertions`, `logic-in-tests`, `multi-phase-test`): 01 Understand

> [!NOTE]
> **Status: IMPLEMENTED**. Scope: keep each test rule simple and orthogonal without duplicating logic between rules.

Markers: **[V]** verified in code, corpus, or a primary source; **[U]** not verified.

---

## 1. Problem Space

1. **Assertion packing (`packed-assertion`)** [V]:
   - Packing multiple checks into `assert (a, b) == (1, 2)` or `assert {"a": x, "b": y} == {"a": 1, "b": 2}` hides which value failed and bypasses `too-many-assertions`.
   - `packed-assertion` checks whether **both sides** of `==` / `!=` (or `assert_eq!` / `assert_ne!`) are composite literals (`Tuple`, `List`, `Set`, `Dict` in Python; tuple or array/slice in Rust) of $\ge 2$ elements. Single-value comparisons (`assert result == (1920, 1080)`, `assert event["data"] == {"id": 1, "ok": True}`) have a single expression on one side and pass with zero special cases.
2. **Assertion counting (`too-many-assertions`)** [V]:
   - Counts `1` per behavioral assertion in a test function (default `max-assertions = 4`), exempting Python type-narrowing guards (`assert x is not None`, `assert isinstance(x, T)`) when `x` is read in a subsequent statement.
   - Leads its suggestion with splitting distinct scenarios into separate test functions and parameterizing variations (`@pytest.mark.parametrize` / `#[rstest]`) before single-object comparison.
3. **Control-flow logic in tests (`logic-in-tests`)** [V]:
   - Loops (`for`, `while`, `loop`), conditionals (`if`, `match`, ternary `a if cond else b`), `try` blocks, and `if`-filtered comprehensions (`[x for x in items if ...]`) inside test functions hide bugs (such as vacuous passes over empty collections), obscure expected values, and bypass `too-many-assertions` by running assertions in a loop.
   - Rare legitimate exceptions (such as multi-step concurrency stress tests) use `# omni:ignore[logic-in-tests] -- <reason>`, which is audited by `missing-suppression-reason` and `stale-suppression`.
4. **Multi-phase tests (`multi-phase-test`)** [V]:
   - Interleaving actions and assertions ($\text{Assert} \to \text{Action} \to \text{Assert}$) in a single test function couples multiple scenarios into one test, making failures harder to localize and violating the single Arrange-Act-Assert structure.
