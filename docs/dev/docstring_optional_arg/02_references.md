# Docstring optional argument: 02 References

> [!NOTE]
> **Status: PENDING** (lead validation).
> Sources verified verbatim: Google pyguide (`scratch/pyguide.md` of the lead's conversation), pydoclint `violation_codes.md`, `config_options.md`, `checking_class_attributes.md` (same directory), the legacy source. Web sources were read through search summaries only and are marked [U] where the exact wording was not seen.

## 1. Style guides

### 1.1 Google Python Style Guide

Silent on the words "optional" and "or None" in descriptions. Three passages bear on it:

- §3.8.3: "A docstring should give enough information to write a call to the function without reading the function's code. The docstring should describe the function's calling syntax and its semantics". The meaning of `None` or omission is calling semantics.
- §3.8.3 *Args*: "The description should include required type(s) if the code does not contain a corresponding type annotation." Nullability is type information, so restating it next to an annotation adds nothing.
- §3.19.5: "If an argument can be `None`, it has to be declared!" The annotation (`str | None`) is where nullability lives.

Its examples never mark a parameter as optional in prose. The defaulted `require_all_keys: bool = False` is described by its effect: "If True only rows with values set for all keys will be returned."

§3.8.4: public attributes go in an `Attributes` section that follows "the same formatting as a function's `Args`", so the same wording standard applies there.

### 1.2 Sphinx Napoleon (Google style, as rendered by Sphinx)

Napoleon's `example_google.py` puts optionality in the **type slot** and restates the default in prose [U, exact wording from search summary]:

```python
def module_level_function(param1, param2=None, *args, **kwargs):
    """...
    Args:
        param1 (int): The first parameter.
        param2 (:obj:`str`, optional): The second parameter. Defaults to None.
```

Its class example documents `attr2 (:obj:`int`, optional): Description of `attr2`.` [U]. This is the most-copied counterexample: `Defaults to None.` says what the signature already says and nothing about what `None` does. The rule flags that sentence (`03_design.md` D3). The `(type, optional)` slot is left to the type-information rule (D7).

### 1.3 numpydoc

Puts optionality in the type line (`x : int, optional`) and the default in the description as "(the default is -1, which implies summation over all axes)"; `str or None` in the type line [U, search summary]. Its own example pairs the default with its meaning, which is the behavior this rule asks for. numpydoc style is not parsed by this rule (Google sections only).

## 2. Linters

| Tool | Closest check | Covers this? |
| :--- | :--- | :--- |
| Ruff | DOC102, DOC201, DOC202, DOC402, DOC403, DOC501, DOC502 (preview); pydocstyle D417 (`undocumented-param`) | No. D417 checks presence of a description, not its wording. |
| pydoclint | DOC105 (type hints do not match) with `--check-arg-defaults` | No. `check-arg-defaults = true` wants the default **in the docstring type hint** (`int, default=3`), consistent with the signature; `false` wants none there. It compares type-slot text, never the description, and never asks what `None` means. |
| pydoclint | DOC109–DOC111 (`arg-type-hints-in-docstring`) | No. Type slot only; that is the `redundant_type_docstring` worker's ground. |
| Pylint `docparams` | `missing-param-doc`, `missing-type-doc`, `differing-param-doc` | No [U]: checks presence and names. Its Google parser accepts `(type, optional)` [U]. |
| darglint | `DAR101`–`DAR103` (missing / excess / type mismatch) | No [U]. darglint is no longer maintained [U]. |
| numpydoc `validate` | `PR01`–`PR10` (parameter presence, type, description punctuation) | No [U]. |
| pydocstyle | D-rules | No (layout and presence only). |
| proselint, Vale | Prose style packages | No: neither parses docstring sections, and no packaged rule targets "optional" / "or None" [U]. A Vale custom rule could match the words but not the section or the signature. |

Prior research (subagent `fe0ab7c6`, batch status row 4): "None found."

### 2.1 Replacing configuration

**None exists.** No `pyproject.toml` setting reproduces the check. The nearest settings concern the type slot and would be used together with this rule, not instead of it:

```toml
[tool.pydoclint]
style = "google"
arg-type-hints-in-docstring = false   # DOC111: no `(type)` when the signature is annotated
check-arg-defaults = false            # DOC105: no `default=` in docstring type hints
```

With `check-arg-defaults = true`, pydoclint asks for `x (int, default=None)`, which is still type-slot text and leaves the description free to say nothing about `None`; it does not conflict with this rule.

## 3. omnicoach

- No existing rule reads docstrings.
- `nullable-collection-return` and Ruff `RUF013` / `UP045` concern the annotation, not its prose.
- Batch D sibling `redundant_type_docstring`: type information in the docstring type slot. Overlap and hand-off: `03_design.md` §5.
- Batch D sibling `redundant_returns_docstring`: `Returns:` wording. This rule does not read `Returns:` (Q3).
