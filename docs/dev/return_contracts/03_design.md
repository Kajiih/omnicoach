# Return contracts: 03 Design

> **Status: VALIDATED** (lead & user, 2026-10-13).

## 1. Rules

| Rule | Languages | Flags | Default mode |
| :--- | :--- | :--- | :--- |
| `positional-tuple-return` | Python, Rust | A function that returns or yields a positional tuple of at least `min-elements` (default 2) elements. | `ban` |
| `non-boolean-predicate` | Python, Rust | A function named like a predicate (`is_*`, `has_*`, …) whose return type is not a boolean. | `ban` |
| `unexplained-return` | Python | A documented nontrivial function with a non-obvious result and no `Returns:` / `Yields:` description. | `ban` |

`undocumented-tuple-return` is removed: its finding becomes `positional-tuple-return`'s, whose fix (a named type) removes the need for a description.

## 2. Decisions

- **D1. One shared abstraction: the function's return contract.** `ast::collect_function_returns(file) -> Vec<FunctionReturn>` (language-dispatched) gives, for every function whose signature its author chose, its name, name node, declared return type node and a `ReturnShape`:
  - `Unit`: no value (Rust: no return type or `()`, also inside `Result` / `Poll`; Python: `-> None`, or unannotated with no `return <value>` and no `yield` and a non-stub body);
  - `Boolean`: `bool` (Python also `TypeGuard[T]` / `TypeIs[T]`), after unwrapping Rust `Result` / `*Result` / `Poll` envelopes and Python `Awaitable` / `Coroutine` / `Annotated`;
  - `PositionalTuple(arity)`: a fixed-length tuple of at least 2 elements, also inside `Option` / `| None` / unions, Rust `Result` / `Poll` / pointer wrappers, and Python generator yield types; unannotated Python: a `return` / `yield` of a tuple display in the function's own scope;
  - `Unknown`: unannotated Python whose type cannot be read (value returns, stubs);
  - `Other`.
  `positional-tuple-return` and `non-boolean-predicate` are a filter over this list each, with no language branch in the rule file.
- **D2. Imposed signatures are excluded once, in the collector.** Python: `PythonFunctionSignature::is_exempt_from_signature_rules()` (`@override`, `@overload`, `@abstractmethod`, fixtures, `@register`, data-model dunders, `Protocol` / `ABC` members). Rust: trait items, trait implementations and inline tests. One mutation per exemption then covers both rules.
- **D3. Predicate names: one language-neutral helper, `ast::is_predicate_name`.** After stripping leading underscores, the name is `<prefix>_<rest>` for `is`, `has`, `can`, `should`, `contains`, `starts_with`, `ends_with`, `matches`, `supports`, `allows`, `requires`, `needs`, or exactly `contains` / `matches` / `starts_with` / `ends_with`. `requires` / `needs` / `is` alone are not predicates (`Distribution.requires` is a list). Not configurable: rule files cannot share an option, the list matched no false positive in three corpora, and `ListKind`'s `banned` / `allowed` keys do not describe a list of name patterns (Q1).
- **D4. `min-elements = 2`.** A positional tuple has at least two elements by definition; same-typed pairs (`(PathBuf, PathBuf)`) are where swaps happen. The option exists so a project can raise it.
- **D5. `positional-tuple-return` defaults to `ban`.** Rules default to `ban` unless they are designed around an explanation (user decision, 2026-10-13). A first `require-explanation` default let every documented Rust function pass, because a `///` doc comment directly above the function counts as the explanation (`04_execution_log.md`). Symmetric pairs (`split_once`) are listed in `known_problems`; a project can suppress them or switch the mode.
- **D6. Accepted boolean envelopes.** Rust `Result<bool, E>` (and aliases whose last segment ends in `Result`, such as `io::Result`, `CargoResult`) and `Poll<bool>` are booleans: the error is out of band, as Python raises. `Option<bool>` / `bool | None` are not: they are three-state values (`if not x` treats `None` as `False`). Generalizing the shared envelope to `*Result` also lets `nullable-collection-return` see `CargoResult<Option<Vec<T>>>`.
- **D7. `unexplained-return` triggers** (only when the docstring has no `Returns:` / `Yields:` section and no `Return(s)` / `Yield(s)` summary):
  1. nullable: an annotated union with `None` and another type;
  2. a union of two or more non-`None` types;
  3. `bool`, unless the name is a predicate (D3) or the summary states the condition (`Whether …`, `… whether …`, `True if/when …`, `False if/when …`);
  4. `Any` / `object`;
  5. unannotated, when the body returns a value or yields.
  A single concrete type (`int`, `Config`, `list[str]`, a tuple) is obvious. Same gate (`min-statements` 12, `min-cognitive-complexity` 6) and scope as `unexplained-parameter`, plus property getters (documented like attributes).
- **D8. Spans.** All three flag the function name: stable under multi-line signatures, and where `require-explanation` looks for a header comment.
- **D9. Diagnostics show the contract.** `non-boolean-predicate` prints the declared return type, or `None` / `()` when absent; `positional-tuple-return` prints the element count.

## 3. Exemptions

| # | Rule | Exemption |
| :-- | :--- | :--- |
| E1 | tuple | Variadic `tuple[T, ...]`, 1-tuples and `()` are not positional tuples. |
| E2 | tuple, predicate | Imposed Python signatures (D2). |
| E3 | tuple, predicate | Rust trait items and trait implementations (D2). |
| E4 | tuple, predicate | Rust inline tests (D2). |
| E5 | tuple | Tuple displays inside nested functions, classes and lambdas do not count. |
| E6 | tuple | A tuple inside a collection (`list[tuple[A, B]]`, `Vec<(A, B)>`) is not a returned tuple (not visited). |
| E7 | predicate | A prefix must be followed by `_` (`island`, `cancel`, `hash_key`). |
| E8 | predicate | `TypeGuard[T]` / `TypeIs[T]`. |
| E9 | predicate | Rust `Result<bool, E>` / `*Result<bool>` / `Poll<bool>`. |
| E10 | predicate | Unannotated Python with a value return, and stub bodies (`Unknown`). |
| E11 | return | Below both gate thresholds. |
| E12 | return | No docstring (left to `undocumented-nontrivial-function`). |
| E13 | return | A `Returns:` / `Yields:` section or a `Return(s)` / `Yield(s)` summary. |
| E14 | return | `bool` with a predicate name. |
| E15 | return | `bool` with a condition summary. |
| E16 | return | A single concrete return type, tuples included. |
| E17 | return | Nested functions, docstring-exempt methods (`@override`, dunders, setters / deleters) and property getters. |

## 4. Questions

- **Q1.** Make the predicate prefixes configurable? Needs a third `ListKind` (keys such as `prefixes` / `extend-prefixes`) and a way for `unexplained-return` to read the same list. Deferred until a project asks.
