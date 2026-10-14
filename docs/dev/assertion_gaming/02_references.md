# Assertion-Count Gaming (`too-many-assertions`): 02 References

> [!NOTE]
> **Status: PENDING** (awaiting user review of `01_understand.md`–`03_design.md`).

Markers: **[V]** verified in code, corpus, or a primary source; **[U]** not verified.

---

## 1. Prior Art & Literature

| Source | What it does | Relevance |
| :--- | :--- | :--- |
| **Ruff `PT018`** (`pytest-composite-assertion`) [V] | Flags `assert a and b` and `assert not (a or b)` in `pytest` tests (`https://docs.astral.sh/ruff/rules/pytest-composite-assertion/`). | Covers `and` in Python, but not tuple/dict/comprehension packing or Rust. |
| **PMD `JUnitTestContainsTooManyAsserts` / Sonar `RSPEC-5961`** [V] | Cap raw assertion statements per test method. | Raw statement counting treats `assert (a, b, c, d, e) == (1, 2, 3, 4, 5)` as `1` and penalizes type-narrowing guards. |
| **Legacy linter (`MaxAssertionsInTestRule`, `legacy_lints.py:1444–1474`)** [V] | Capped `ast.Assert` nodes at `4` and warned in prose not to group unrelated asserts into a tuple. | Had no AST logic for logical assertion counting or narrowing guards. |
| **Software Engineering at Google, ch. 12** [V] | One behavior per test; split distinct scenarios or compare one domain object (`https://abseil.io/resources/swe-book/html/ch12.html`). | Motivates leading with splitting/parameterizing in `too-many-assertions`'s suggestion and counting `assert result == Expected(...)` as `1`. |
| **Python Typing Spec: Narrowing (`is not None`, `isinstance`)** [V] | Standard narrowing forms required before using `T \| None` or union values (`https://typing.python.org/en/latest/spec/narrowing.html`). | Motivates exempting `assert x is not None` and `assert isinstance(x, T)` when `x` is read in a subsequent statement. |
