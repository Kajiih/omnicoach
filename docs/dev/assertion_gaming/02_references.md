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
| **Gerard Meszaros, *xUnit Test Patterns: Chained Tests*** [V] | Defines the *Chained Tests* smell where one test calls another test or relies on it as a setup step, coupling failure blast radius (`http://xunitpatterns.com/Chained%20Tests.html`). | Primary reference for `test-calls-test` alongside *Software Engineering at Google*, ch. 12 (*Shared Setup*). |
| **pytest Good Practices: Conventions for Python Test Discovery** [V] | Documents that `pytest` collects every module-level `test_*` function and every `test_*` method on `Test*` classes as an independent test item (`https://docs.pytest.org/en/stable/explanation/goodpractices.html#conventions-for-python-test-discovery`). | Explains why a helper named `test_*` is collected and executed twice when called from another test, motivating `test-calls-test`. |
| **Martin Fowler, *Eradicating Non-Determinism in Tests* / *Software Engineering at Google*, ch. 14: Larger Tests** [V] | Identifies polling and scheduler-yield loops in tests as non-deterministic synchronization and recommends state-based barriers, channels, or condition variables (`https://martinfowler.com/articles/nonDeterminism.html`, `https://abseil.io/resources/swe-book/html/ch14.html`). | Primary references for `yield-loop-in-tests`. Neither Ruff (`flake8-async` `ASYNC110` only checks `while` + `asyncio.sleep`), Clippy, nor Pylint covers scheduler-yield loops (`asyncio.sleep(0)`, `trio.sleep(0)`, `anyio.sleep(0)`, `os.sched_yield()`, `thread::yield_now()`, `tokio::task::yield_now()`). |
