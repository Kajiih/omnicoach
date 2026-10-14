# Test Assertion & Structure Rules: 02 References

> [!NOTE]
> **Status: IMPLEMENTED**.

Markers: **[V]** verified in code, corpus, or a primary source; **[U]** not verified.

---

## 1. Prior Art & Literature

| Source | What it does | Relevance |
| :--- | :--- | :--- |
| **Ruff `PT018`** (`pytest-composite-assertion`) [V] | Flags `assert a and b` and `assert not (a or b)` in `pytest` tests (`https://docs.astral.sh/ruff/rules/pytest-composite-assertion/`). | Covers `and` in Python, but not tuple/dict packing or Rust. |
| **PMD `JUnitTestContainsTooManyAsserts` / Sonar `RSPEC-5961`** [V] | Cap raw assertion statements per test method. | Raw statement counting treats `assert (a, b, c, d, e) == (1, 2, 3, 4, 5)` as `1` and penalizes type-narrowing guards. |
| **Software Engineering at Google, ch. 12: Unit Testing** [V] | Recommends one behavior per test (*Test behaviors, not methods*), straight-line test bodies without branches or loops (*Don't Put Logic in Tests*), and a single Arrange-Act-Assert phase per test (*Structure Tests to Emphasize Behaviors*) (`https://abseil.io/resources/swe-book/html/ch12.html`). | Primary reference for `too-many-assertions`, `logic-in-tests`, and `multi-phase-test`. |
| **Ethan Carlsson, *Don't Write Conditional Logic in Tests*** [V] | Shows how `if`, `switch`/`match`, and filtered comprehensions inside tests hide vacuous passes and duplicate production logic (`https://dev.to/ethancarlsson/dont-write-conditional-logic-in-tests-5aa`). | Motivates flagging filtered comprehensions (`[x for x in items if ...]`) and branching constructs in `logic-in-tests`. |
| **Python Typing Spec: Narrowing (`is not None`, `isinstance`)** [V] | Standard narrowing forms required before using `T \| None` or union values (`https://typing.python.org/en/latest/spec/narrowing.html`). | Motivates exempting `assert x is not None` and `assert isinstance(x, T)` when `x` is read in a subsequent statement in both `too-many-assertions` and `multi-phase-test`. |

