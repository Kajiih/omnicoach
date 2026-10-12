# Complex function docstring: 03 Design

> [!IMPORTANT]
> **Status: VALIDATED** (lead, 2026-10-12; implemented as `undocumented-nontrivial-function`, `unexplained-parameter` and `undocumented-tuple-return`).

> [!NOTE]
> **Implemented rules** (see [03b_complexity_gate.md](03b_complexity_gate.md), [03c_section_requirements.md](03c_section_requirements.md) and [04_execution_log.md](04_execution_log.md)):
> 1. `undocumented-nontrivial-function`: replaces the line gate (D4, D5, E1) with `statements >= 12 OR cognitive complexity >= 6` (options `min-statements`, `min-cognitive-complexity`).
> 2. `unexplained-parameter`: flags a nontrivial function (`statements >= 12 OR cognitive >= 6`) whose docstring has no `Args:` section and has at least one `Optional`/`= None`, `bool`/`= True`/`= False`, `Callable`, or `*args`/`**kwargs` parameter (incomplete `Args:` sections are covered by Ruff `D417`; constructor parameters stored unchanged on `self` and documented under class `Attributes:` are exempt).
> 3. `undocumented-tuple-return`: flags a documented function at any size returning or yielding a positional tuple of `>= 2` elements without `...` when the docstring has neither a `Returns:`/`Yields:` section nor a `Return(s)`/`Yield(s)` summary.

## 1. Verdict

The gate (`batch_d_status.md`) proposed dropping the check in favor of Pylint configuration. Under the batch decision (a), the rule is designed anyway, because the external tools leave the gaps G1–G10 (`02_references.md` §4) and the configuration that comes closest needs Pylint plus a plugin plus four options.

| Rule | Flags | The legacy linter part |
| :--- | :--- | :--- |
| `undocumented-long-function` | A function of at least `min-lines` code lines with no docstring (V1). | Implicit (B5) |
| `undocumented-parameter` | A docstring that omits parameters from its `Args:` sections (V2). | `Args:` half |
| `undocumented-return-value` | A docstring that does not describe the value a function returns or yields (V3). | `Returns:` half |

V4 (`Raises:`) is not ported (D9).

## 2. Decisions

### Scope and split

- **D1. Three rules, one template each.** V1, V2, V3 differ in why they are bad and how they are fixed (`01_understand.md` §3). Incomplete `Args:` entries and a missing `Args:` section stay one rule: the harm (a caller cannot learn what to pass) and the fix (add the entry) are the same.
- **D2. One finding per function, never three.** Without a docstring, only `undocumented-long-function` reports; the other two rules only inspect existing docstrings. This also matches Pylint (`check_arguments_in_docstring` returns on an empty docstring).
- **D3. Python only.** Rustdoc has no per-argument or return section convention. A Rust analogue of `undocumented-long-function` is a ROADMAP suggestion (§7).

### When documentation is required

- **D4. Size signal: code lines of the body.** Count the lines from the first statement after the docstring to the end of the function, skipping blank lines and comment-only lines. Nested functions count. This removes B3/G1 (documenting a function never puts it in scope) and most of G10 (comments and blank lines do not count). Lines are kept over statements or branches because a reader can check them at a glance, which the Prediction Test favors, and because Google's trigger is "nontrivial size". Argument-per-line formatting still inflates the count; that goes in `known_problems`. Branch complexity ("non-obvious logic", the third Google trigger) is a different signal and is not added (Q1).
- **D5. Option `min-lines`, default 20, declared by each rule.** the legacy linter's 25 physical lines included the `def` line, the docstring and blank lines; 20 code lines is roughly the same size. Each rule declares its own `CountOption` (rule files cannot share consts); the same default is used in all three. A project that wants one threshold sets it three times (Q2).
- **D6. A structured docstring is checked at any size.** `undocumented-parameter` and `undocumented-return-value` apply when the function has at least `min-lines` code lines **or** its docstring already has an `Args:`, `Returns:`, `Yields:` or `Raises:` section. Google lets sections be omitted only when a one-line docstring suffices (§3.8.3); once the author writes sections, an omitted parameter or return value is an omission, whatever the size. This closes G3 (the legacy linter, Pylint) and G2 (pydoclint, Pylint defaults) together. `Example:` / `Note:` sections do not make a docstring structured.
- **D7. Private functions are checked.** Google's size trigger is independent of visibility. This deliberately differs from the legacy linter (`__x`), Pylint (`^_`) and Ruff D1xx (G4).
- **D8. Test files are skipped (`RuleTarget::SourceOnly`).** As in the legacy linter. Google §3.8.2.1 (Test modules) says "method docstrings are not required on `test_` methods in subclasses of `unittest.TestCase`" and that "Docstrings that do not provide any new information should not be used"; long tests are better fixed by splitting than by documenting.

### What counts as documented

- **D9. `Raises:` is out of scope.** It never satisfies `Returns:` (fixes B1/G5; a regression `fail` case covers it). No raise rule is added: Google wants only exceptions "relevant to the interface" and not API-violation errors, which no syntactic check can tell apart, so a raise rule would flag correct code by design. Ruff DOC501 / pydoclint DOC501 remain available for teams that want it.
- **D10. Parameter matching.** Required: every parameter in the signature (positional-only, regular, keyword-only, `*args`, `**kwargs`). Documented: every entry name of every `Args`-kind section (`Args`, `Arguments`, `Parameters`, `Params`, `Keyword Args`, `Keyword Arguments`, `Other Parameters`), compared after stripping leading `*`. Entry text and order are not checked (other Batch D rules read the text).
- **D11. A value-returning function** is one whose return annotation is present and is not `None`, `"None"`, `NoReturn` or `Never`, or, when unannotated, whose own body has a `return <expr>` whose expression is not the `None` literal. A generator is a function whose own body has `yield` or `yield from`. Nested `def`, `lambda` and `class` bodies are not the function's own body. Using both the annotation and the statements fixes G9 and avoids the statement-only blind spot of Ruff and Pylint for annotated stubs.
- **D12. Which section satisfies `undocumented-return-value`.** An undecorated generator needs `Yields:` (Google: "`Yields:` for generators"). Every other function accepts `Returns:` or `Yields:`: a decorated generator documents the decorated behavior (Google's `@contextmanager` example uses `Returns:`, G7), and an interface stub annotated `-> Iterator[T]` may describe what it yields. Checking that the section kind matches the function is a different antipattern (pydoclint DOC403) and stays out.
- **D13. A summary starting with `Return`, `Returns`, `Yield` or `Yields` satisfies `undocumented-return-value`.** Google allows it when the sentence suffices; the rule cannot judge sufficiency and trusts the author (G6). This is the summary line, not the `Returns:` body that `RedundantReturnsDocstringRule` inspects.
- **D14. Constructor parameters may be documented in the class docstring.** For `__init__` and `__new__`, the `Args:` entries of the directly enclosing class's docstring count too. Google's example uses the `__init__` docstring, pydoclint's default the class docstring, and Pylint accepts either; accepting both keeps omnicoach compatible with the pydoclint configuration recommended for the dropped `ClassDocstringRule` (Q5). The class_docstring worker reached the same decision independently (its D5). The lead verified that pydoclint's defaults reject Google's own §3.8.4 `SampleClass` (DOC301, DOC101, DOC103), so following pydoclint's placement alone would flag the style guide's example.

### Locations and messages

- **D15. Spans.** `undocumented-long-function`: the function name. `undocumented-parameter`: the header line of the first `Args`-kind section (`PythonDocstring::line_node`), or the docstring when there is none. `undocumented-return-value`: the docstring.
- **D16. `undocumented-parameter` reports once per function**, listing the missing parameters, so a docstring with no `Args:` section and six parameters yields one finding with one fix, and one suppression. This needs a new placeholder `{parameters}`, rendered bare with each name in its own backticks like `{duplicates}` (Q4).

### Constructors (folded in from `class_docstring`)

The lead dropped the class_docstring worker's `undocumented-constructor-parameter` (its D4–D7, E1–E4 in `../class_docstring/03_design.md`): `undocumented-parameter` owns constructor parameters.

- **D17. Constructor gate.** `__init__` / `__new__` are in scope when the constructor has at least `min-lines` code lines, **or** its own docstring **or** its class's docstring has an `Args:`, `Returns:`, `Yields:` or `Raises:` section. This is D6 with the class docstring added to the docstrings that document the constructor (D14). The class_docstring worker's gate was "the class or `__init__` docstring has more than one non-blank line". D6's section test is kept, for three reasons: (1) one gate for every function is easier to predict than a constructor-only variant; (2) a multi-line class docstring usually describes the class, not how to construct it, so prose length is no evidence that the author chose to document parameters; (3) a line-count gate rewards collapsing a docstring to one line, which that worker already listed as a perverse incentive, while the section gate only asks for completeness from authors who wrote sections. Cost: a short `__init__` with no docstring under a multi-line, section-less class docstring is not checked (E27 pins it); a long one is (D4). An `Attributes:` section alone does not make the class docstring structured: it documents attributes, not constructor arguments, and `undocumented-public-attribute` owns those.
- **D18. `@overload` stubs are skipped** by `undocumented-parameter` (E26). Through D17, a structured class docstring puts every `__init__` in scope, including `@overload` stubs whose parameter sets differ from the implementation's. The implementation's parameters are the ones to document. Other `@overload` stubs rarely have docstrings, and skipping them everywhere keeps the exemption simple.
- **D19. Positional-only parameters count, and entries match with or without their stars.** Already D10 and E8; both workers agree (its D7, E4).
- **D20. No explicit `__init__`, nothing to check.** Synthesized constructors (dataclass, attrs, `NamedTuple`, Pydantic) have no signature to read; their fields are attributes, owned by `undocumented-public-attribute`. Inherited constructors are documented on the base class. No exemption is needed: the rule iterates function definitions, and there is none.

## 3. Exemptions

Every exemption gets one dedicated `pass` case and an entry in `scripts/exemption_mutations.py`. Exemptions shared by several rules get one case per rule, since each rule can lose its copy independently.

### `undocumented-long-function`

| # | Exemption | Reason |
| :-- | :--- | :--- |
| E1 | Docstring, blank and comment-only lines do not count toward `min-lines`. | D4 |
| E2 | `@override` methods. | §3.8.3.1: documentation lives on the base method. |
| E3 | Data-model dunders other than `__init__`, `__new__`, `__call__`. | Their contract is the data model's; the legacy linter skipped them too. |
| E4 | `@<name>.setter` / `@<name>.deleter` accessors. | A property is documented once, on its getter. |
| E5 | `__init__` / `__new__` whose class has a docstring. | D14: the class docstring describes construction; requiring both contradicts pydoclint's default (DOC301). |

### `undocumented-parameter`

| # | Exemption | Reason |
| :-- | :--- | :--- |
| E6 | The receiver (`self` / `cls`). | Not passed by the caller. |
| E7 | Underscore-only names (`_`, `__`, `*_`). | Intentionally unused; pydoclint's default. |
| E8 | `*args` / `**kwargs` documented without their stars. | D10: Google lists them with stars, but the omission still documents the parameter; a formatting rule could own the stars. |
| E9 | Entries under `Keyword Args:` / `Other Parameters:` count. | D10 |
| E10 | Functions under `min-lines` with an unstructured docstring. | D6: Google lets sections be omitted when a one-liner suffices. |
| E11 | Constructor parameters documented in the class docstring. | D14 |
| E12 | `@override` methods. | As E2. |
| E13 | Data-model dunders. | As E3. |
| E14 | Property setters / deleters. | As E4. |
| E26 | `@overload` stubs, including `__init__` stubs under a structured class docstring. | D18 |
| E27 | Short `__init__` without a docstring whose class docstring is multi-line but has no `Args:` / `Returns:` / `Yields:` / `Raises:` section (for example only `Attributes:`). | D17 |

### `undocumented-return-value`

| # | Exemption | Reason |
| :-- | :--- | :--- |
| E15 | Return annotation `None`, `"None"`, `NoReturn`, `Never`. | D11 |
| E16 | Unannotated function with only bare `return` / `return None`. | D11 |
| E17 | `return <value>` inside a nested `def` / `lambda` / `class`. | D11 |
| E18 | `@property`, `@functools.cached_property` getters. | §3.8.3: a property is documented like an attribute. |
| E19 | Summary starts with `Return(s)` / `Yield(s)`. | D13 |
| E20 | Decorated generator documented with `Returns:`. | D12, Google's `@contextmanager` example. |
| E21 | Non-generator documented with `Yields:` (such as an `-> Iterator[T]` stub). | D12 |
| E22 | Functions under `min-lines` with an unstructured docstring. | D6 |
| E23 | `@override` methods. | As E2. |
| E24 | Data-model dunders. | As E3. |
| E25 | Property setters / deleters. | As E4. |

Not exemptions, but `fail` cases worth naming: a `-> int` function documented only with `Raises:` (B1); an undecorated generator documented with `Returns:` (D12); a short function whose `Args:` lists one of two parameters (D6); a private long function (D7); a long function whose `Args:` text mentions `Returns:` in prose (B2).

Abstract methods, `Protocol` members and `@overload` stubs need no exemption: their one-line bodies fall under `min-lines`, and a structured docstring on an interface method is exactly where a missing parameter matters.

## 4. Declarations

### `undocumented-long-function`

```rust
const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Function `{function}` has {count} code lines and no docstring.",
    rationale: "Without a docstring, a caller has to read the whole body to learn what the function does and how to call it.",
    suggestion: "Add a docstring to `{function}` that states what it does, what each parameter means and what it returns.",
};
```

- Doc summary: "Requires a docstring on functions of at least a configurable number of code lines."
- Classification: topic `docstrings` (new, Q3); `Precision::Heuristic` (line count stands in for "nontrivial size"); `Consensus::Opinionated`; `ImpactedQuality::Maintainability`.
- Option: `min-lines` (D5). Enforcement mode default `RequireExplanation`, as the canonical rule.
- References: Google pyguide §3.8.3; Pylint C0116 with `docstring-min-length`.

### `undocumented-parameter`

```rust
const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Docstring of `{function}` does not document {parameters}.",
    rationale: "A caller cannot tell what to pass for an undocumented parameter without reading how the body uses it.",
    suggestion: "Add an `Args:` entry for each of {parameters} to the docstring of `{function}`.",
};
```

- Doc summary: "Requires the docstring of a long or sectioned function to document every parameter in `Args:`."
- Classification: `docstrings`; `Heuristic` (the size branch of D6); `Opinionated`; `Maintainability`.
- References: Google §3.8.3 *Args:*; Ruff D417; pydoclint DOC101; Pylint `missing-param-doc`.

### `undocumented-return-value`

```rust
const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Docstring of `{function}` does not describe the value it returns.",
    rationale: "A caller cannot tell what the result means, such as what an empty or sentinel value signals, without reading the body.",
    suggestion: "Add a `{replacement}` section to the docstring of `{function}`, or start its summary with `Returns`.",
};
```

`{replacement}` is `Yields:` for an undecorated generator and `Returns:` otherwise.

- Doc summary: "Requires the docstring of a long or sectioned function to describe its return value."
- Classification: `docstrings`; `Heuristic`; `Opinionated`; `Maintainability`.
- References: Google §3.8.3 *Returns:*; Ruff DOC201 / DOC402; pydoclint DOC201 / DOC402; Pylint `missing-return-doc`.

`what_it_does` of each rule states D6 (structured docstrings are checked at any size) and the main hand-offs: no docstring is `undocumented-long-function`'s finding (D2), `@override` methods are skipped, and `Raises:` does not count (D9). `known_problems`: line counts depend on argument-per-line formatting (D4); D13 trusts any "Returns …" summary; `**kwargs: Unpack[TypedDict]` must still be documented (Ruff D417 exempts it); constructor parameters are looked up in the directly enclosing class only; a partial `Args:` section is also reported by Ruff `D417` when that rule is enabled (both fire).

## 5. Helper API needed

The lead's `docstrings.rs` covers docstring text, sections, entries and line spans; nothing there needs to change. The rules need these facts about the function, which `PythonFunctionSignature` keeps `pub(super)` today (`definition`, decorators):

| # | Helper (proposed, in `ast/python/functions.rs` unless noted) | Used by |
| :-- | :--- | :--- |
| H1 | `PythonFunctionSignature::body_code_line_count() -> usize`: lines after the docstring, not blank, not comment-only. Should use Ruff's comment ranges so `#` inside a multi-line string is not taken for a comment. | all three |
| H2 | `PythonFunctionSignature::is_generator() -> bool`: `yield` / `yield from` in the function's own scope. | `undocumented-return-value` |
| H3 | `PythonFunctionSignature::returns_value() -> bool`: D11 (annotation, else own-scope `return <non-None expr>`), with the `None` / `NoReturn` / `Never` matching done through the existing import-aware path matching. | `undocumented-return-value` |
| H4 | `PythonFunctionSignature::is_exempt_from_docstring_rules() -> bool`: `@override`, data-model dunder (reusing `is_exempt_dunder_method`), property setter / deleter. One fact shared by the three rules (E2–E4, E12–E14, E23–E25), so they cannot drift apart. | all three |
| H5 | `PythonFunctionSignature::is_property_getter() -> bool`: `@property`, `@functools.cached_property`. | `undocumented-return-value` |
| H6 | `PythonFunctionSignature::has_decorators() -> bool`. | `undocumented-return-value` (D12) |
| H7 | Dropped (lead decision). Constructors use the lead-owned `PythonClassInfo::constructor()` (the class's direct, non-`@overload` `__init__`): the rules iterate classes, take `constructor()` and read the class's `docstring`. | `undocumented-long-function` (E5), `undocumented-parameter` (E11, D17) |
| H8 | `PythonDocstring::summary()` and `DocstringEntry::bare_name()` (in `docstrings.rs`): **owned by the lead**. | `undocumented-return-value` (D13), `undocumented-parameter` (D10, E8) |
| H9 | `PythonFunctionSignature::is_overload() -> bool`: decorated with `@overload` (`typing` / `typing_extensions`). The fact is already part of `has_exempt_signature_decorator`, but not exposed on its own. | `undocumented-parameter` (E26) |

H1–H3 are new structural facts; H4–H6 and H9 expose decorator facts that `functions.rs` already computes privately.

## 6. Test plan (Phase 4 preview)

One `rule_test!` per rule. Beyond the exemption cases: the `fail` cases of §3; `async def` and async generators; a keyword-only and a positional-only parameter; `Args:` written in upper case (Napoleon is case-insensitive, B2); a nested long function inside a short one (both measured on their own lines, the outer one including the inner's lines). The harness duplicates each `fail` case in one module; the three rules reason per function only, so the duplicate needs no special handling.

## 7. ROADMAP suggestions

- **Rust `undocumented-long-function`.** rustc `missing_docs` covers public items and Clippy `missing_docs_in_private_items` covers every private item, with no size gate in between. A size-gated private-function rule would mirror the Python one. Parameters and return values have no rustdoc section convention, and `# Errors` / `# Panics` are covered by Clippy `missing_errors_doc` / `missing_panics_doc`.
- **Stale `Args:` entries** (a documented name that is not a parameter): a different fix (remove or rename the entry), covered by pydoclint DOC102 / Ruff DOC102 (preview) and Pylint `differing-param-doc`. Candidate only if the batch wants to replace those too.
- **Branch-complexity trigger** for "non-obvious logic", if Q1 keeps lines only.

## 8. Open questions

- **Q1.** Size signal: code lines (D4) or statements (formatter-independent, like Ruff `PLR0915`)? Should branch complexity also trigger the requirement?
- **Q2.** Default `min-lines = 20`, declared separately by each rule? The alternative is a single shared threshold, which the options framework does not support across rules today.
- **Q3.** New topic `docstrings` (description: "Docstring presence and content"; scope note excluding comments and suppression directives). The other ported Batch D rules would share it; the lead owns `taxonomy.rs`.
- **Q4.** One `undocumented-parameter` finding per function with a new `{parameters}` placeholder (D16), or one per parameter on its name, using the existing `{name}`?
- **Q5.** Accept constructor parameters in the class docstring (D14, E5, E11), or follow Google's example strictly (`__init__` docstring only)? Both Batch D workers recommend accepting either; pydoclint's defaults reject Google's own example.
- **Q6.** Confirm D7: private functions are checked, unlike the legacy linter, Pylint and Ruff.
- **Q7.** Confirm D6: a sectioned docstring is checked at any size. This is stricter than the legacy linter on short functions and is the main improvement over the external tools.
- **Q8.** Confirm D17: the constructor gate uses sections (as for every function) rather than the class_docstring worker's "multi-line class or `__init__` docstring", accepting the E27 gap.
