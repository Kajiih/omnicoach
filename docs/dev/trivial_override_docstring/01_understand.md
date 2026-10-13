# Trivial override docstring: 01 Understand

> [!IMPORTANT]
> **Status: VALIDATED** (lead, 2026-10-12; implemented).

Phase 1 of porting the legacy linter's `TrivialOverrideDocstringRule` ([legacy_lints.py, lines 3389–3429](../../../scratch/legacy_reference/legacy_lints.py)) to omnicoach, Batch D (docstrings). Lead gate: rule 6, verdict *Narrow*.

## 1. What the legacy linter does

```python
class TrivialOverrideDocstringRule(LintRule):
    """Flags 1-line docstrings on @override methods.

    Overridden methods should either have no docstring (to cleanly inherit the parent's docstring)
    or a detailed multi-line docstring explaining implementation specifics.
    """
```

For every `def` / `async def`:

1. Skips the function unless one decorator is a `Name` or `Attribute` whose last segment is `override`. No import resolution: `@my_lib.override` matches, `@override()` (a `Call`) does not.
2. Skips it if `ast.get_docstring(node)` is empty or missing.
3. Reports when the docstring has one non-blank line or fewer, at `node.lineno` (the `def` line, or the first decorator line on Python < 3.8).

Message: "Method `{name}` has `@override` decorator and a 1-line docstring. Either remove the docstring to cleanly inherit parent documentation, or provide a full multi-line implementation-specific docstring."

What it does **not** look at: what the docstring says, the base class or its docstring, methods that override without `@override`, test files (no `is_test_file` check, unlike its neighbour `ComplexFunctionDocstringRule`).

## 2. What the style guide says

Google Python Style Guide §3.8.3.1 *Overridden Methods* (`scratch/pyguide.md`, the lead's verified copy):

> A method that overrides a method from a base class does not need a docstring if it is explicitly decorated with `@override`, unless the overriding method's behavior materially refines the base method's contract, or details need to be provided (e.g., documenting additional side effects), in which case a docstring with at least those differences is required on the overriding method.

Its examples:

```python
# Child class, but without @override decorator, a docstring is required.
class Child(Parent):
  def do_something(self):
    pass

# Docstring is trivial, @override is sufficient to indicate that docs can be
# found in the base class.
class Child(Parent):
  @override
  def do_something(self):
    """See base class."""
```

Also relevant:

- §3.8.3 *Functions and Methods*: "A docstring should give enough information to write a call to the function without reading the function's code." It also names side effects as something to document: "a function that mutates one of its arguments as a side effect should note that in its docstring."
- §3.8.2.1 *Test modules*: "A docstring that only repeats class or method name should be omitted." This is the only place the guide condemns a name-restating docstring, and it is scoped to tests.

## 3. Where the legacy linter's proxy is wrong

The legacy linter uses *line count* as a proxy for *trivial*. Against what a reader actually sees (§4):

| Docstring on an `@override` method | What hover / `help()` shows | Legacy linter |
| :--- | :--- | :--- |
| none | the full base docstring | fine |
| `"""See base class."""` | only the pointer | flagged ✓ |
| `"""Also invalidates the cache."""` (one difference, one line) | only the difference; the base contract is hidden | flagged, but its fix ("multi-line implementation-specific docstring") does not ask for the hidden contract |
| a multi-line copy of the base docstring | the same text as without it | not flagged ✗ |
| a multi-line difference without the base's `Args:` / `Returns:` | the difference only | not flagged ✗ |

Line count separates none of these cases by what they hide. Its suggestion pushes authors to pad a one-liner rather than to either inherit or complete it, a perverse incentive (rule_design_guide.md §3).

## 4. What a reader sees: own docstring replaces, never merges

Every tool that renders a method's documentation picks **one** docstring: the method's own if it has one, otherwise the first one found by name lookup through the MRO. None merges the two.

| Tool | Behaviour | Source |
| :--- | :--- | :--- |
| Pyright / Pylance hover and completion | `resolveMethodDocInfo` returns the class's own member docstring when present (step 1); only when it is empty does it walk the MRO by member name, skipping the class itself and builtin classes, and return the first docstring found (step 2). No `@override` check, no merging. | `packages/pyright-internal/src/analyzer/typeDocStringUtils.ts`, `resolveMethodDocInfo` (verified by the lead, lines 184–274) |
| `inspect.getdoc`, hence `help()` and `pydoc` | "Changed in version 3.5: Documentation strings are now inherited if not overridden." | `Doc/library/inspect.rst` (CPython) |
| Sphinx autodoc | `autodoc_inherit_docstrings` (default `True`): "inherit docstrings from their parents if not explicitly set". | Sphinx `autodoc` configuration docs |
| griffe / mkdocstrings | Inherits a missing docstring; merging the parent's sections into the child's happens only with the opt-in `merge: true` option. | griffe docs |

Consequences:

- **No docstring** shows the full base docstring, as long as the base defines the method (name lookup; `@override` is not needed for inheritance, and builtins such as `object.__repr__` are skipped by Pyright).
- **Any docstring** hides the base docstring entirely. A docstring that states only the differences therefore removes the rest of the contract (`Args:`, `Returns:`, `Raises:`, preconditions) from the reader's view.
- **A deferral phrase** (`"""See base class."""`) is the worst case: it hides the docstring it points to, so the reader gets pure indirection and must look up the base by hand.

## 5. Philosophy: no docstring, or a complete one

From §4, the user's rule for an `@override` method is: **either no docstring (inherit), or a full self-contained docstring** that a caller can use without reading the base, as §3.8.3 asks of any function docstring.

**This departs from Google §3.8.3.1** on purpose. Google requires "a docstring with at least those differences"; a differences-only docstring is compliant there. Here it is not, because no documentation tool merges it with the base docstring: hover, `help()` and generated docs would show the differences and nothing else. The departure only removes options: a complete docstring satisfies "at least those differences", so every docstring this policy accepts is also Google-compliant. The forms it rejects are the pointer (which Google shows as *trivial*, not required) and the incomplete difference.

Under this philosophy, an override docstring is a problem in three ways, each with its own harm and fix (split test, rule_design_guide.md §1):

| # | Pattern | Harm | Fix |
| :--- | :--- | :--- | :--- |
| P1 | The whole docstring is a deferral phrase | Hides the docstring it points to | Remove it |
| P2 | Equal to the inherited docstring or its summary | Adds nothing; drifts when either contract changes | Remove it |
| P3 | Different text that omits sections the inherited docstring has | Hides part of the contract | Complete it, or remove it |

Signals a single-file linter can observe:

| # | Signal | Available in one file? |
| :--- | :--- | :--- |
| S1 | Deferral phrase: the whole docstring is "See base class.", "Same as `Base.run`.", … | Always (no base needed) |
| S2 | Copy of the base method's docstring (exact, normalized, summary line, prefix, token overlap) | Only when the base class is defined earlier in the same file |
| S3 | Same docstring on several sibling implementations of one method name | Only for siblings in the same file; the shared base may be elsewhere |
| S4 | Docstring that only restates the method name ("Gets the name." on `get_name`) | Always |
| S5 | Override docstring lacks an `Args:` / `Returns:` / `Yields:` / `Raises:` section that the base docstring has | Only when the base class is defined earlier in the same file |

`02_references.md` §3 measures them; `03_design.md` decides.

## 6. Making overrides explicit

The rules only look at methods decorated with `@override` (`03_design.md` D8). That is complete only when the project makes every override explicit. Ruff has no such rule. Two type checkers do (lead probe, `explicit_override.py`):

| Implicit override | mypy `explicit-override` | Pyright `reportImplicitOverride` |
| :--- | :--- | :--- |
| Method redefining a base-class method | flagged | flagged |
| `object` dunder such as `__repr__` | flagged | flagged |
| Implementation of an `ABC` abstract method | flagged | flagged |
| Method of a class that explicitly subclasses a `Protocol` | flagged | flagged |
| `__init__` | not flagged | not flagged |
| Structural `Protocol` implementation (no inheritance) | not flagged | not flagged |

The two unflagged cases are consistent with §4 for structural implementations: without inheritance, no docstring is inherited, so a docstring there is the only documentation and is out of scope. `__init__` overrides stay out of scope unless decorated.

## 7. Decisions

- **D1.** The legacy linter's line-count proxy is not ported (§3).
- **D2.** The target is redefined in content terms: an `@override` docstring that hides what the method would inherit, either by adding nothing (P1, P2) or by omitting part of the inherited contract (P3). The philosophy is §5; the split is decided in `03_design.md`.
- **D3.** Python only (batch decision b). Rust's rustdoc also shows the trait method's documentation on an undocumented impl method; a Rust analogue goes to `03_design.md` as a ROADMAP suggestion only.

## 8. Open questions

None at this phase. The former Q1 (undecorated overrides) is settled by the user: `@override` only (§6, `03_design.md` D8).
