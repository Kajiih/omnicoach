# Complex function docstring: 01 Understand

> [!IMPORTANT]
> **Status: PENDING** (lead validation). Batch D, the legacy linter check `ComplexFunctionDocstringRule`.

## 1. The candidate

Source: `scratch/legacy_reference/legacy_lints.py` lines 3432–3480.

```python
class ComplexFunctionDocstringRule(LintRule):
    """Enforces detailed docstrings with Args and Returns/Raises on complex functions."""
    LINE_THRESHOLD = 25

    def check(self, node):
        if self.visitor.is_test_file or node.name.startswith("__") or is_overridden_method(node):
            return
        length = end_line - node.lineno + 1
        if length <= self.LINE_THRESHOLD:
            return
        docstring = ast.get_docstring(node) or ""
        has_args_section = any(sec in docstring for sec in ("Args:", "Parameters:", "Arguments:"))
        has_returns_section = any(sec in docstring for sec in ("Returns:", "Yields:", "Raises:"))
        non_self_params = [arg.arg for arg in all_args if arg.arg not in {"self", "cls"}]
        # missing `Args:` if non_self_params and not has_args_section
        # missing `Returns:` if node.returns is not None and not has_returns_section
        #   and the annotation is not the literal `None`
        # one warning at the `def` line: "Function `f` is complex/long (N lines) and requires a
        # detailed docstring containing `Args:` and `Returns:` section(s)."
```

Helpers it relies on: `is_overridden_method` (line 1178) matches a decorator named `override` (`ast.Name` or the `attr` of an `ast.Attribute`, any module); `is_test_file` is `path.match("tests/**/*.py") or path.name.startswith("test_")` (line 3734).

### 1.1 Exact behavior

| Aspect | The legacy linter behavior |
| :--- | :--- |
| Unit | `def` and `async def`, nested ones included. |
| Size | Physical lines from the `def` line to the last body line, so it counts the docstring, comments and blank lines but not decorators. Flagged above 25. |
| Skipped | Test files; every name starting with `__` (dunders **and** name-mangled privates such as `__helper`); any decorator named `override`. |
| Args requirement | At least one parameter other than `self` / `cls` (any position, `*args` / `**kwargs` included) and none of the substrings `Args:`, `Parameters:`, `Arguments:` in the docstring. |
| Returns requirement | A return annotation that is not the literal `None`, and none of the substrings `Returns:`, `Yields:`, `Raises:`. |
| Content | Presence only: the entries are never compared with the signature. |
| Report | One warning per function, on the `def` line, naming both missing sections. |

### 1.2 Defects found while reading

- **B1. `Raises:` satisfies `Returns:`.** A function `-> int` whose docstring documents only its exceptions passes. Both the lead's gate and the research report call it a bug: a `Raises:` section says nothing about the return value.
- **B2. Substring matching.** `Args:` anywhere counts: in prose (`"See Args: below"`), in a code example, or as part of `Other Parameters:`. Matching is case-sensitive, so `ARGS:` does not count, while Napoleon accepts it.
- **B3. The docstring counts toward the size.** A 20-line function becomes "complex" once someone writes a 6-line prose docstring for it: documenting the function is what puts it in scope. Comments and blank lines count too, so the size depends on formatting.
- **B4. `startswith("__")` skips name-mangled private helpers** (`__compute`), and also `__init__`, `__new__` and `__call__`, whose parameters are designed by the author and documented in Google's own example (§3.8.4).
- **B5. A long function with no docstring is only flagged by accident.** `def main() -> None` with 200 lines, no parameters and no docstring passes. With parameters, the message asks for an `Args:` section, while the actual problem is that there is no docstring at all.
- **B6. Presence only.** An `Args:` section that lists 1 of 5 parameters passes, and so does a stale one listing renamed parameters.
- **B7. Annotation only.** An unannotated function that returns a value is never asked for `Returns:`; a `-> NoReturn` or `-> Never` function is (it cannot return), and so is a `-> "None"` string annotation.
- **B8. Properties are asked for `Returns:`.** Google says the opposite (§2 below).
- **B9. The `"""Returns …"""` form is ignored.** Google lets a docstring that starts with "Returns" / "Yields" omit the section.

## 2. What the Google style guide says

Verbatim, from `pyguide.md` (lead's scratch copy).

§3.8.3 Functions and Methods (lines 2058–2068):

> In this section, "function" means a method, function, generator, or property.
>
> A docstring is mandatory for every function that has one or more of the following properties:
>
> - being part of the public API
> - nontrivial size
> - non-obvious logic

Lines 2069–2071:

> A docstring should give enough information to write a call to the function without reading the function's code.

Lines 2081–2084, properties:

> The docstring for a `@property` data descriptor should use the same style as the docstring for an attribute or a function argument (`"""The Bigtable path."""`, rather than `"""Returns the Bigtable path."""`).

Lines 2086–2091, when sections may be omitted:

> Certain aspects of a function should be documented in special sections, listed below. […] These sections can be omitted in cases where the function's name and signature are informative enough that it can be aptly described using a one-line docstring.

Lines 2095–2102, *Args:*:

> List each parameter by name. […] If a function accepts `*foo` (variable length argument lists) and/or `**bar` (arbitrary keyword arguments), they should be listed as `*foo` and `**bar`.

Lines 2105–2120, *Returns:* (or *Yields:* for generators):

> Describe the semantics of the return value […]. If the function only returns None, this section is not required. It may also be omitted if the docstring starts with "Return", "Returns", "Yield", or "Yields" (e.g. `"""Returns row from Bigtable as a tuple of strings."""`) *and* the opening sentence is sufficient to describe the return value. […] If the function uses `yield` (is a generator), the `Yields:` section should document the object returned by `next()`, instead of the generator object itself that the call evaluates to.

Lines 2123–2129, *Raises:*:

> List all exceptions that are relevant to the interface followed by a description. […] You should not document exceptions that get raised if the API specified in the docstring is violated (because this would paradoxically make behavior under violation of the API part of the API).

Lines 2207–2221, decorated functions: "A decorated function's docstring must specify the function's behavior after the decorator has been applied to it". Its example is a `@contextlib.contextmanager` generator documented with **`Returns:`** ("A context manager providing a reference to a remote file."), not `Yields:`.

§3.8.3.1 Overridden Methods (lines 2234–2240):

> A method that overrides a method from a base class does not need a docstring if it is explicitly decorated with `@override`, unless the overriding method's behavior materially refines the base method's contract, or details need to be provided […], in which case a docstring with at least those differences is required on the overriding method.

§3.8.4 Classes (lines 2275–2299): public attributes go in the class docstring's `Attributes` section; the example documents the constructor parameter in the **`__init__` docstring** (`Args: likes_spam: …`).

### 2.1 Consequences for this rule

1. Size is one of three independent triggers for a docstring. Private functions are not exempt from it: the legacy linter (B4, partly) and Pylint (`no-docstring-rgx = "^_"` by default, see `02_references.md`) both drop private functions, which Google does not.
2. Sections are optional exactly when a one-line docstring suffices. A linter cannot judge "suffices"; size is the only mechanical proxy the legacy linter had, and it is the proxy Google names.
3. `Returns:` is not required for `None`, for properties, for a "Returns …" summary, and a decorated generator documents its decorated behavior (so `Returns:` is correct for a context manager).
4. `Raises:` documents a different fact than `Returns:`. Whether a raise needs documenting depends on whether it is part of the interface, which no syntactic check can tell (§3.8.3 Raises).

## 3. The antipatterns, separated

Applying the split test ([rule_design_guide.md](../rule_design_guide.md) §1), the legacy check bundles four violations:

| # | Violation | Why it is bad | How to fix |
| :-- | :--- | :--- | :--- |
| V1 | A long function has no docstring. | A caller must read the whole body to learn what it does. | Write a docstring. |
| V2 | A function's parameters are missing from its docstring (no `Args:` section on a long function, or an `Args:` section that omits some). | A caller cannot learn what to pass without reading how each parameter is used. | Add an `Args:` entry per parameter. |
| V3 | A function that returns a value does not document it (`Returns:` / `Yields:`). | A caller cannot learn what comes back, or which values (empty, sentinel) to expect. | Add a `Returns:` (or `Yields:`) section, or start the summary with "Returns …". |
| V4 | A function raises but has no `Raises:` section. | Callers do not know which exceptions to handle. | Add a `Raises:` section. |

V1–V3 differ in both *why* and *how*. V4 only appears in the legacy linter as a substitute for V3 (B1); see `03_design.md` D9 for why it is not ported.

## 4. Interpretations to settle in Phase 3

- **I1. Complexity signal.** Physical lines (the legacy linter, Pylint `docstring-min-length`), code lines without docstring / comments / blanks, statement count (Ruff `PLR0915` style), or branch count (`C901`, `PLR0912`).
- **I2. Does a section requirement depend on size only?** Once a docstring has sections, Google's "may omit" condition (a one-liner suffices) no longer holds, so an incomplete `Args:` section is incomplete whatever the size. Ruff D417 follows that; the legacy linter and pydoclint do not (`02_references.md`).
- **I3. Where constructor parameters go.** Google's example puts them in the `__init__` docstring, pydoclint's default in the class docstring, Pylint accepts either.
- **I4. Public vs private.** Google: size alone triggers the requirement. The legacy linter, Pylint, Ruff D1xx: private functions are skipped.
- **I5. Return detection.** Annotation (the legacy linter) vs `return <value>` statements (Ruff, Pylint, pydoclint) vs both.

## 5. Relation to the rest of Batch D

- `ClassDocstringRule` (dropped at the gate): its `Args:` half is about constructor parameters, which V2 touches through I3. The decision on I3 here must not contradict the pydoclint configuration recommended there.
- `RedundantTypeDocstringRule` and `DocstringOptionalArgRule` read the same `Args:` entries; V2 only checks which names are present, never their text, so the rules do not overlap.
- `RedundantReturnsDocstringRule` flags a `Returns:` section that starts with "Returns"; V3 accepts a *summary line* that starts with "Returns". Different lines of the docstring, no overlap, but the two docs should name each other so readers do not confuse them.
- `TrivialOverrideDocstringRule`: V1–V3 skip `@override` methods entirely (§3.8.3.1), leaving their docstrings to that rule.
