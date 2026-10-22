# Repeated docstring type: 02 References

> [!NOTE]
> **Status: VALIDATED (lead, 2026-10-12; implemented).**
> Quotes are verbatim from the copies in the lead's scratch directory (Google pyguide, pydoclint `violation_codes.md`, `config_options.md`, `checking_class_attributes.md`) and from the Pylint extensions page fetched on 2026-10-11. [U] marks what was not verified against a page or a tool run.

## 1. Google Python Style Guide

URL: <https://google.github.io/styleguide/pyguide.html#383-functions-and-methods>

- §3.8.3 *Args*: "List each parameter by name. A description should follow the name, and be separated by a colon followed by either a space or newline. […] The description should include required type(s) if the code does not contain a corresponding type annotation. If a function accepts `*foo` (variable length argument lists) and/or `**bar` (arbitrary keyword arguments), they should be listed as `*foo` and `**bar`."
- §3.8.3 *Returns*: "Describe the semantics of the return value, including any type information that the type annotation does not provide."
- §3.8.3 *Raises*: "List all exceptions that are relevant to the interface followed by a description. Use a similar exception name + colon + space or newline and hanging indent style as described in *Args:*."
- §3.8.3 example: every parameter of `fetch_smalltable_rows` is annotated and every `Args` entry is `name: description` with no type; `Returns:` starts with "A dict mapping keys to …", not with a type.
- §3.8.4: "Public attributes, excluding properties, should be documented here in an `Attributes` section and follow the same formatting as a function's `Args` section." The example writes `likes_spam: A boolean indicating …` with no type.

What this gives the rule:

- It endorses the check exactly as the legacy linter conditions it: a type in an `Args` entry is expected *only* when the annotation is missing (D3).
- By §3.8.4, `Attributes` entries follow the `Args` formatting, so the same condition applies to them (D1).
- *Returns* asks for type information "that the type annotation does not provide", which excludes restating the annotation (D4).
- *Raises* names exceptions, which are types themselves; there is nothing to repeat (D5).

## 2. pydoclint

URLs: <https://jsh9.github.io/pydoclint/violation_codes.html>, <https://jsh9.github.io/pydoclint/config_options.html>, <https://jsh9.github.io/pydoclint/checking_class_attributes.html>

Violation codes (verbatim):

| Code | Explanation |
| :--- | :--- |
| DOC105 | Argument names match, but type hints do not match |
| DOC106 | The option `--arg-type-hints-in-signature` is `True` but there are no argument type hints in the signature |
| DOC107 | The option `--arg-type-hints-in-signature` is `True` but not all args in the signature have type hints |
| DOC109 | The option `--arg-type-hints-in-docstring` is `True` but there are no type hints in the docstring arg list |
| DOC110 | The option `--arg-type-hints-in-docstring` is `True` but not all args in the docstring arg list have type hints |
| DOC111 | The option `--arg-type-hints-in-docstring` is `False` but there are type hints in the docstring arg list |
| DOC203 | Return type(s) in the docstring not consistent with the return annotation |
| DOC404 | The types in the docstring's Yields section and the return annotation in the signature are not consistent |
| DOC605 | Attribute names match, but type hints in these attributes do not match |

Options (verbatim):

- `--arg-type-hints-in-docstring` (default `True`): "If `True`, there need to be type hints in the argument list of a docstring. If `False`, there cannot be any type hints in the argument list of a docstring."
- `--arg-type-hints-in-signature` (default `True`): "If `True`, there need to be type hints for input arguments in the function/method signature."
- "Note: if users choose `True` for both options, the argument type hints in the signature and in the docstring need to match, otherwise there will be a style violation."
- `--check-return-types` (default `True`): "If True, check that the type(s) in the docstring return section and the return annotation in the function signature are consistent".
- `--check-yield-types` (default `True`): same for "yields".
- `--check-arg-defaults` (default `False`): "If True, docstring type hints should contain default values consistent with the function signature. If False, docstring type hints should not contain default values."

Class attributes (verbatim, Google example of `checking_class_attributes.md`):

```python
class MyPet:
    """
    A class to hold information of my pet.

    Attributes:
        name (str): Name of my pet
        age_in_months (int): Age of my pet (unit: months)
        ...
    Args:
        airtag_id (int): The ID of the AirTag that I put on my pet
    """
    name: str
    age_in_months: int
    ...
    def __init__(self, airtag_id: int) -> None:
```

Gaps and defects relative to Google style (improvements this rule makes):

| ID | pydoclint behavior | Example | Kind |
| :-- | :--- | :--- | :--- |
| G1 | DOC111 flags a docstring type even when the parameter is unannotated, which Google requires. | `def f(x):` with `x (int): …` | False positive (vs Google) |
| G2 | The default (`arg-type-hints-in-docstring = true`) *requires* the repetition (DOC109/DOC110), and with both options `true` DOC105 polices that the copies agree instead of removing one. | `def f(x: int):` with `x: The x.` is DOC109 | Opposite default |
| G3 | No option forbids types in `Attributes:`; its own documented Google convention writes `name (str):` above `name: str`. DOC605 only compares them. | The example above | False negative |
| G4 | No option forbids a type prefix in `Returns:` / `Yields:`. With `check-return-types = true` (default), DOC203 compares the prefix with the annotation [U: whether a Returns section *without* a type prefix is itself reported as inconsistent]. | `Returns:\n    bool: True if found.` with `-> bool` | False negative |
| G5 | DOC111 is one global switch; it cannot express "types only where the signature has none". | Gradually typed codebase | Granularity |
| G6 | Class-level `Args:` is checked against `__init__` (default `allow-init-docstring = false`); Google puts `Args:` in the `__init__` docstring. | §3.8.4 example | Placement conflict (not this rule's concern; the rule checks both places, D9) |

Ruff implements none of DOC105–DOC111, DOC203, DOC404 or DOC605: its DOC set is DOC102, DOC201, DOC202, DOC402, DOC403, DOC501, DOC502, all preview (<https://docs.astral.sh/ruff/rules/#pydoclint-doc>).

## 3. Pylint docparams

URL: <https://pylint.readthedocs.io/en/latest/user_guide/checkers/extensions.html#parameter-documentation-checker>

- `missing-type-doc (W9016)`: "\"%s\" missing in parameter type documentation. Please add parameter type declarations for all parameters."
- `missing-return-type-doc (W9012)`: "Missing return type documentation. Please document the type returned by this method."
- `missing-yield-type-doc (W9014)`: "Missing yield type documentation. Please document the type yielded by this method."
- The page's own Google example documents `x (float): bla x` and `y (int): bla y`.

All three go the opposite way: they ask for docstring types. [U] Whether a signature annotation satisfies `missing-type-doc` is not stated on the page. No Pylint message flags a repeated type.

## 4. Sphinx Napoleon (the origin of the `name (type):` form)

URL: <https://www.sphinx-doc.org/en/master/usage/extensions/example_google.html> [U: page fetch was denied; content from the prior research report]

- Napoleon's Google example writes `param1 (int): The first parameter.`, `param2 (:obj:`str`, optional): The second parameter.`, and `Returns:\n    bool: The return value. True for success, False otherwise.` It documents that the type may be omitted when the signature is annotated (PEP 484) [U].
- This is why the parenthesized slot and the `Returns:` `type:` prefix are common in Google-style code, and why `optional` appears inside the parentheses (D7, E3).
- Projects that render docs with Napoleon lose the type in the rendered page when it is removed from the docstring, unless Sphinx renders annotations (`autodoc_typehints = "description"`) [U: option name not re-verified]. This goes in `known_problems` (D11).

## 5. Ruff

- No rule inspects docstring types. `D417` (`undocumented-param`) checks that parameters are listed, not their types (<https://docs.astral.sh/ruff/rules/undocumented-param/>).
- Complement: `ANN001` / `ANN201` (flake8-annotations) require the annotations that make docstring types redundant.

## 6. Configuration that replaces the legacy check today

For a project that adopts pydoclint instead of this rule (narrower than this rule: no `Attributes`, no `Returns` / `Yields`; broader on unannotated parameters, G1):

```toml
[tool.pydoclint]
style = "google"
arg-type-hints-in-docstring = false  # DOC111: no type in any Args entry, annotated or not
arg-type-hints-in-signature = true   # DOC106/DOC107: every parameter is annotated, so G1 cannot bite
check-return-types = false           # [U] otherwise DOC203 compares a Returns type prefix with the annotation
check-yield-types = false            # [U] same for Yields (DOC404)

[tool.ruff.lint]
extend-select = ["ANN001"]           # annotations, which is where types belong
```

## 7. Rust

Rustdoc has no typed-parameter syntax: the signature is the only place a type is written. The nearest analogue is a `# Arguments` list that restates types in prose (``* `count` - A `usize` giving …``). No Clippy lint targets it [U]. Recorded only as a ROADMAP suggestion (`03_design.md` §10).
