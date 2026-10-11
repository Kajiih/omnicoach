# Repeated docstring type: 01 Understand

> [!NOTE]
> **Status: PENDING (lead validation).**
> Scope: port of the legacy linter `RedundantTypeDocstringRule` (Batch D, docstrings). Python only.
> Sources: `scratch/legacy_reference/legacy_lints.py` lines 2087–2148; Google Python Style Guide §3.8.3 and §3.8.4; pydoclint and Pylint docs (quoted in `02_references.md`). [U] marks claims not checked against a tool run or its source code.

## 1. What the legacy linter does

`_parse_docstring_annotated_params` (lines 2087–2112) and `RedundantTypeDocstringRule.check` (lines 2115–2148):

1. Runs on every `def` / `async def` that has a docstring (`ast.get_docstring`). Classes and modules are never checked.
2. Collects the names of **annotated** parameters: positional-only, regular, keyword-only, `*args`, `**kwargs`. Returns early if none is annotated.
3. Scans the docstring for a line whose stripped text is exactly `Args:`, `Arguments:`, `Parameters:` or `Keyword Args:`. The section ends at the first non-blank line indented no deeper than the header.
4. Inside the section, every line (entry or continuation) is matched against `^(\*?\*?[\w_]+)\s*\(([^)]+)\)\s*:`. The name has its `*` stripped.
5. If the name is an annotated parameter, it reports at the `def` line: "Argument `x` in docstring has type annotation `(int)`. Type annotations are banned from docstrings when repeating the signature's annotation."

## 2. The legacy linter defects found while reading

| ID | Defect | Example | Effect |
| :-- | :--- | :--- | :--- |
| P1 | Continuation lines are matched like entry lines. | A wrapped description whose continuation line reads `limit (see below): …` in a function with an annotated `limit` | False positive. |
| P2 | `[^)]+` stops at the first `)`, so nested parentheses fail the trailing `\)\s*:`. | `pair (tuple(int, str)): …` | False negative. |
| P3 | Headers are matched case-sensitively and only four of Napoleon's spellings. | `Params:`, `Keyword Arguments:`, `Other Parameters:`, `ARGS:` | False negative. |
| P4 | Reports at the `def` line, once per entry. | A function with three typed entries | Three identical-position findings; the reader must search the docstring. |
| P5 | Class docstrings are ignored. | `Attributes:` entries `count (int):` with `count: int` in the class body; class-level `Args:` (pydoclint's default placement) | False negative. |
| P6 | `Returns:` / `Yields:` are ignored. | `Returns:\n    bool: True if found.` with `-> bool` | False negative (the form of Napoleon's own example, `02_references.md` §4). |
| P7 | Any parenthesized text counts as a type. | `x (optional): …` | Flags default information, which is not type information (§4, Q3). |

## 3. The antipattern

A Google-style docstring entry states a type that the code already states in an annotation:

```python
def fetch(url: str, attempts: int = 3) -> bytes:
    """Fetches `url`.

    Args:
        url (str): The address to fetch.
        attempts (int, optional): Maximum number of requests.

    Returns:
        bytes: The response body.
    """
```

Why it is bad:

- Type checkers verify the annotation, never the docstring. When the annotation changes, the docstring copy goes stale silently and readers cannot tell which one is right.
- The style guide the project follows says so. Google §3.8.3 *Args*: "The description should include required type(s) if the code does not contain a corresponding type annotation." *Returns*: "Describe the semantics of the return value, including any type information that the type annotation does not provide." §3.8.4 makes `Attributes` "follow the same formatting as a function's `Args` section".
- The space is better spent on what the annotation cannot say: units, valid ranges, what `None` means.

When it is fine (so the rule must not fire):

- The parameter or attribute has **no** annotation. Google then *requires* the type in the docstring.
- The parenthesized text carries no type (`(optional)`), see Q3.

## 4. Questions the brief asked, and where they are answered

| Question | Answer | Where |
| :--- | :--- | :--- |
| Args only, or also `Attributes`? | Both: same reason, same fix. | `03_design.md` D1 |
| `Returns:` / `Yields:` starting with a bare type? | Yes, but only when the prefix equals the annotation, because `X:` is ambiguous with prose. | D4, E5 |
| `Raises:`? | No: the entry name *is* the exception type and there is no annotation to repeat. | D5, E7 |
| Napoleon `(type, optional)`? | Flagged as one parenthetical; `optional` alone is not a type. | D7, E3 |
| Docstring type differs from the annotation? | Same rule, same message, no type comparison. | D2 |
| Unannotated parameters? | Never flagged (Google requires the type there). | D3, E1 |
| Interaction with `docstring_optional_arg`? | Disjoint by construction: this rule owns the parenthesized slot, that rule owns the description. `(optional)` alone is owned by neither today. | D12, Q3 |

## 5. Existing coverage (summary; details in `02_references.md`)

| Tool | Rule | Relation to this check |
| :--- | :--- | :--- |
| pydoclint | DOC111 (`arg-type-hints-in-docstring = false`) | Broader on Args: also flags unannotated parameters, contradicting Google. Global switch. Default is the opposite (`true`: types *required*). |
| pydoclint | DOC105, DOC203, DOC404, DOC605 | Compare docstring types with annotations; with the defaults they push *towards* repeating types. |
| pydoclint | (none) | No option forbids types in `Attributes:`; its own Google example writes `name (str):` over `name: str`. |
| Pylint docparams | `missing-type-doc` (W9016), `missing-return-type-doc` (W9012) | Opposite direction: ask for docstring types. |
| Ruff | (none) | DOC set is DOC102, DOC201, DOC202, DOC402, DOC403, DOC501, DOC502; D417 checks names only. |

## 6. Decisions and open questions

Numbered decisions (D1…), exemptions (E1…) and open questions (Q1…) are in `03_design.md` §7–§9.
