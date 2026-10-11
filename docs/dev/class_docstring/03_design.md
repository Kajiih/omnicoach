# Class docstring completeness: 03 Design

> [!IMPORTANT]
> **Status: PENDING** (lead validation). Verdict: **narrow** to one rule, `undocumented-public-attribute`. The constructor-parameter half goes to `undocumented-parameter`, owned by [complex_function_docstring](../complex_function_docstring/03_design.md) (D16).

## 1. Decisions

| # | Decision | Why |
| :-- | :--- | :--- |
| D1 | **Two defects, two rules**: an undocumented constructor parameter (call contract, `Args:`) and an undocumented public attribute (data contract, `Attributes:`). | Split test (rule_design_guide.md §1, user decision (c)): they differ in why they are bad and in the section, and possibly the docstring, that fixes them (01_understand.md §4). |
| D16 | **The constructor half is not a rule of this design.** It is the `__init__` case of `undocumented-parameter` (all functions, accepts `__init__` parameters documented in the class docstring, its D14). This document keeps only `undocumented-public-attribute`. Constructor-specific findings are handed over in §8. | Lead consolidation (2026-10-11): one antipattern, one rule. A separate `undocumented-constructor-parameter` would flag the same parameter twice. |
| D2 | **Implement, not drop**, despite pydoclint coverage of class-body attributes. | User decision (a). pydoclint never requires `self.x` attributes and rejects Google's own §3.8.4 example (02_references.md G1, G2, both confirmed by the lead's run). |
| D3 | **A missing class docstring is out of scope.** | Ruff `D101`, Pylint `C0115`. Requiring entries in a docstring that does not exist would duplicate them. |
| D4 | **A summary-only class docstring is exempt** (E5): at most one non-blank line (`PythonDocstring::non_blank_line_count() <= 1`, or the lead's `PythonDocstring::summary()` once it lands). | pydoclint's default `skip-checking-short-docstrings = true` and the legacy linter (P1) agree. Google §3.8.4 gives classes no explicit one-line exception: see Q1. The perverse incentive (collapse to one line) is stated in `known_problems`. |
| D8 | **Attributes are** (a) class-body assignments `name = …`, `name: T = …`, `name: T` (each `Name` target, including multiple and tuple-unpacking targets), and (b) assignments `<receiver>.name = …` / `<receiver>.name: T = …` (including tuple-unpacking targets) directly in an instance-initializer method: `__init__`, `__post_init__`, `__attrs_post_init__`, `model_post_init`. The receiver is the method's first parameter (existing `method_receiver_name_ast`), not the literal `self`. Nested functions, lambdas and classes are not entered (E10). Other methods are not read (E11). | Google counts public attributes wherever they are created; its example only has `self.x`. Initializer hooks are where synthesized classes create derived state. Attributes first created in arbitrary methods are Pylint `W0201` (`attribute-defined-outside-init`); reading every method would push the fix toward documenting a smell. |
| D9 | **Init parameters stored on the instance are attributes too.** `self.host = host` needs `host` under `Attributes:`, whatever `Args:` says. | Google's example lists `likes_spam` under both `Args:` (of `__init__`) and `Attributes:`. The legacy linter's exclusion (P5) is dropped: see Q2. |
| D10 | **Not attributes**: `_`-prefixed names (private and dunder, E6); names the class body binds with `def` (properties, methods, and `self.x = …` assigning through a property setter, E7); every member of an Enum class (E8); names in the `allowed` list, default `["model_config"]` (E9). Dataclass, attrs, NamedTuple, TypedDict, Pydantic and Protocol fields **are** attributes. `ClassVar` and constants **are** attributes. Inherited attributes are not required unless re-assigned in this class. | Google: "Public attributes, excluding properties". Enum members are the type's values, not instance state; the legacy linter and pydoclint (G4) both demand them. `model_config` is Pydantic configuration, not a field. Fields of record types are exactly the data contract `Attributes:` describes. A project-tunable list follows rule_design_guide.md §5. |
| D11 | **Anchor**: the first definition of the attribute in source order (the class-body target, or the `<receiver>.name` target). One finding per missing name; no separate "section missing" finding. | Points at the undocumented thing and lets a suppression cover one name (the legacy linter P8 put every finding on the `class` line). One template (decision (c)). After `inline-public-attribute-annotation`'s fix, the anchor moves from `self.retries: int = 3` to the class-body `retries: int`; the verdict does not change (01_understand.md §3). |
| D12 | **Google-style sections only**, through the shared parser (`DocstringSectionKind::Attributes`). An entry under `Args:` does not document an attribute. | Fixes the legacy linter P6's header matching. NumPy / Sphinx docstrings are a stated limit (`known_problems`), not detected. |
| D13 | **Scope: Python only, `RuleTarget::SourceOnly`, every class including nested and private ones.** | Test classes are not an API (same target as `inline-public-attribute-annotation`). A private class that has a detailed docstring should still be complete; the legacy linter and pydoclint do not exempt private classes either. |
| D14 | **No stale-entry, order or type checks.** | Different defects with different fixes (Q3); order and types are pydoclint `DOC604`/`DOC605`. |
| D15 | **Classification**: topic `docstrings` (new, Q5), `Precision::Heuristic` (a subclass re-assigning an inherited attribute in `__init__` is flagged though the base documents it, and an Enum is recognized only by its direct base), `Consensus::Opinionated` (typed, self-describing code without attribute lists is ordinary Python), `ImpactedQuality::Maintainability`. | tag_guide.md §2.2–§2.4. |

D5, D6 and D7 of the first draft (constructor `Args:` placement, which `__init__` is read, which parameters count) moved to the hand-over in §8.

## 2. Exemptions

Each gets one dedicated `pass` case and one entry in `scripts/exemption_mutations.py`. E1–E4 of the first draft are hand-over notes (§8), not exemptions of this rule.

| # | Exemption | Pass case (sketch) |
| :-- | :--- | :--- |
| E5 | Class docstring summary-only (or absent). | `class Point: """A 2-D point."""` with `x: float`. |
| E6 | `_`-prefixed names (private, dunder). | `self._cache = {}`, `__slots__ = ("x",)`. |
| E7 | Names bound by `def` in the class body. | `@property def area(…)` and a setter assigned as `self.area = …` in `__init__`. |
| E8 | Enum classes. | `class Color(Enum):` with a multi-line docstring and `RED = 1`. |
| E9 | Names in `allowed` (default `model_config`). | Pydantic model with `model_config = ConfigDict(frozen=True)`. |
| E10 | Assignments inside nested functions, lambdas or classes in an initializer. | `def __init__(self): def bind(other): other.leak = 1`. |
| E11 | Attributes first assigned outside initializer methods. | `def connect(self): self.socket = …`. |

## 3. Message

```rust
summary: "Public attribute `{name}` of `{class}` is not listed under `Attributes:` in the class docstring.",
rationale: "Readers learn an instance's state from the class docstring, so an unlisted attribute is only discoverable by reading every assignment in the class.",
suggestion: "Add a `{name}:` entry with its description under `Attributes:` in the docstring of `{class}`.",
```

Placeholders `{name}` and `{class}` are in the vocabulary; `Add` is in the verb list. Doc summary: "Requires detailed Python class docstrings to list every public attribute under `Attributes:`."

## 4. Documentation content (Prediction Test)

`what_it_does`: when the class docstring has more than a summary line, every public attribute created in the class body or by `self.<name> = …` in `__init__` (or a post-init hook) needs an entry under `Attributes:`, including dataclass fields and parameters stored on `self`. Properties, methods, `_`-prefixed names and Enum members are not attributes. Constructor parameters are `undocumented-parameter`'s concern.

`known_problems`: re-assigning an inherited attribute in a subclass constructor is flagged; Enum subclasses of a project Enum are not recognized; attributes created in other methods are not seen; NumPy and Sphinx docstrings are flagged; collapsing the class docstring to one line silences the rule.

References: Google §3.8.4; pydoclint `DOC601`–`DOC603` and its instance-attribute gap.

Example (Python only), flagged span `eggs`:

```python
class SampleClass:
    """Summary of class here.

    Attributes:
        likes_spam: A boolean indicating if we like SPAM or not.
    """

    def __init__(self, likes_spam: bool = False) -> None:
        self.likes_spam = likes_spam
        self.eggs = 0
```

## 5. Options

One `ListOption` of kind `allowed` (attribute names never required), Python default `["model_config"]`, plus the enforcement mode from `RuleOptions::code_rule`.

## 6. Helpers

Uses `src/code_lint/ast/python/docstrings.rs` as landed: `PythonClassInfo::docstring`, `PythonDocstring::{non_blank_line_count, sections}` (or `summary()`, lead-owned), `DocstringSection::{kind, entries}`, `DocstringEntry::name`. Proposed additions in `ast/python/classes.rs`, built in Phase 4:

| # | Change | Reason |
| :-- | :--- | :--- |
| H2 | `PythonClassInfo::attribute_definitions() -> Vec<PythonAttributeDefinition>` with `name` and `node`: the first definition of each name in source order, per D8. Needs a `pub(super)` reference to the class body. | No existing walker collects unannotated class-body assignments or unannotated `self.x` (`collect_class_attributes` and `collect_self_annotated_assignments` read annotated ones only). |
| H3 | `PythonClassInfo::defines_function(name) -> bool` and `PythonClassInfo::is_enum() -> bool` (direct base resolving to `enum.Enum`, `IntEnum`, `StrEnum`, `Flag`, `IntFlag`, `ReprEnum`). | E7 and E8 stay visible in the rule file, where the mutation script can disable them. |

H1 (`PythonClassInfo::constructor()`) is withdrawn with D16; `undocumented-parameter` may still need it to find the class docstring of an `__init__` (§8). `DocstringEntry::bare_name()` (H4) is lead-owned.

`rule_test!` doubling: each class is analysed alone and nothing reasons across top-level definitions, so the duplicated copy reports exactly twice.

## 7. Verification plan (Phase 4)

1. Fail cases: undocumented constructor `self.x`; parameter stored on `self` but missing from `Attributes:` (documented only under `Args:`); dataclass field; class-body constant; attribute in `__post_init__`; tuple-unpacking `self.a, self.b = …`; no `Attributes:` section with two attributes → two findings at their own anchors → check: each case reports exactly its expected spans.
2. One pass case per E5–E11 → check: `scripts/exemption_mutations.py` fails each with its exemption disabled.
3. Full verification from adding_a_rule.md step 9 and `omnicoach code .`.

## 8. Hand-over to `undocumented-parameter`

Constructor findings from this analysis, for the owner of [complex_function_docstring](../complex_function_docstring/03_design.md). They are notes, not exemptions of this rule:

- **Gate (former D4/E1).** For `__init__`, treat the constructor as documented in detail when the class docstring *or* the `__init__` docstring has more than a summary line. The legacy linter gated on the class docstring alone; Google §3.8.4 puts `Args:` in the `__init__` docstring.
- **Placement (former D5/E3).** Accept the entry in either docstring's `Args`-kind section (Pylint docparams: "either the class docstring or the `__init__` docstring, but not both"). pydoclint forces one side and, with its default, flags Google's `SampleClass` with `DOC301`/`DOC101`/`DOC103` (02_references.md §2, confirmed by the lead's run).
- **Which `__init__` (former D6/E2).** The direct `def __init__` of the class body, skipping `@overload` stubs: parameters come from the implementation. No explicit `__init__` (dataclass, inherited) → nothing to check; fields are `undocumented-public-attribute`'s concern.
- **Which parameters (former D7/E4).** Every non-receiver parameter, including positional-only and `*args` / `**kwargs` (the legacy linter ignored all three, P2). Match entries with or without the `*` / `**` prefix (`DocstringEntry::bare_name()`).
- **Overlap.** Ruff `D417` also reports a partial `Args:` section of the `__init__` docstring.

Outcome (owner's reply, 2026-10-11): placement, `@overload`, parameters and the `D417` overlap are adopted in complex_function_docstring/03_design.md. The gate differs: the owner kept its function-wide gate (its D17: long `__init__`, or a section in either docstring), which avoids the collapse-to-one-line incentive; the unchecked short `__init__` under a section-less multi-line class docstring is its E27 and its Q8.

## 9. Open questions for the user

| # | Question | Recommendation |
| :-- | :--- | :--- |
| Q1 | Should one-line class docstrings be checked for attributes? Google §3.8.4 gives classes no one-line exception, so `class Point: """A 2-D point."""` with fields violates it literally. | **Skip** (E5): matches pydoclint's default and the legacy linter, and small value objects would otherwise need a full docstring. |
| Q2 | Should init parameters stored on `self` also need an `Attributes:` entry (D9, Google's example), breaking with the legacy linter's exclusion? | **Yes**, follow Google: a parameter and the attribute it initializes are different facts (a caller's input vs the instance's state) and may diverge. |
| Q3 | Should stale entries (a documented attribute that does not exist) become a rule? | **Not now; ROADMAP candidate** `stale-docstring-entry`. Stale `Attributes:` needs inherited attributes, other methods and properties to avoid false positives, which a single-file AST cannot give. |
| Q5 | New topic `docstrings` for Batch D (lead-owned, shared). | Admit it: a construct, every Batch D rule deserves it. |

Q4 and Q6 of the first draft (generalizing the constructor rule, `Args:` placement option) are resolved by D16; placement is now `undocumented-parameter`'s decision.

## 10. ROADMAP suggestion: Rust

No rule. Public struct fields are covered by rustc's `missing_docs` lint (allow-by-default), which requires a `///` comment on every public field. Suggested *Not pursued* entry: "Rust analogue of `undocumented-public-attribute`: covered by `#![warn(missing_docs)]`."
