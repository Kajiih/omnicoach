# Phase 2: SOTA & References — `FakeMustInheritProtocolRule`

This document records **Phase 2 (Gather Resources and Reference)** for porting `FakeMustInheritProtocolRule` to Omni. It builds on [01_understand.md](01_understand.md) (goals G1–G5, non-goals NG1–NG5, candidate decisions D1–D10, and open questions Q1–Q5).

> **Status**: **VALIDATED** (2026-10-03). SOTA linter survey, Python typing & test-double specifications, and internal Omni codebase references complete.

Confidence markers: ✅ verified against official docs/source this session · ⚠️ from recalled docs/source · ❌ searched, not found.

---

## 1. External State of the Art

### 1.1 Linter & Type-Checker Comparison Matrix

| Tool / Specification | Rule / Feature | Enforces `Fake*` Class Inheritance? | What It Actually Checks & How It Relates |
| :--- | :--- | :--- | :--- |
| **Ruff** (`PT`, `PYI`, `B`, `UP`, `N`, `PL`, `RUF`) ✅ | None ❌ | **No** ❌ | - `UP004` (`useless-object-inheritance`) flags `class Foo(object):` in Python 3.<br>- `PYI059` (`generic-not-last-base-class`) checks `Generic` ordering in base class lists.<br>- `PT` (`flake8-pytest-style`) checks `@pytest.fixture`, `@pytest.mark.parametrize`, and `pytest.raises`, not test-double classes.<br>- **Gap**: Ruff has no rule requiring `Fake*` classes to inherit from a `Protocol`, `ABC`, or interface base class. |
| **Pylint** (`classes` checker) ✅ | `W0223` (`abstract-method`), `W0221` (`arguments-differ`), `W0236` (`invalid-overridden-method`), `R0205` (`useless-object-inheritance`) | **No** ❌ | - Pylint validates abstract method implementation (`W0223`) and signature compatibility (`W0221`) **only when a class explicitly inherits from a base class**.<br>- **Key Synergy**: If `class FakeClient:` omits its base class, all of Pylint's inheritance checks (`W0221`, `W0223`, `W0236`) are completely bypassed. Requiring an explicit base class activates Pylint's class hierarchy checks. |
| **`flake8-pytest-style` (`PT`)** ✅ | `PT001`–`PT031` | **No** ❌ | Inspects pytest decorators, assertions, and fixture scopes; does not inspect test-double class definitions. |
| **`wemake-python-styleguide` (`WPS`)** ✅ | `WPS306` (`Found class without a base class`, deprecated/removed) | **No** ❌ | Historically required Python 2-style `class Foo(object):` on *every* class (later inverted/removed in favor of Ruff `UP004`). Never distinguished `Fake*` test doubles or required a `Protocol`/`ABC` contract. |
| **Mypy & Pyright** (PEP 544 & PEP 698) ✅ | `abstract` instantiation check, `override` / `explicit-override` (`reportImplicitOverride`) | **Only when subclassed** ✅ | - Under **PEP 544** (*Explicitly Declaring Implementation*), when `class FakeRepo(RepoProtocol):` explicitly subclasses a `Protocol` or `ABC`, Mypy and Pyright check method signatures at the `FakeRepo` definition site and raise `Cannot instantiate abstract class "FakeRepo" with abstract attribute "..."` whenever `RepoProtocol` adds a method that `FakeRepo` has not implemented—even when test functions are unannotated.<br>- Under **PEP 698** (`@typing.override`), type checkers verify that `@override` methods match a base class member and flag methods that do not exist on any base class. Without an explicit base class on `FakeRepo`, `@override` cannot be used at all. |
| **Rust (`rustc` & Clippy)** ✅ | Nominal trait system (`impl Trait for FakeFoo`) | **N/A (Compiler-enforced at use sites)** ✅ | Rust has no class inheritance syntax (`struct FakeFoo` never lists traits in its header). Because Rust has no duck typing and requires static types in `#[test]` functions, any `FakeFoo` passed to a generic `T: Repo` or `&dyn Repo` boundary must have an explicit `impl Repo for FakeFoo` block checked by `rustc`. Confirms **NG1 / D2**. |

---

### 1.2 Literature & Specification Backing

1. ***Software Engineering at Google*, Chapter 13: Test Doubles** ([abseil.io](https://abseil.io/resources/swe-book/html/ch13.html)) ✅:
   - Recommends **fakes** over mocking frameworks because mocks encourage interaction testing and drift from real behavior.
   - Identifies **fidelity**—conformance of the fake to the real implementation's contract—as the primary requirement for trustworthy fakes: *"A fake must maintain fidelity to the API contracts of the real implementation."*
   - States that reusable fakes should be authored and maintained alongside the real implementation (commonly in a package's `testing.py` or `fakes.py` module, outside `tests/`), directly motivating **`RuleTarget::All` (D3)**.
2. **PEP 544 — Protocols: Structural subtyping (static duck typing), §"Explicitly Declaring Implementation"** ([peps.python.org/pep-0544](https://peps.python.org/pep-0544/#explicitly-declaring-implementation)) ✅:
   - Explicitly defines the semantics of subclassing a `Protocol`:
     - A class may explicitly inherit from a `Protocol` to declare that it implements the protocol's contract.
     - Static type checkers treat unimplemented protocol methods and attributes as abstract on the subclass, disallowing instantiation (`FakeClient()`) until all protocol members are implemented and checking method signatures at the class definition site.
     - Without explicit inheritance, structural checks only happen when an expression typed as `FakeClient` is assigned to a target explicitly annotated as `ClientProtocol`—which frequently does not happen in unannotated pytest test functions or fixtures.
3. **PEP 698 — Override Decorator for Static Typing (`@typing.override`)** ([peps.python.org/pep-0698](https://peps.python.org/pep-0698/)) ✅:
   - Specifies that `@override` requires an explicit base class in the class hierarchy. If a protocol method is renamed or deleted during a refactoring, a fake class that explicitly subclasses the `Protocol` and marks its methods `@override` fails type checking immediately instead of keeping dead methods.

---

## 2. Internal Codebase References & Constraints

### 2.1 Existing Omni `TEST_DOUBLES` Rules

| Rule | File | `RuleTarget` | `Classification` | Connection to `fake-without-protocol` |
| :--- | :--- | :--- | :--- | :--- |
| `mock-in-tests` | [src/code_lint/rules/mock_in_tests.rs](../../../src/code_lint/rules/mock_in_tests.rs) | `RuleTarget::TestsOnly` | `Topic::TEST_DOUBLES`, `Precision::Exact`, `Consensus::Opinionated`, `ImpactedQuality::Reliability` | Suggestion text (L82): `"Inject an in-memory fake (such as FakeRepository or FakeHttpClient) that implements the dependency's Protocol."` `fake-without-protocol` enforces that the resulting `Fake*` class actually inherits from that `Protocol`. |
| `mock-call-assertion` | [src/code_lint/rules/mock_call_assertion.rs](../../../src/code_lint/rules/mock_call_assertion.rs) | `RuleTarget::TestsOnly` | `Topic::TEST_ASSERTIONS`, `Topic::TEST_DOUBLES`, `Precision::Exact`, `Consensus::Opinionated`, `ImpactedQuality::Maintainability` | Suggestion text (L42): `"Assert on the returned value or on the state of an in-memory fake."` |

### 2.2 Existing Class-Level Rules & AST Helpers

| Building Block | Location | Details & Reuse |
| :--- | :--- | :--- |
| `extract_classes` & `PythonClassInfo` | [src/code_lint/ast/python.rs L482–586](../../../src/code_lint/ast/python.rs#L482-L586) | Extracts `name: String`, `name_node: AstNode<'a>`, `bases: Vec<PythonBaseClass<'a>>`, `decorators: Vec<DecoratorInfo<'a>>`, and `node: AstNode<'a>` (the `decorated_definition` or `class_definition`).<br>**Defect to fix (G5 / D10)**: L554 currently filters `superclasses.children()` with `kind != "(" && kind != ")" && kind != "," && kind != "keyword_argument"`. It should also require `child.is_named() && !child.is_extra()` (matching `call_argument_nodes` in [src/code_lint/ast.rs L189](../../../src/code_lint/ast.rs#L189)) so comments inside `class FakeClient(\n # comment\n):` are not pushed into `bases`. |
| `mutable-dataclass` & `unslotted-dataclass` | [src/code_lint/rules/mutable_dataclass.rs](../../../src/code_lint/rules/mutable_dataclass.rs), [src/code_lint/rules/unslotted_dataclass.rs](../../../src/code_lint/rules/unslotted_dataclass.rs) | Reference implementations for `CodeRule` (with `Options = ()`, `RuleTarget::All`) iterating over `extract_classes(file)` and anchoring diagnostics on `&class.name_node` with `&[("class", &class.name)]`. |
| Word-boundary identifier splitting | [src/code_lint/rules/abbreviated_name.rs L37–71](../../../src/code_lint/rules/abbreviated_name.rs#L37-L71) | Splits identifiers on `_` and lowercase/digit-to-uppercase transitions. For `Fake*` class names, checking `name.trim_start_matches('_').strip_prefix("Fake").is_some_and(|rest| rest.is_empty() \|\| !rest.starts_with(\|c: char\| c.is_ascii_lowercase()))` (or a dedicated predicate on `PythonClassInfo`) cleanly handles `FakeClient`, `_FakeClient`, `FakeHTTPClient`, `Fake_Client`, `Fake2FA`, and `Fake` while rejecting `Faker`, `Fakeable`, `FakerProvider`. |
| `DEFAULT_TEST_PATTERNS` | [src/config.rs L16–22](../../../src/config.rs#L16-L22) | Default test path globs are `**/tests/**`, `**/test_*.py`, `**/*_test.py`, `**/*_test.rs`, `**/tests.rs`. Because `src/pkg/testing.py`, `src/pkg/fakes.py`, and top-level `conftest.py` do not match `DEFAULT_TEST_PATTERNS`, `RuleTarget::All` is required so reusable fakes in those modules are linted. |

### 2.3 Registry & Style Validation Constraints ([tests/registry.rs](../../../tests/registry.rs))

1. **Rule name**: `fake-without-protocol` (3 words, kebab-case, no forbidden prefix `no-` / `prefer-` / `enforce-` / `banned-` / `max-` / `min-`, no `-enforced` suffix). File name: `src/code_lint/rules/fake_without_protocol.rs`, exporting `pub const RULE: CodeRule`.
2. **Template fields** (`ViolationTemplate`):
   - `summary`: `"Fake class `{class}` does not inherit from a `Protocol` or base class."`
     - Single sentence ending with `.`, starts with uppercase, uses `{class}` from `PLACEHOLDERS`, contains no fix verb (`use`, `replace`, `add`, `rename`, `remove`) and no judgement word (`banned`, `forbidden`, `discouraged`, `illegal`, `must`).
   - `rationale`: `"A standalone fake class is not checked against its collaborator's contract at definition time, so tests keep passing when the real interface changes."`
     - Single sentence ending with `.`, no `should` / `must`, does not start with a fix verb, explains the concrete failure mode (false-green tests on interface drift).
   - `suggestion`: `"Add the collaborator's `Protocol` or `ABC` as a base class of `{class}`."`
     - Starts with `"Add"` (in `SUGGESTION_VERBS`), ends with `.`, uses `{class}` from `PLACEHOLDERS`.
   - Neither the template nor `RuleDoc` uses any word containing `"comment"` or `"require-explanation"` (enforced by `test_rules_with_a_mode_leave_comment_explanations_to_the_framework`).
3. **`RuleDoc::summary`**:
   - `"Requires Python `Fake*` classes to inherit from a `Protocol` or base class."` (One sentence starting with `Requires ` and ending with `.`).

---

## 3. Trivial / Non-Contract Base Class Analysis (Edge Cases E3 & E4)

When a `Fake*` class has a `superclasses` list (`class FakeClient(...):`), which base class expressions do **not** establish a collaborator interface contract?

1. **`object` / `builtins.object`**:
   - Python 3 implicit root class (flagged as redundant by Ruff `UP004`). Writing `class FakeClient(object):` provides zero interface verification.
2. **`Generic[...]` / `typing.Generic[...]` / `typing_extensions.Generic[...]`**:
   - Declares type parameters (`TypeVar`) for a generic class (`class FakeRepo(Generic[T]):`), equivalent to PEP 695 `class FakeRepo[T]:`. On its own (without a second base class such as `RepoProtocol[T]`), it provides zero interface verification.
3. **`Protocol` / `typing.Protocol` / `typing_extensions.Protocol` / `Protocol[...]`**:
   - Writing `class FakeClient(Protocol):` defines a new `Protocol` rather than a fake implementation, and raises `TypeError: Protocols cannot be instantiated` at runtime when a test calls `FakeClient()`.
   - Therefore, a base whose stripped name (before any `[...]` subscript) is `object`, `builtins.object`, `Generic`, `typing.Generic`, `typing_extensions.Generic`, `Protocol`, `typing.Protocol`, or `typing_extensions.Protocol` does **not** satisfy the requirement for a collaborator interface base class.
4. **Valid interface bases (Pass)**:
   - Any domain `Protocol`, `ABC`, or base class: `class FakeClient(HttpClient):`, `class FakeClient(http.HttpClient):`, `class FakeRepo(Repository[User]):`, `class FakeRepo(Repository[T], Generic[T]):`, `class FakeStorage(BaseFakeStorage):`, `class FakeBaseStorage(ABC):`, `class FakeError(Exception):`.

---

## 4. Sources

- *Software Engineering at Google*, Chapter 13: *Test Doubles* (Fakes, Fidelity, and Maintenance): https://abseil.io/resources/swe-book/html/ch13.html
- PEP 544 — *Protocols: Structural subtyping (static duck typing)*, §"Explicitly Declaring Implementation": https://peps.python.org/pep-0544/#explicitly-declaring-implementation
- PEP 698 — *Override Decorator for Static Typing*: https://peps.python.org/pep-0698/
- Mypy Documentation — *Protocols and structural subtyping*: https://mypy.readthedocs.io/en/stable/protocols.html
- Ruff Rules Catalog (`UP004`, `PYI059`, `PT`): https://docs.astral.sh/ruff/rules/
- Pylint Messages (`W0221`, `W0223`, `W0236`, `R0205`): https://pylint.readthedocs.io/en/stable/user_guide/messages/messages_overview.html
