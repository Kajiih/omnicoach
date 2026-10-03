# Phase 1: Understand — `FakeMustInheritProtocolRule` (`fake-without-protocol`)

This document records **Phase 1 (Understand)** for porting the legacy linter's `FakeMustInheritProtocolRule` ([legacy_lints.py](../../../scratch/legacy_reference/legacy_lints.py#L998-L1014)) to Omni as `fake-without-protocol`.

> **Status**: **VALIDATED** (2026-10-03). All Goals (G1–G5), Non-Goals (NG1–NG5), Decisions (D1–D10), and Questions (Q1–Q5) validated.

---

## 1. Context & Problem Statement

In the legacy linter ([legacy_lints.py L998–1014](../../../scratch/legacy_reference/legacy_lints.py#L998-L1014)), `FakeMustInheritProtocolRule` is an error-level AST rule that inspects Python `ast.ClassDef` nodes:

```python
class FakeMustInheritProtocolRule(LintRule):
    """Enforces that fake classes (named Fake*) explicitly inherit from an interface or Protocol."""

    node_types = (ast.ClassDef,)

    @override
    def check(self, node: ast.AST) -> None:
        if not isinstance(node, ast.ClassDef):
            return

        if node.name.startswith("Fake") and not node.bases:
            self.visitor.report(
                node.lineno,
                f"Fake class `{node.name}` must explicitly inherit "
                "from an interface or Protocol class.",
            )
```

### 1.1 How This Rule Complements `mock-in-tests` and `mock-call-assertion`

Omni currently ships two rules under `Topic::TEST_DOUBLES`:

1. **`mock-in-tests`** ([mock_in_tests.rs](../../../src/code_lint/rules/mock_in_tests.rs)): Bans dynamic mocks and monkeypatching (`Mock`, `MagicMock`, `AsyncMock`, `patch`, `mocker.*`, `monkeypatch.*`) in tests. Its diagnostic suggestion explicitly directs developers and AI agents to:
   > *"Inject an in-memory fake (such as `FakeRepository` or `FakeHttpClient`) that implements the dependency's `Protocol`."*
2. **`mock-call-assertion`** ([mock_call_assertion.rs](../../../src/code_lint/rules/mock_call_assertion.rs)): Bans mock interaction assertions (`assert_called_once_with`, etc.) in favor of asserting on return values or the state of an in-memory fake.

However, under [Rule Design Guide §3](../rule_design_guide.md) (*Anticipate Perverse Incentives*), when a developer or AI agent replaces `MagicMock()` to satisfy `mock-in-tests`, the path of least resistance is to write a bare, standalone duck-typed class with **no base class**:

```python
class FakeUserRepository:
    def __init__(self) -> None:
        self.users: dict[str, User] = {}

    def save(self, user: User) -> None:
        self.users[user.id] = user
```

(In fact, [mock_in_tests.rs L170–181](../../../src/code_lint/rules/mock_in_tests.rs#L170-L181) currently uses a base-less `class FakeUserRepository:` in its own `state_based_fake_repository` pass case!)

When a `Fake*` class does **not** explicitly inherit from the dependency's `Protocol` or `ABC` (`class FakeUserRepository(UserRepository):`), it reintroduces the exact **false-green test hazard** that `mock-in-tests` was designed to prevent:

1. **No definition-site or instantiation-site contract check**: Under PEP 544 structural subtyping, a class that does not inherit from a `Protocol` is only checked at *typed call sites*. In pytest suites, test functions and fixtures are frequently unannotated (`def test_register_user(repo):`), or a test constructs `FakeUserRepository()` and exercises only a subset of methods. If `UserRepository` adds a new required method or changes a method signature, `class FakeUserRepository:` without a base class passes static type checking at its definition and instantiation sites.
2. **Explicit `Protocol` / `ABC` subclassing triggers immediate static verification**: When `class FakeUserRepository(UserRepository):` explicitly subclasses a `Protocol` (PEP 544 *"Explicitly Declaring Implementation"*) or an `ABC`, Mypy and Pyright treat any unimplemented protocol/abstract members as abstract on `FakeUserRepository`, immediately flagging `FakeUserRepository()` with `Cannot instantiate abstract class "FakeUserRepository" with abstract attribute "..."` and checking method signature compatibility at the fake's definition site—even when every test function is unannotated.
3. **Enables `@override` (PEP 698)**: In Python 3.12+, `@typing.override` on fake methods catches stale methods when a protocol method is renamed or removed. Type checkers reject `@override` on methods of a class that has no base class.

Together, `mock-in-tests`, `mock-call-assertion`, and `fake-without-protocol` form a complete **test-double fidelity triad**:
- `mock-in-tests`: Replace dynamic mocks/patches with an in-memory fake.
- `mock-call-assertion`: Assert on observable outputs or fake state, not call wiring.
- `fake-without-protocol`: Ensure the fake explicitly subclasses the `Protocol` or `ABC` it stands in for, so the type checker enforces contract parity.

---

### 1.2 Edge Cases & Blind Spots in the legacy linter's Implementation

The legacy linter's check (`node.name.startswith("Fake") and not node.bases`) has several blind spots and edge cases:

| # | Edge Case / Blind Spot | The legacy linter Behavior | Analysis & Desired Behavior in Omni |
| :--- | :--- | :--- | :--- |
| **E1** | **Word boundary in class names (`Fake` vs `FakeClient` vs `Faker` / `Fakeable` / `Fakeaw`)** | `node.name.startswith("Fake")` flags `class Faker:`, `class FakerProvider:`, `class Fakeable:`, `class Fakeout:`. | `"Fake"` in `Faker` or `Fakeable` is part of a longer word (`"faker"`, `"fakeable"`), not the word `"Fake"`. Conversely, private module-internal fakes `_FakeClient` or underscore-separated `Fake_Client` start with `_` or use `_` as a separator. Matching checks that `"Fake"` (after stripping any leading `_`) is followed by an uppercase letter (`FakeClient`, `FakeHTTPClient`), digit (`Fake2FA`), `_` (`Fake_Client`), or end-of-identifier (`Fake`). |
| **E2** | **Bare `class Fake:` vs `class FakeClient:`** | Flags `class Fake:`. | A class named literally `Fake` (or `_Fake`) with no base class is either an ad-hoc generic fake object or a base-less fake helper; both lack an interface contract. |
| **E3** | **Trivial / non-contract base classes (`class FakeClient(object):`, `Generic[T]`)** | `node.bases` is non-empty (`[Name("object")]`), so the legacy linter **fails to flag** `class FakeClient(object):` or `class FakeRepo(Generic[T]):`. | `object` (`builtins.object`) is the implicit root of every Python 3 class and provides zero interface contract. Similarly, `Generic[T]` (`typing.Generic[T]`, `typing_extensions.Generic[T]`) only declares type variables, not a collaborator interface. A `Fake*` class whose base list contains *only* these structural markers has no interface/protocol base and must still be flagged. |
| **E4** | **Self-declared `Protocol` (`class FakeClient(Protocol):`)** | `node.bases` is non-empty (`[Name("Protocol")]`), so the legacy linter passes it. | A `Protocol` cannot be instantiated (`TypeError: Protocols cannot be instantiated`). If a developer writes `class FakeClient(Protocol):` (or `typing.Protocol`, `typing_extensions.Protocol`) as the *only* base class, they have declared a new protocol rather than implementing an existing one; it must be flagged (validated in Q2). |
| **E5** | **Metaclass / keyword-only class arguments (`class FakeClient(metaclass=ABCMeta):`)** | In Python's `ast`, `keywords` is separate from `bases`, so `not node.bases` flags it. | In `tree-sitter-python`, `superclasses` is an `argument_list` containing both base expressions and `keyword_argument` nodes. Omni's `extract_classes` ([src/code_lint/ast/python.rs L550–561](../../../src/code_lint/ast/python.rs#L550-L561)) already filters out `keyword_argument`, `(`, `)`, and `,`. |
| **E6** | **Comments inside `class FakeClient(\n # comment\n):`** | Python `ast` strips comments. | In `tree-sitter-python`, a comment inside `superclasses` (`argument_list`) is a named extra child (`comment`). Currently, `extract_classes` in [src/code_lint/ast/python.rs L554](../../../src/code_lint/ast/python.rs#L554) checks `kind != "(" && kind != ")" && kind != "," && kind != "keyword_argument"` without checking `!child.is_extra()`, so a comment inside `class FakeClient(# comment\n):` would be misclassified as a `PythonBaseClass` unless `extract_classes` filters with `child.is_named() && !child.is_extra()`. |
| **E7** | **Record / data fixture classes (`@dataclass class FakeConfig:`, `TypedDict`, `NamedTuple`)** | Flags `@dataclass class FakeUser:` if it has no base class; passes `class FakeUser(NamedTuple):` and `class FakeUser(TypedDict):`. | Stateful fakes in Python very commonly use `@dataclass` to hold their in-memory state (`@dataclass class FakeUserRepository(UserRepository):`). Therefore `@dataclass` classes named `Fake*` are checked just like any other `Fake*` class (validated in Q3). |
| **E8** | **File scope (`RuleTarget::All` vs `RuleTarget::TestsOnly`)** | The legacy linter runs `FakeMustInheritProtocolRule` on **all** files (`src/` and `tests/`). | In Omni, `DEFAULT_TEST_PATTERNS` ([src/config.rs L16–22](../../../src/config.rs#L16-L22)) only matches `**/tests/**`, `**/test_*.py`, and `**/*_test.py`. It does **not** match root `conftest.py`, `src/pkg/testing.py`, `src/pkg/fakes.py`, or `src/pkg/testing/fakes.py`—where reusable library fakes are canonically placed (*Software Engineering at Google*, Ch. 13: *"Fakes should be authored and maintained by the team that owns the real implementation"*). Moreover, a base-less `Fake*` class is never valid in production code either. Thus `RuleTarget::All` is required so fakes in `testing.py` / `fakes.py` / `conftest.py` are checked. |
| **E9** | **Language scope (Python vs Rust)** | Python-only. | In Rust, structs have no inheritance syntax (`struct FakeClient` never declares base traits in its header; traits are implemented via separate `impl HttpClient for FakeClient` blocks). Furthermore, Rust's static type system is strictly nominal even in tests: a `FakeClient` cannot be passed to a function expecting `impl HttpClient` or `&dyn HttpClient` without an explicit `impl HttpClient for FakeClient` block verified by `rustc`. A Rust equivalent is therefore both syntactically inapplicable and semantically redundant. |

---

## 2. Goals & Non-Goals

### 2.1 Goals

| ID | Goal | Reason |
| :--- | :--- | :--- |
| **G1** | Flag Python `Fake*` classes that do not explicitly inherit from an interface, `Protocol`, or base class. | Closes the false-green test-double loop alongside `mock-in-tests` and `mock-call-assertion` (§1.1). |
| **G2** | Eliminate the legacy linter's word-boundary false positives (`Faker`, `Fakeable`) and trivial-base false negatives (`class FakeClient(object):`, `class FakeClient(Generic[T]):`, `class FakeClient(Protocol):`). | High signal-to-noise ratio (§1.2 E1, E3, E4); [Rule Design Guide §3](../rule_design_guide.md). |
| **G3** | Check `Fake*` classes across all Python files (`RuleTarget::All`), including `fakes.py`, `testing.py`, `conftest.py`, and `tests/`. | Reusable fakes frequently live outside `test_*.py` / `tests/` (§1.2 E8). |
| **G4** | Provide orthogonal, actionable `summary`, `rationale`, and `suggestion` messages conforming to [Naming and Message Style Guide](../naming_and_message_style_guide.md) and validated by [tests/registry.rs](../../../tests/registry.rs). | [Rule Design Guide §2](../rule_design_guide.md). |
| **G5** | Fix the `extract_classes` comment/extra-node blind spot in [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) so comments inside `class Foo(# comment\n):` are not treated as base classes. | AST robustness (§1.2 E6). |

### 2.2 Explicit Non-Goals

| ID | Non-Goal | Reason |
| :--- | :--- | :--- |
| **NG1** | **Rust language support** (`SupportLang::Rust`). | Rust has no class inheritance syntax (`impl Trait for FakeFoo` is separate from `struct FakeFoo`), and `rustc`'s nominal trait system already enforces trait implementation completeness and signature compatibility at compile time (§1.2 E9). |
| **NG2** | **Cross-file type resolution** to prove that a declared base class in `class FakeClient(HttpClient):` is specifically a `typing.Protocol` or `abc.ABC`. | Omni rules operate per-file on Tree-sitter CSTs without cross-file symbol resolution. Inheriting from a concrete base class, `ABC`, or `Protocol` all provide static type-checker verification of method signatures and `@override`. |
| **NG3** | **Enforcing that non-`Fake` production classes inherit from a `Protocol`** (the legacy linter's separate `RequireExplicitProtocolInheritanceRule`, L1113–1186). | Separate rule with a different mechanism (whole-repo protocol method-signature matching) and different rationale ([Rule Design Guide §1](../rule_design_guide.md) split test). |
| **NG4** | **Enforcing `@override` on methods of `Fake*` classes**. | Separate antipattern covered by type checkers (`mypy --enable-error-code=explicit-override`, Pyright `reportImplicitOverride`) or a dedicated `@override` rule. |
| **NG5** | **Automatic code fixes (`--fix`)**. | The linter cannot know which `Protocol` or `ABC` a standalone `Fake*` class was intended to implement. |

---

## 3. Validated Decisions (D1–D10)

| ID | Decision | Rationale |
| :--- | :--- | :--- |
| **D1** | **Rule name**: `fake-without-protocol` (validated in Q1). | Complies with [Naming and Message Style Guide §1](../naming_and_message_style_guide.md): kebab-case, ≤ 4 words, no polarity prefix (`no-`, `prefer-`, `enforce-`), names the flagged pattern rather than the policy, no `-in-tests` suffix because `RuleTarget` is `All`. |
| **D2** | **Languages**: `&[SupportLang::Python]`. | NG1 / §1.2 E9. |
| **D3** | **Target scope**: `RuleTarget::All`. | Fakes routinely live in `testing.py`, `fakes.py`, and root `conftest.py`, which are outside `DEFAULT_TEST_PATTERNS`, and base-less `Fake*` classes are never valid in production code (§1.2 E8). |
| **D4** | **Class name matching**: Match any Python class whose name (after stripping leading underscores `_`) starts with `"Fake"` at a word boundary—specifically, `"Fake"` followed by end-of-string (`Fake`), `_` (`Fake_Client`), an ASCII uppercase letter (`FakeClient`, `FakeHTTPClient`), or an ASCII digit (`Fake2FA`). Equivalently, `rest.is_empty() \|\| !rest.starts_with(\|c: char\| c.is_ascii_lowercase())`. | Flags `FakeClient`, `_FakeClient`, `FakeHTTPClient`, `Fake_Client`, `Fake2FA`, and `Fake`, while exempting words that merely share the letters `F-a-k-e` (`Faker`, `FakerProvider`, `Fakeable`, `Fakeout`). |
| **D5** | **Base class requirement & trivial-base filtering**: A matched `Fake*` class is flagged unless it has at least one meaningful base class expression after excluding trivial non-interface bases (`object`, `builtins.object`, `Generic`, `typing.Generic`, `typing_extensions.Generic`, and `Protocol`, `typing.Protocol`, `typing_extensions.Protocol`, with or without `[...]` type arguments — validated in Q2). | Prevents `class FakeClient(object):`, `class FakeRepo(Generic[T]):`, or `class FakeClient(Protocol):` from bypassing the check without implementing an interface contract (§1.2 E3, E4). `ABC` / `abc.ABC` and any domain base class are allowed. PEP 695 generic syntax `class FakeRepo[T]:` has no `superclasses` node unless `(Base)` is also written, so `class FakeRepo[T]:` is naturally flagged and `class FakeRepo[T](Repo[T]):` naturally passes. |
| **D6** | **Diagnostic span & placeholder**: Anchor the diagnostic on `class.name_node` with placeholder `("class", &class.name)`, matching `mutable-dataclass` and `unslotted-dataclass`. | Consistent with all existing class-header rules in Omni; `RuleOptions::code_rule(())` `require-explanation` comments (when configured) above the class or decorator block work out of the box. |
| **D7** | **Options**: `RuleOptions::code_rule(())` (default `EnforcementMode::Ban`, no count/list options — validated in Q4). | Matches `mutable-dataclass`, `unslotted-dataclass`, `mutable-module-constant`, and the legacy linter's `_ERROR_RULES`. Teams that want comment-based exemptions can set `enforcement-mode = "require-explanation"` in `.omnilint.toml`. |
| **D8** | **Classification**:<br>- `topics: &[Topic::TEST_DOUBLES]`<br>- `precision: Precision::Heuristic`<br>- `consensus: Consensus::Opinionated`<br>- `impacted_quality: ImpactedQuality::Reliability` | - `Topic::TEST_DOUBLES`: directly governs fake test doubles alongside `mock-in-tests` and `mock-call-assertion`.<br>- `Precision::Heuristic`: matches `primitive-duration` and `type-suffixed-name`, because the `Fake*` prefix is a naming proxy for a test-double collaborator and any non-trivial base class is a syntactic proxy for an interface/protocol.<br>- `Consensus::Opinionated`: PEP 544 allows structural subtyping without explicit inheritance.<br>- `ImpactedQuality::Reliability`: prevents false-green tests when a collaborator's `Protocol` or `ABC` changes and unannotated tests/fixtures use a drifted `Fake*` class. |
| **D9** | **Message template** (`ViolationTemplate`):<br>- `summary`: `"Fake class `{class}` does not inherit from a `Protocol` or base class."`<br>- `rationale`: `"A standalone fake class is not checked against its collaborator's contract at definition time, so tests keep passing when the real interface changes."`<br>- `suggestion`: `"Add the collaborator's `Protocol` or `ABC` as a base class of `{class}`."` | Strictly orthogonal What / Why / How ([Rule Design Guide §2](../rule_design_guide.md)). Uses `{class}` from §3 vocabulary and starts `suggestion` with `Add` (in `SUGGESTION_VERBS`, [Naming and Message Style Guide §2.4](../naming_and_message_style_guide.md)). |
| **D10** | **AST fix in `extract_classes`**: Filter `superclasses.children()` with `child.is_named() && !child.is_extra() && child.kind() != "keyword_argument"`. | Prevents comments (`# comment`) or line continuations inside `class FakeFoo(\n    # comment\n):` from being collected as base classes (§1.2 E6). |

---

## 4. Resolved Questions (Q1–Q5)

1. **Q1 (Rule Name)**: **Validated — `fake-without-protocol`** (Option A).
2. **Q2 (`Protocol` as the *Direct* Base Class of `Fake*`)**: **Validated** — exclude `object` (`builtins.object`), `Generic` (`typing.Generic`, `typing_extensions.Generic`), and `Protocol` (`typing.Protocol`, `typing_extensions.Protocol`) from counting as a meaningful base class on a `Fake*` class. Allow `ABC` / `abc.ABC` and any other base class.
3. **Q3 (Dataclasses / Record Types Named `Fake*`)**: **Validated** — `@dataclass` classes named `Fake*` without a meaningful base class are flagged just like any other `Fake*` class.
4. **Q4 (Options & Default `EnforcementMode`)**: **Validated** — `RuleOptions::code_rule(())` (default `EnforcementMode::Ban`, no count/list options).
5. **Q5 (Updating `mock-in-tests` Test Case Snippet)**: **Validated** — add a base class to `class FakeUserRepository(UserRepository):` in [src/code_lint/rules/mock_in_tests.rs](../../../src/code_lint/rules/mock_in_tests.rs) during Phase 4 so Omni's own test suite models the rule.
