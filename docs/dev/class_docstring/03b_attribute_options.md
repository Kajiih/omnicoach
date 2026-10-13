# Class docstring completeness: 03b Options for constructor arguments stored on `self`

> [!IMPORTANT]
> **Status: VALIDATED** (lead, 2026-10-12; implemented). The user chose **option A**: every public attribute, constructor arguments stored on `self` included, goes under the class docstring's `Attributes:`; an `Attributes:` section in the `__init__` docstring does not count. Question was: must a constructor argument stored on `self` also be listed under `Attributes:`?

Inputs: [01_understand.md](01_understand.md), [02_references.md](02_references.md), [03_design.md](03_design.md) (D9, Q2), [complex_function_docstring/03_design.md](../complex_function_docstring/03_design.md) (`undocumented-parameter`: D14, D17, E27), Google pyguide §3.8.3 and §3.8.4, the legacy `ClassDocstringRule` (`scratch/legacy_reference/legacy_lints.py`, lines 637–698). Markers: **[run]** = probed in this study (Sphinx 8 + Napoleon, pydoclint 0.11.0 via `uv`), **[U]** = not verified.

## 1. The options

| # | `undocumented-public-attribute` requires under `Attributes:` | `undocumented-parameter` requires under `Args:` |
| :-- | :--- | :--- |
| A | Every public attribute, including arguments stored on `self` (Google, current D9). | Every parameter (as designed). |
| B | Every public attribute whose name is not an `__init__` parameter (the legacy linter: `extra_attrs = attributes - params`). | Every parameter. |
| C | Like A, except an attribute whose first definition is `self.x = x` (plain copy of the same-named parameter). `self.x = max(x, 1)`, `self.x = list(x)`, `self.x = x or []` are required. | Every parameter. |
| D1 | Every public attribute, but an entry under the constructor's `Args:` (class or `__init__` docstring) also counts. | Every parameter. |
| D2 | As D1. | Every parameter, but an `Attributes:` entry also counts for a parameter stored on `self` ("one entry, anywhere"). |
| D3 | As A. | Every parameter, but an `Attributes:` entry in the class docstring also counts for a parameter stored **unchanged** (`self.x = x`). |

D1 and D2 are the "either section" idea; D3 is the variant this study found worth adding: it keeps the attribute rule exactly as A and moves the deduplication into `undocumented-parameter`, only where both entries would state the same fact.

## 2. Example and exact diagnostics

One class with four public attributes: `url` stored unchanged, `retries` and `tags` transformed, `attempts` not a parameter.

```python
class Fetcher:
    """Fetches pages with retries.

    Attributes:
        url: Page being fetched.
        retries: Attempts per fetch, at least 1.
        tags: Labels attached to every request.
        attempts: Attempts made so far.
    """

    def __init__(self, url: str, retries: int, tags: Iterable[str]) -> None:
        """Initializes the fetcher.

        Args:
            url: Page to fetch.
            retries: Attempts per fetch; values below 1 mean 1.
            tags: Labels attached to every request.
        """
        self.url = url
        self.retries = max(retries, 1)
        self.tags = list(tags)
        self.attempts = 0
```

That is layout L1 (Google). The other layouts change only the docstrings:

| Layout | Class `Attributes:` | `Args:` (class or `__init__`) |
| :--- | :--- | :--- |
| L1 Google | url, retries, tags, attempts | url, retries, tags |
| L2 legacy linter | attempts | url, retries, tags |
| L3 Middle | retries, tags, attempts | url, retries, tags |
| L4 Attributes only, short `__init__` (no docstring, < 20 code lines) | url, retries, tags, attempts | none |
| L5 Attributes only, `__init__` ≥ 20 code lines | url, retries, tags, attempts | none |
| L6 Arguments undocumented, long `__init__` | attempts | none |

Messages (from the two designs; `{function}` assumed to render as `Fetcher.__init__`):

- UPA: "Public attribute `url` of `Fetcher` is not listed under `Attributes:` in the class docstring." One finding per name, on its `self.url` target.
- UP: "Docstring of `Fetcher.__init__` does not document `url`, `retries`, `tags`." One finding per function.

Findings per layout (UPA names / UP names; "–" = clean):

| Layout | A | B | C | D1 | D2 | D3 |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| L1 | – / – | – / – | – / – | – / – | – / – | – / – |
| L2 | url, retries, tags / – | – / – | retries, tags / – | – / – | – / – | url, retries, tags / – |
| L3 | url / – | – / – | – / – | – / – | – / – | url / – |
| L4 | – / – (E27 gate) | – / – | – / – | – / – | – / – | – / – |
| L5 | – / url, retries, tags | – / url, retries, tags | – / url, retries, tags | – / url, retries, tags | – / – | – / retries, tags |
| L6 | url, retries, tags / url, retries, tags | – / url, retries, tags | retries, tags / url, retries, tags | url, retries, tags / url, retries, tags | url, retries, tags / url, retries, tags | url, retries, tags / url, retries, tags |

Entries the author must write to satisfy both rules (L1-like code):

| | A | B | C | D1 | D2 | D3 |
| :--- | :-- | :-- | :-- | :-- | :-- | :-- |
| `url` (unchanged) | 2 | 1 (Args) | 1 (Args) | 1 (Args) | 1 (either) | 1 (Attributes) |
| `retries`, `tags` (transformed) | 2 each | 1 each (Args) | 2 each | 1 each (Args) | 1 each (either) | 2 each |
| `attempts` | 1 | 1 | 1 | 1 | 1 | 1 |
| **Total** | **7** | **4** | **6** | **4** | **4** | **6** |

Observations:

- L6 (nothing documented) is reported twice per name under A, D1, D2 and D3 (under C, only the transformed names): once by each rule. Under B it is reported once. Each rule's message names its own section, so the double report is two distinct fixes, not noise, under A/C/D3; under D1/D2 one entry fixes both, so the second finding is redundant.
- D1 is B in disguise while `undocumented-parameter` is enabled and in scope: UP already forces every argument into `Args:`, which then satisfies UPA. D1 differs from B only when UP is disabled or gated off (E27): then D1 still demands one entry per argument somewhere, B demands none.
- L4 passes everywhere because D17/E27 keep a short, docstring-less `__init__` under an `Attributes:`-only class docstring out of UP's scope. The duplication that A imposes is therefore enforced only on constructors that are long or have an `Args:` section.

## 3. What readers get

**Sphinx + Napoleon [run].** Napoleon turns the class `Attributes:` into `.. attribute::` entries and an `Args:` into `:param:` fields; they render as two separate lists. With autodoc's default `autoclass_content = "class"`, **the `__init__` docstring is not rendered at all**: for L2 (the legacy linter), the class page shows only `attempts`; `url`, `retries` and `tags` appear nowhere but in the signature. With `autoclass_content = "both"`, the page shows the Attributes list followed by a separate "Parameters" list. An `Args:` in the class docstring renders as "Parameters" in both modes (not probed).

**IDE hover [U].** Pyright/Pylance show the class docstring on class hover, and the `__init__` docstring in call signature help. Hovering `obj.url` shows the declared type and only a PEP 257-style attribute docstring (a string literal after the assignment), not the `Attributes:` entry.

| Option | Class hover / default Sphinx lists `url` | Call-site help describes `url` | Two descriptions to read |
| :--- | :--- | :--- | :--- |
| A | Yes | Yes (Args in `__init__`) | Yes, possibly diverging |
| B | No (unless Args sits in the class docstring) | Yes | No |
| C | No for unchanged args; yes for transformed | Yes | Only for transformed |
| D1 | Same as B | Same as B | No |
| D2 | Depends on where the author put it | Depends | No |
| D3 | Yes | No for unchanged args documented only in Attributes | Only for transformed |

## 4. Drift and duplication

| Option | Duplication | Drift risk |
| :--- | :--- | :--- |
| A | One extra entry per stored argument; ~89% of stored arguments are plain copies (§7), so most duplicate entries say nearly the same thing. | Two free texts per argument, never compared. A rename is caught by both presence checks; the stale old entry is not (no stale-entry rule, Q3). Google's own example already words the two entries differently. |
| B | None. | A later change from `self.x = x` to `self.x = list(x)` makes the attribute differ from the argument silently. The exclusion is by name only, so `self.name = f"{name}-{id}"` or `self.config = parse(config)` is exempt too. The instance state is never described as state. |
| C | Only where the facts differ. | Changing `self.x = x` to a transform raises a UPA finding at that moment, which is the right time. A pre-assignment rebinding (`url = url.strip()` then `self.url = url`) is treated as unchanged (false negative, not measured). |
| D1 | None. | Same as B. |
| D2 | None. | Same as B, plus placement drift: half the arguments in each section is legal. |
| D3 | Only for transformed arguments. | Same as C, mirrored: an unchanged argument's input contract (validation, units) has only the attribute text. |

## 5. Dataclasses, attrs, Pydantic

No explicit `__init__`, so `undocumented-parameter` checks nothing (its D20) and the legacy linter's `attributes - params` subtracts an empty set. **All six options behave identically**: every field needs one `Attributes:` entry, written once. Corpus: 90% of detailed record-type docstrings already list their fields under `Attributes:`, none under `Args:` (§7). D1/D2 would only differ if they synthesized a constructor from the fields to accept `Args:` entries; nobody writes that, so it is not worth building.

Side finding for D8 (any option): `dataclasses.InitVar[...]` pseudo-fields are constructor-only and never attributes, but a class-body annotated assignment; D8 would require them under `Attributes:`. Suggest skipping `InitVar` annotations. `field(init=False)` fields are attributes and not arguments: correct as designed.

## 6. Agreement with references

| Option | Google §3.8.4 / §3.8.3 | pydoclint 0.11.0 [run] | Pylint docparams | Legacy linter |
| :--- | :--- | :--- | :--- | :--- |
| A | Matches the example (`likes_spam` in both). | Neutral: it rejects *every* `Attributes:` entry for a `self.x` attribute (`DOC602`/`DOC603`) in all four probed layouts, including the legacy linter's lone `attempts`. | No attribute check; requires constructor params in `Args:` (`W9015`), like strict UP. | Diverges (stricter). |
| B | Contradicts the example. | Neutral (same). | Same as A. | Matches. |
| C | Partly (drops `likes_spam`, a plain copy). | Neutral. | Same as A. | Partly. |
| D1 | Contradicts the example's intent on attributes. | Neutral. | Same as A. | ≈ Matches. |
| D2 | Relaxes §3.8.3 "List each parameter by name". | Against: an Attributes-only constructor gets `DOC101`/`DOC103` [run], even with no `__init__` docstring. | Against (`W9015`). | Diverges. |
| D3 | Matches §3.8.4; relaxes §3.8.3 for plain copies. | Against for those (same `DOC101`/`DOC103`). | Against for those. | Diverges. |

## 7. google3 corpus

Method: script `attr_args_corpus.py` in this worker's scratch directory, read-only over `/google/src/files/head/depot/google3`. Random descent into the 412 top-level directories (minus `third_party`, `experimental`), at most 25 non-test `.py` files each. Heuristic Google-section parser (headers `Args`/`Arguments`/`Parameters`/`Attributes`, entries at the first indent). A "stored argument" is a public `self.<p> = …` in `__init__` where `<p>` is a parameter; "unchanged" = right-hand side is exactly the name `<p>`; "None-default" = `p or …` / `p if … else …`. Two independent samples:

| Measure | Sample 1 | Sample 2 |
| :--- | --: | --: |
| Files parsed / classes | 3,898 / 4,834 | 3,885 / 4,773 |
| Classes storing ≥ 1 argument publicly | 680 | 605 |
| Stored arguments: unchanged / None-default / transformed | 1,477 / 78 / 96 (89 / 5 / 6%) | 1,307 / 77 / 88 (89 / 5 / 6%) |
| Arguments stored privately (`self._p = p`, out of scope) | 1,129 | 1,212 |
| Classes with stored args and **no** section anywhere | 385 | 382 |

Where stored arguments are documented, in classes with at least one section (names):

| | Sample 1 (779) | Sample 2 (668) |
| :--- | --: | --: |
| `Args:` only | 431 (55%) | 454 (68%) |
| `Attributes:` only | 260 (33%) | 166 (25%) |
| Both | 79 (10%) | 36 (5%) |
| Neither | 9 (1%) | 12 (2%) |

Classes **that have an `Attributes:` section** (131 and 73): stored arguments listed under Attributes only (no `Args:`) in 62% / 63%, in both in 33% / 27%, excluded from Attributes legacy-style in 1 / 2 classes (≈1%).

Record types (dataclass, attrs, NamedTuple, Pydantic) with a multi-line docstring and public fields: 255 / 271; fields under `Attributes:` in 227 / 244 (89% / 90%), under `Args:` in 0 / 0.

Gate-aware findings on stored arguments, sample 2 (668 names; UPA gate = multi-line class docstring, UP gate = D17 approximated by ≥ 20 statement lines or an `Args:` section):

| | A | B | C | D1 / D2 | D3 |
| :--- | --: | --: | --: | --: | --: |
| UPA findings | 102 | 0 | 20 | 7 | 102 |
| UP findings (names) | 22 | 22 | 22 | 22 / ≤ 7 | 6 |

Not counted: classes with no section at all (382 in sample 2); under A, a multi-line section-less docstring there yields one UPA finding per stored argument on top of the non-argument attributes all options flag.

Reading: authors who write an `Attributes:` section almost always list stored arguments in it (≈ 90–95%); the legacy linter's exclusion is practically unused. Authors who do not write one mostly document arguments under `Args:` with a one-line class docstring, which E5 exempts, so A's rollout cost is moderate (102 findings ≈ 15% of stored names). Plain copies dominate (89%), so C would exempt almost everything A requires.

## 8. Implementation and precision

| Option | Work in `undocumented-public-attribute` | Work in `undocumented-parameter` | Precision |
| :--- | :--- | :--- | :--- |
| A | None beyond D8. | None. | Exact. |
| B | Read the direct `__init__`'s parameter names (positional-only, keyword-only, variadic); set difference. | None. | Name match only: exempts transformed and unrelated values. |
| C | H2 must expose the assignment's value node; exempt when it is a bare `Name` equal to a parameter of the same `__init__` (annotated assignments too, not in `__post_init__`). One more exemption, pass case and mutation entry. | None. | Syntactic; misses pre-assignment rebinding. |
| D1 | Find the constructor (withdrawn H1) and read both docstrings' `Args:`; message must name both sections. | None. | Exact; double reports on L6. |
| D2 | As D1. | Read class `Attributes:` and `self.<p>` assignments. | Exact; coupling both ways. |
| D3 | None (= A). | Read class `Attributes:` and the first `self.<p>` assignment's value (needs H2 from this design). | As C, on the other rule. |

## 9. Recommendation

**A for `undocumented-public-attribute` (keep D9 and Q2's "yes").**

1. It is Google's literal rule and example, and it is the only option under which every attribute appears on class hover and on a default Sphinx page (B/D1 leave arguments documented in `__init__` invisible there [run]).
2. It matches practice where `Attributes:` is used at all (≈ 90–95% list stored arguments; the legacy linter's exclusion ≈ 1%), and it treats dataclass fields and `self.x = x` the same way.
3. It is the simplest and exact: no right-hand-side heuristics, no coupling between rules.
4. Cost is moderate thanks to E5: ≈ 15% of stored arguments in the sample would be flagged, mostly by adding an `Attributes:` entry beside an existing `Args:` one.

The duplication cost of A only bites where `undocumented-parameter` is in scope (long or sectioned constructors). If the user finds writing a plain copy twice unacceptable, the precise relief is **D3**, a change to `undocumented-parameter`, not to this rule: it removed 16 of 22 UP findings in the sample, keeps the attribute visible to readers, and still requires both entries when the argument is transformed. I do not recommend it now (it relaxes §3.8.3, disagrees with pydoclint and Pylint, and couples the two rules for few findings); it is a ROADMAP candidate for the parameter rule's owner. C is not recommended: it exempts 89% of stored arguments, so readers lose the attribute where it is most common, for an extra heuristic. D1/D2 are not recommended: D1 equals B in practice with a redundant double report, D2 makes placement arbitrary.

Side finding for D8: exempt `InitVar` annotations (§5).
