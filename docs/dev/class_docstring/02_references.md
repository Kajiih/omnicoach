# Class docstring completeness: 02 References

> [!IMPORTANT]
> **Status: VALIDATED** (lead, 2026-10-12; implemented).

Sources read verbatim: Google pyguide §3.8.3 / §3.8.4 (`scratch/pyguide.md` in the lead's scratch directory); pydoclint `violation_codes.md`, `config_options.md`, `checking_class_attributes.md` (same directory, copies of jsh9.github.io/pydoclint); the Pylint docparams extension page (pylint.readthedocs.io/en/latest/user_guide/checkers/extensions.html). Behavior that the docs imply but nobody ran is marked **[U]**. The lead ran pydoclint (via `uvx`) on Google's §3.8.4 `SampleClass` verbatim and on a class with an undocumented `self.cache = {}`; probe file: `scratch/docstring_probe/google_sample_class.py` in the lead's scratch directory. Results are cited as **[run]**.

## 1. Style guides

| Source | What it says | Effect on the design |
| :--- | :--- | :--- |
| Google §3.8.4 | "Public attributes, excluding properties, should be documented here in an `Attributes` section". Example: `Args:` in the `__init__` docstring; `likes_spam` is under both `Args:` (of `__init__`) and `Attributes:` (of the class); both attributes are `self.x` in `__init__`. | Attributes go in the class docstring; constructor `Args:` may go in `__init__`; instance attributes count; init parameters stored on `self` are attributes too. |
| Google §3.8.3 | "List each parameter by name." `*foo` / `**bar` are listed with their stars. Sections "can be omitted" when a one-line docstring suffices. | Variadics count; one-line docstrings may omit `Args:`. |
| PEP 257 | "The docstring for a class should summarize its behavior and list the public methods and instance variables. […] The class constructor should be documented in the docstring for its `__init__` method." (<https://peps.python.org/pep-0257/#multi-line-docstrings>) **[U]**: quoted from memory of the PEP, not from a verified copy. | Same split as Google: constructor in `__init__`, instance variables in the class. |

## 2. pydoclint

Defaults: `--style=numpy` (`config_options.md` §3), so Google users must set `style = "google"`.

| Concern | pydoclint | Notes |
| :--- | :--- | :--- |
| Constructor parameters documented | `DOC101` (fewer args), `DOC103` (different args) on the class docstring | Placement is a single global choice: `--allow-init-docstring` (default `False`) "If it is set to `True`, having a docstring for class constructors (`__init__()`) is allowed, and the arguments are expected to be documented under `__init__()` rather than in the class docstring." With the default, `DOC301` "`__init__()` should not have a docstring; please combine it with the docstring of the class" fires on Google's own `SampleClass`, together with `DOC101`/`DOC103` on `__init__` **[run]**. With `True`, `SampleClass` still fails `DOC602`/`DOC603`, plus `DOC105`/`DOC109`/`DOC110` because `arg-type-hints-in-docstring` defaults to `True`, the opposite of Google §3.8.3 **[run]**. A class-docstring `Args:` in that mode should give `DOC304` "Class docstring has an argument/parameter section; please put it in the `__init__()` docstring" **[U]** (not probed). |
| Stale constructor parameters | `DOC102`, `DOC103` | |
| Short docstrings | `--skip-checking-short-docstrings` (default `True`): "won't check functions that have only a short description" | Same gate as the legacy linter's P1. |
| Class attributes documented | `DOC601`/`DOC602`/`DOC603` with `--check-class-attributes` (default `True`): "check the class attributes (defined under the class definition) against the "Attributes" section" | **Only the class body.** `self.x = …` attributes are invisible. |
| Private / dunder attributes | `--ignore-private-class-attributes`, `--ignore-special-dunder-class-attributes` (both default `True`). "Ignore" means "An ignored name must not appear in the docstring; documenting it produces an extra-name violation" | |
| Properties | `--treat-property-methods-as-class-attributes` (default `False`) | Default matches Google. |
| `ClassVar` | `--only-attrs-with-ClassVar-are-treated-as-class-attrs` (default `False`) | |
| Inline attribute docstrings | `--require-inline-class-var-docs` (default `False`) → `DOC606` / `DOC607` | |
| Order, types | `DOC104`, `DOC105`, `DOC604`, `DOC605` | Out of scope here. |

### 2.1 pydoclint gaps this design improves on

| # | Gap | Example |
| :-- | :--- | :--- |
| G1 | Instance attributes (`self.x = …`) are never required (false negative): an undocumented `self.cache = {}` in `__init__` gets no `DOC601` **[run]**. | `class Job:` with a multi-line docstring and `self.retries = 3` in `__init__` passes. |
| G2 | Documenting instance attributes is reported as extra: Google's `SampleClass` fails `DOC602` and `DOC603` ("Arguments in the docstring but not in the actual class attributes: [eggs, likes_spam]") with the default config and with `allow-init-docstring = true` **[run]**. Users must choose between following Google and passing pydoclint. | Google §3.8.4 example. |
| G3 | One placement for constructor `Args:` per project; the other is a violation. Google's example needs `allow-init-docstring = true`; a codebase mixing both styles cannot pass. | Pylint's docparams instead accepts either (§3). |
| G4 | Enum members, `model_config` and similar names are class-body assignments, so `DOC601` requires each one under `Attributes:` **[U]** (no exemption is documented). | `class Color(Enum): RED = 1` with a multi-line docstring. |
| G5 | NumPy is the default style, and `arg-type-hints-in-docstring` defaults to `True` (types required in `Args:` even when annotated) **[run]**. | Configuration burden only. |

## 3. Pylint (`pylint.extensions.docparams`)

Verbatim, from the extension page:

> Constructor parameters can be documented in either the class docstring or the `__init__` docstring, but not both

`multiple-constructor-doc (W9005)`: "\"%s\" has constructor parameters documented in class and %s". Missing parameters: `missing-param-doc (W9015)`. "by default docstrings of private and magic methods are not checked. To change this behaviour (for example, to also check `__init__`) add `no-docstring-rgx=^(?!__init__$)_`".

Gaps: Pylint has no attribute check at all. Whether missing parameters are reported when no parameter is documented depends on `accept-no-param-doc`, whose default I could not verify **[U]** (believed `yes`, which would accept a docstring with no `Args:` at all, the most common omission).

## 4. Ruff

- DOC rules: only `DOC102`, `DOC201`, `DOC202`, `DOC402`, `DOC403`, `DOC501`, `DOC502`, all preview (lead's gate). None checks class attributes or class-docstring `Args:`.
- `D417` (undocumented-param): function docstrings, and only when an `Args:` section exists (research report `fe0ab7c6`, from <https://docs.astral.sh/ruff/rules/undocumented-param/>). It sees the `__init__` docstring, not the class docstring **[U]**.
- `D101` (undocumented-public-class), `D107` (undocumented-public-init): missing docstrings.

## 5. Configuration that replaces the legacy linter's check with external tools

For the record (batch playbook Phase 0.1), the closest external replacement, Google placement:

```toml
[tool.pydoclint]
style = "google"
allow-init-docstring = true          # Args in __init__, as in Google §3.8.4
skip-checking-short-docstrings = true
check-class-attributes = true        # class-body attributes only (G1, G2)

[tool.pylint.main]
load-plugins = ["pylint.extensions.docparams"]

[tool.pylint.basic]
no-docstring-rgx = "^(?!__init__$)_"

[tool.pylint.parameter_documentation]
default-docstring-type = "google"
accept-no-param-doc = false
```

With both tools, a project still misses `self.x` attributes (G1) and must pick one `Args:` placement for pydoclint (G3). Ruff alone covers neither half.

## 6. Coverage summary

| Legacy sub-check | Covered by | Gap left |
| :--- | :--- | :--- |
| Constructor parameters in class `Args:` | pydoclint `DOC101`/`DOC103` (one placement); Pylint `W9015` (either placement) | Two linters for one fact; pydoclint's single placement; the legacy linter's own defects (01_understand.md §1.2). |
| Class-body attributes in `Attributes:` | pydoclint `DOC601`–`DOC603` | Enum / framework names (G4). |
| Constructor `self.x` attributes in `Attributes:` | Nothing | Whole sub-check (G1, G2). |
