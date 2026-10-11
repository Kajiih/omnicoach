# Class docstring completeness: 01 Understand

> [!IMPORTANT]
> **Status: PENDING** (lead validation). Batch D, the legacy linter check `ClassDocstringRule`. Worker phases 1–3 only.

## 1. The candidate

Source: `scratch/legacy_reference/legacy_lints.py`, `ClassDocstringRule` (lines 637–698) and its helpers `_get_class_init_params` (376), `_find_class_level_attributes` (388), `_find_instance_level_attributes` (405), `_parse_docstring_sections` (449), `_is_identifier_in_section` (475).

The legacy linter's own docstring states the intent:

> Verifies class-level docstring blocks (Args, Attributes) exist. Ensures they document all attributes/parameters.

### 1.1 Exact behavior

| # | Step | Code | Consequence |
| :-- | :--- | :--- | :--- |
| P1 | Gate | `docstring = ast.get_docstring(node)`; `if not docstring: return`; `if len(doc_lines) <= 1: return` | Missing and one-line class docstrings are never checked. |
| P2 | Constructor parameters | `init_node.args.args + init_node.args.kwonlyargs if arg.arg != "self"` on the first direct `ast.FunctionDef` named `__init__` | Positional-only parameters, `*args` and `**kwargs` are ignored. The receiver is filtered by the literal name `self`. An `@overload` stub wins if it comes first. |
| P3 | Where `Args:` must be | `sections = _parse_docstring_sections(docstring)` on the **class** docstring only | An `Args:` section in the `__init__` docstring is never consulted. |
| P4 | Attributes | Class body: `ast.Assign` with `ast.Name` targets and `ast.AnnAssign` with an `ast.Name` target, name not starting with `_`. Constructor: `ast.walk(init_node)` for `self.<attr> = …` / `self.<attr>: T = …`, name not starting with `_` | Includes constants, `ClassVar`s, Enum members, dataclass / NamedTuple / TypedDict / Pydantic fields and framework names such as `model_config`. `ast.walk` also enters nested functions, lambdas and classes inside `__init__`. Ignores tuple targets, attributes created in any other method (`__post_init__`, `setUp`), and properties (functions are never collected). |
| P5 | Init parameters removed from attributes | `extra_attrs = set(attributes) - set(params)` | `self.host = host` never needs an `Attributes:` entry, and neither does any other attribute that happens to share a parameter's name. |
| P6 | Section parsing | Header recognized only if `stripped in {"Args:", "Attributes:"}`; a section ends at the first non-indented non-empty line | `Arguments:`, `Parameters:`, `Keyword Args:` are not recognized, so the class is reported as having no `Args:` block. |
| P7 | Entry matching | `re.compile(r"^\s*" + re.escape(identifier) + r"\s*(?:\(.*\))?\s*:")` against **every** line of the section | A continuation line that starts with `name:` satisfies the check (false negative). |
| P8 | Reporting | `self.visitor.report(node.lineno, …)`: one "missing `Args:` block" / "missing `Attributes:` block" finding, otherwise one finding per missing name, all on the `class` line | Every finding of a class shares one line, so one suppression silences all of them. |

### 1.2 the legacy linter false positives and false negatives

False positives:

- **Google's own example is flagged.** §3.8.4's `SampleClass` documents `likes_spam` in the `__init__` docstring's `Args:`; the legacy linter reports `Class SampleClass missing Args: block.` (P3).
- `Arguments:` / `Parameters:` / `Keyword Args:` headers are reported as a missing `Args:` block (P6).
- Enum members, `model_config` and similar framework names are required under `Attributes:` (P4).
- `self.x` in a nested function or class inside `__init__` is collected (P4, `ast.walk`).

False negatives:

- Positional-only parameters, `*args` and `**kwargs` are never required, although Google §3.8.3 says to list them as `*foo` and `**bar` (P2).
- Attributes first assigned in `__post_init__` or `__attrs_post_init__` are not collected (P4).
- An attribute named like a parameter is never required, whatever it holds (P5).
- A continuation line beginning with `name:` counts as an entry (P7).
- One-line docstrings are skipped entirely, so collapsing a docstring to one line silences the rule (P1): a perverse incentive (rule_design_guide.md §3).

## 2. What the style guide says

Google Python Style Guide, verified copy at `scratch/pyguide.md` (batch lead's scratch).

§3.8.4 Classes (line ~2270):

> Classes should have a docstring below the class definition describing the class. Public attributes, excluding properties, should be documented here in an `Attributes` section and follow the same formatting as a function's `Args` section.

Its example:

```python
class SampleClass:
    """Summary of class here.

    Longer class information...
    Longer class information...

    Attributes:
        likes_spam: A boolean indicating if we like SPAM or not.
        eggs: An integer count of the eggs we have laid.
    """

    def __init__(self, likes_spam: bool = False):
        """Initializes the instance based on spam preference.

        Args:
          likes_spam: Defines if instance exhibits this preference.
        """
        self.likes_spam = likes_spam
        self.eggs = 0
```

Three facts follow, each contradicting the legacy linter:

1. `Args:` for constructor parameters is in the `__init__` docstring, not the class docstring (contradicts P3).
2. `likes_spam` is both an init parameter (under `Args:`) and a public attribute (under `Attributes:`) (contradicts P5).
3. Both attributes are assigned as `self.x` in `__init__`, not declared in the class body. A tool that only reads the class body (pydoclint, see 02_references.md) cannot check this example.

§3.8.3 Functions and Methods (line ~2053), on the `Args:` section:

> List each parameter by name. […] If a function accepts `*foo` (variable length argument lists) and/or `**bar` (arbitrary keyword arguments), they should be listed as `*foo` and `**bar`.

and on omitting sections:

> These sections can be omitted in cases where the function's name and signature are informative enough that it can be aptly described using a one-line docstring.

§3.8.4 has no equivalent one-line exception for the `Attributes` section. §3.8.3 also states that properties are documented like attributes, in their own docstring (`"""The Bigtable path."""`), which is why §3.8.4 excludes them from `Attributes`.

## 3. Interaction with existing omnicoach rules

| Rule | Relation |
| :--- | :--- |
| `inline-public-attribute-annotation` (`src/code_lint/rules/inline_public_attribute_annotation.rs`) | Its fix moves `self.retries: int = 3` to a class-body `retries: int` plus `self.retries = 3`. The attribute stays the same public attribute, so a class-docstring check that counts class-body declarations **and** constructor assignments under one name gives the same verdict before and after that fix. Only the anchor moves (03_design.md D11). It does not touch unannotated `self.x = …`, which is exactly what pydoclint misses. |
| `field-after-method`, `mutable-dataclass`, `unslotted-dataclass` | Inspect the same classes, unrelated concern. |
| Other Batch D candidates | `ComplexFunctionDocstringRule` (dropped in favour of Pylint docparams, pending user) is the function-level analogue of the constructor half; see 03_design.md Q4. |

No omnicoach rule reads docstrings today (batch_d_status.md, shared-helper inventory).

## 4. Problem statement

Two different defects hide in one the legacy linter check:

1. **A constructor parameter is not documented.** The caller cannot write a call without reading the constructor. The fix is an `Args:` entry, in the class or `__init__` docstring.
2. **A public attribute is not documented.** A reader of an instance cannot know its state without reading every assignment. The fix is an `Attributes:` entry, always in the class docstring.

They differ in why they are bad (call contract vs data contract) and how to fix them (section and docstring), so by rule_design_guide.md §1 and user decision (c) they are two rules (03_design.md D1).

Out of scope, by the brief's questions:

- A missing class docstring: Ruff `D101`, Pylint `C0115` (03_design.md D3).
- Documented names that do not exist (stale entries): a different defect with a different fix (03_design.md Q3).
- Order and type mismatches between docstring and code (pydoclint `DOC104`, `DOC604`, `DOC105`, `DOC605`).
- Rust: user decision (b); ROADMAP note only (03_design.md §9).
