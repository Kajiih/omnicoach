# Complex function docstring: 02 References

> [!IMPORTANT]
> **Status: VALIDATED** (lead, 2026-10-12; implemented).

Sources read for this document (all on 2026-10-11):

- Google pyguide §3.8.3, §3.8.3.1, §3.8.4: lead's scratch copy `pyguide.md` (quotes in `01_understand.md` §2).
- pydoclint: `violation_codes.md`, `config_options.md` (lead's scratch copies of the official docs).
- Pylint docparams: the extension's documentation page (`pylint.readthedocs.io/en/latest/user_guide/checkers/extensions.html`) and its source, `pylint/extensions/docparams.py` on `main` (line numbers below refer to that file).
- Ruff: rule pages `docs.astral.sh/ruff/rules/docstring-missing-returns/` (DOC201) and `docs.astral.sh/ruff/rules/undocumented-param/` (D417). Ruff's DOC set is DOC102, DOC201, DOC202, DOC402, DOC403, DOC501, DOC502, all preview.

Claims not checked against a primary source are marked **[U]**.

## 1. Coverage per violation

V1–V4 are defined in `01_understand.md` §3.

| Violation | Pylint | pydoclint | Ruff |
| :--- | :--- | :--- | :--- |
| V1 long function without docstring | C0116 `missing-function-docstring` with `docstring-min-length` [U: C0116 honoring the option was not read in source] | Not reported [U] | D103 / D102: public only, no size gate |
| V2 missing parameters | `missing-param-doc` (W9015), `missing-any-param-doc` (W9021) with `accept-no-param-doc = false` | DOC101 / DOC103 | D417, only when an `Args:` section exists |
| V3 undocumented return | `missing-return-doc` (W9011), `missing-yield-doc` (W9013) | DOC201, DOC402 | DOC201, DOC402 (preview) |
| V4 undocumented raise | `missing-raises-doc` (W9006) | DOC501 | DOC501 (preview) |

## 2. What each tool actually does, and where it is wrong

### 2.1 Pylint `docparams`

From the source:

- **Size gate is inconsistent.** `visit_functiondef` (lines 211–213) and `visit_yield` (lines 383–385) skip functions shorter than `docstring-min-length`, but `visit_return` (lines 350–377) does not: `missing-return-doc` fires on a 3-line function while `missing-param-doc` is silent on it. `_is_shorter_than_min_length` (lines 698–712) measures `last line - def line`, so the docstring counts toward the size, as in the legacy linter (B3).
- **No docstring, no parameter check.** `check_arguments_in_docstring` returns when the docstring is empty (lines 562–563); V1 is left to C0116.
- **All-or-nothing tolerance.** `accept-no-param-doc` (default `True`, lines 134–142) accepts a docstring with no parameter documentation at all (lines 592–594) but checks a partial one. With the default, a 300-line function with a one-line docstring passes.
- **Private functions are skipped.** Every check returns early when the name matches `no-docstring-rgx` (lines 207–209, 301–303, 360–362, 390–392), whose default is `^_` per the extension documentation ("by default docstrings of private and magic methods are not checked"). Google's "nontrivial size" trigger applies to private functions too.
- **Statement-driven returns.** `missing-return-doc` is emitted from `visit_return` for each `return <value>` (lines 350–371): an annotated function whose returns are all in a helper branch it cannot see is fine, but the return annotation alone never triggers it.
- **Generators must say `Yields:`.** With Google style, `visit_yield` requires a Yields section (lines 398–406), so Google's own `@contextlib.contextmanager` example, documented with `Returns:`, is reported as `missing-yield-doc` once `accept-no-yields-doc = false`. False positive.
- **Overload stubs** are skipped (line 198); abstract methods are skipped for redundant-returns/yields (lines 271, 283).
- The `"""Returns …"""` summary form is not recognized [U: `has_returns()` for Google style was not read].

### 2.2 pydoclint

From `config_options.md` and `violation_codes.md`:

- **No size gate, only a docstring-shape gate.** `--skip-checking-short-docstrings` (default `True`): "`pydoclint` won't check functions that have only a short description in their docstring." So a 300-line function with a one-line docstring is not checked at all (false negative), while with the option `False` every function with a docstring, of any size, needs full `Args:` / `Returns:` (contrary to Google's "can be omitted … one-line docstring").
- **No missing-docstring check** [U]: nothing in the violation list reports a missing docstring.
- **DOC402** "has \"yield\" statements, but the docstring does not have a \"Yields\" section": Google's `contextmanager` example is a false positive here too.
- **DOC201 property exemption is positional:** "Methods with `@property` as its outer-most decorator (i.e., on the top) do not need to have a return section." `@functools.cached_property` and a property under another decorator are not covered [U: whether `cached_property` counts].
- `--require-return-section-when-returning-nothing` (default `False`) exempts implicit `None`, `-> None` and `NoReturn`.
- `--ignore-underscore-only-args` (default `True`) exempts `_`, `*_`, `**__`; `--omit-stars-when-documenting-varargs` (default `False`) requires the stars of `*args`.
- `--skip-checking-private-functions` (default `False`) checks private functions, matching Google.
- `--allow-init-docstring` (default `False`): constructor arguments are expected in the class docstring, unlike Google's §3.8.4 example. The lead ran pydoclint with its defaults on that example verbatim (`SampleClass`, in the lead's scratch `docstring_probe/google_sample_class.py`): it reports DOC301 (the `__init__` docstring), DOC101 and DOC103 (`likes_spam` missing from the class docstring). False positives on the style guide's own code.
- DOC501 checks every `raise`, including API-violation errors that Google says not to document (§3.8.3 Raises).

### 2.3 Ruff

- **D417** "Note that this rule only checks docstrings with an arguments (e.g. Args) section." No size gate. "Parameters annotated with typing.Unpack are exempt from this rule." Option `lint.pydocstyle.ignore-var-parameters`.
- **DOC201** (preview since 0.5.6): "Checks for functions with return statements that do not have \"Returns\" sections in their docstrings. […] This rule is not enforced for abstract methods or functions that only return None. It is also ignored for \"stub functions\"". Options `lint.pydoclint.ignore-one-line-docstrings`, `lint.pydocstyle.property-decorators`. Statement-driven, no size gate.
- **D103 / D102** report missing docstrings on public functions and methods, with no size gate and never on private ones [U: page not fetched in this session].

## 3. Configuration that replaces the legacy check

Closest equivalent with existing tools. It still misses the gaps listed in §4.

```toml
[tool.pylint.main]
load-plugins = ["pylint.extensions.docparams"]

[tool.pylint.basic]
# C0116 and docparams skip functions spanning fewer lines (docstring included).
docstring-min-length = 25
# Default "^_" also exempts private helpers; Google's "nontrivial size" does not.
no-docstring-rgx = "^__\\w+__$"

[tool.pylint.parameter_documentation]
default-docstring-type = "google"
accept-no-param-doc = false
accept-no-return-doc = false
accept-no-yields-doc = false
```

The table names follow Pylint's checker sections; Pylint reads options from any `[tool.pylint.*]` table [U].

Alternative with pydoclint (no size gate, so it is stricter on short functions with multi-line docstrings and blind to long functions with one-line docstrings):

```toml
[tool.pydoclint]
style = "google"
skip-checking-short-docstrings = true
skip-checking-raises = true
arg-type-hints-in-docstring = false
check-return-types = false
check-yield-types = false
```

Ruff (preview) adds `DOC201`, `DOC402` and `D417` under `[tool.ruff.lint] select = ["D417", "DOC201", "DOC402"]`, `preview = true`, `[tool.ruff.lint.pydocstyle] convention = "google"`.

## 4. Gaps none of them closes

| # | Gap | Example | Tools affected |
| :-- | :--- | :--- | :--- |
| G1 | Size counts the docstring, so writing docs puts a function in scope. | 20 body lines + 6-line docstring → flagged | The legacy linter, Pylint |
| G2 | A long function with a one-line docstring is not checked. | 300 lines, `"""Syncs the cache."""`, 4 parameters | pydoclint (default), Pylint (default `accept-no-*`), Ruff D417 |
| G3 | A short function with a partial `Args:` section is not checked. | 5 lines, `Args:` lists `x` but not `y` | The legacy linter, Pylint (size gate) |
| G4 | Private long functions are skipped. | `def _reconcile(...)`, 80 lines, no docstring | The legacy linter (`__x` only), Pylint, Ruff D1xx |
| G5 | `Raises:` accepted for `Returns:`. | `-> int`, only `Raises:` | The legacy linter |
| G6 | `"""Returns …"""` summary not honored. | `"""Returns the active rows.\n\nArgs: …"""` | pydoclint, Ruff DOC201 [U for Pylint] |
| G7 | Decorated generator documented with `Returns:` reported. | Google's `@contextmanager` example | Pylint, pydoclint DOC402, Ruff DOC402 |
| G8 | Properties asked for `Returns:`. | `@property def path(self) -> str:` (long) | The legacy linter; pydoclint only when not outer-most |
| G9 | Unannotated value returns ignored, `NoReturn` flagged. | `def f(): return 1` / `def g() -> NoReturn:` | The legacy linter |
| G10 | Size is physical lines, so formatting and comments change the verdict. | A 22-line call reformatted to one argument per line | The legacy linter, Pylint |

## 5. Prior art for the size signal

| Tool | Signal |
| :--- | :--- |
| The legacy linter | Physical lines `def` → end, > 25 |
| Pylint `docstring-min-length` | Physical lines `def` → end (docstring included), default `-1` (off) |
| Ruff `PLR0915` too-many-statements | Statements, default 50 |
| Ruff `C901` / `PLR0912` | Cyclomatic complexity 10 / branches 12 |
| Clippy `too_many_lines` | Code lines of the body, excluding blank and comment-only lines, default 100 [U: exact counting rule] |
