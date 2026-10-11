# Docstring optional argument: 01 Understand

> [!NOTE]
> **Status: PENDING** (lead validation).
> Batch D (docstrings), worker for the legacy linter `DocstringOptionalArgRule`. Process: [rule_batch_playbook.md](../rule_batch_playbook.md). References: [02_references.md](02_references.md). Design: [03_design.md](03_design.md).
> [U] marks claims not checked against code, documentation text, or tool output.

## 1. The legacy check

Source: `scratch/legacy_reference/legacy_lints.py`, lines 2223–2273.

```python
class DocstringOptionalArgRule(LintRule):
    """Verifies optional argument descriptions in docstring.

    They should not start with 'Optional...' or us '... or None' phrasing.
    """

    node_types = CALLABLE_NODES  # functions, async functions, classes (line 484)

    @staticmethod
    def _get_violation_reason(desc_clean: str) -> str | None:
        if re.match(r"^Optional(?:\b|\s|:|\.|,)", desc_clean, re.IGNORECASE):
            return "start with 'Optional...'"
        if re.search(r"\b(?:or\s+None|\|\s*None)\b(?:\.|$)", desc_clean, re.IGNORECASE):
            return "specify '... or None'"
        return None
```

The visitor walks `ast.get_docstring(node).splitlines()`. A line that is exactly `Args:`, `Arguments:`, `Parameters:`, `Keyword Args:` or `Attributes:` opens a section; a non-blank line indented no deeper than the header closes it. Every non-blank line inside the section that contains `:` is split at its first `:` and the text after it is tested. The report goes to the `def` / `class` line, with the message:

> Argument description in docstring should not {issue}. Instead, describe fallback behavior (e.g., 'If not provided...'). Found on line: `{stripped}`

## 2. What it is trying to prevent

The suggested fix ("describe fallback behavior, e.g. 'If not provided...'") shows the intent: the description of an optional or nullable argument should say **what the code does when the value is `None` or left out**, not just **that** it may be. Two phrasings are used as evidence that the author wrote the label instead of the meaning:

- `timeout: Optional timeout in seconds.`: does omission mean wait forever, a library default, or no wait?
- `parent: The parent node or None.`: is a `None` parent the root, a detached node, or "look it up"?

Google's rule for docstrings is the yardstick (pyguide §3.8.3): "A docstring should give enough information to write a call to the function without reading the function's code." Nullability itself is already in the signature (§3.19.5: "If an argument can be `None`, it has to be declared!"), so `Optional` and `or None` restate the annotation and leave out the one thing only prose can give.

### 2.1 Two readings of the antipattern

| Reading | Bad because | Fix | Flags `Optional timeout. If None, waits forever.`? |
| :--- | :--- | :--- | :--- |
| **R1: unexplained optionality** | The meaning of `None` / omission is missing | State the meaning | No: the meaning is there |
| **R2: restated nullability** | Prose duplicates the annotation (type information) | Delete the marker | Yes |

R2 is a "type information in the docstring" rule, the territory of the `redundant_type_docstring` worker (pyguide §3.8.3 Args: "include required type(s) if the code does not contain a corresponding type annotation"), and its harm is one redundant word. R1 is the harm the legacy message names. The design takes **R1** (`03_design.md` D1).

## 3. The legacy linter defects found

| # | Defect | Effect | Example |
| :-- | :--- | :--- | :--- |
| P1 | Each indented line with `:` is tested on its own, so wrapped continuation lines are treated as entries | False positive | `keys: Keys to fetch.` / `    Format: Optional prefix then id.` |
| P2 | A description on the line after the name (Google's second `Args:` form, pyguide line ~2180) is never tested: the name line has an empty description and the next line has no `:` | False negative | `timeout:` / `    Optional timeout.` |
| P3 | A marker wrapped onto a continuation line without `:` is never tested | False negative | `parent: The parent node` / `    or None.` |
| P4 | `re.IGNORECASE` also applies to `None`, so English "none" matches | False positive | `tags: Tags to keep, or none.` |
| P5 | `\b(?:…\|\s*None)` needs a word character right before `\|`, so the spaced form is missed | False negative | `x: A float \| None.` (only `float\|None` matches) |
| P6 | Explained descriptions are flagged when they start with `Optional` | False positive under R1 | `Optional timeout. If None, waits forever.` |
| P7 | `Defaults to None.`, `Can be None.`, `(optional)`, a trailing `Optional.` are not markers | False negative | Sphinx Napoleon's own example: `The second parameter. Defaults to None.` |
| P8 | Headers are case-sensitive and the list misses Napoleon's `Params:`, `Keyword Arguments:`, `Other Parameters:` | False negative | `Keyword Arguments:` |
| P9 | Reported on the `def` line with the raw line pasted into the message | Poor location | — |

P4–P7 were checked by running the legacy linter's two regexes on sample descriptions: `A float | None.`, `Defaults to None.`, `Can be None.` and ``The parent or `None`.`` are not matched; `A float|None.`, `Tags to keep, or none.` and `Optional timeout. If None, waits.` are. P1–P3 follow from the line loop quoted in §1.

What the legacy linter gets right, and the design keeps: `or None` counts only at the end of a sentence (`… or None.`), so `or None to use the default` (which explains) passes; `Optionally` is not a marker (`^Optional(?:\b|…)` fails on `Optionally`), which avoids `sort: Optionally sorts the output.` on a `bool`.

## 4. Questions the design must answer

From the brief, answered in `03_design.md`:

| Question | Where |
| :--- | :--- |
| Real antipattern (R1 vs R2) | D1 |
| Key on the parameter's default or annotation? | D6, Q1, Q2 |
| Other phrasings: `Defaults to None.`, `can be None`, `optional.`, `if any` | D3, D4 |
| Napoleon `name (type, optional):` | D7 |
| `Attributes:` sections | D5 |
| Case sensitivity | D3 (E3) |
| Overlap with `redundant_type_docstring` | D7, §5 of `03_design.md` |

## 5. Language scope

Python only (batch decision b). Rustdoc has no standard per-parameter section: `# Arguments` lists are a convention of some crates [U], and `Option<T>` parameters are rare in public APIs compared with builder methods. A Rust analogue is recorded as a ROADMAP suggestion only (`03_design.md` §7).
