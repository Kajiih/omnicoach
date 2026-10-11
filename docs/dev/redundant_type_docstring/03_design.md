# Repeated docstring type: 03 Design

> [!NOTE]
> **Status: PENDING (lead validation).**
> Designed against `src/code_lint/ast/python/docstrings.rs` as landed (`PythonFunctionSignature::docstring`, `PythonClassInfo::docstring`, `PythonDocstring::sections` / `line_node`, `DocstringSection`, `DocstringEntry`). Canonical conventions: `src/code_lint/rules/nullable_collection_return.rs`.

## 1. Verdict and name

- **Verdict: implement, one rule, wider than the legacy linter** (user decision a): Args entries of functions and classes, `Attributes` entries of classes, and the `type:` prefix of `Returns` / `Yields` when it equals the annotation. Narrower than pydoclint DOC111 on unannotated parameters (never flagged).
- **Name (proposed): `repeated-docstring-type`.** It names the pattern (a type stated again in the docstring), not the verdict ("redundant") or the fix, and follows `repeated-literal` / `repeated-index-access`. Alternatives in Q1.
- File `src/code_lint/rules/repeated_docstring_type.rs`, `pub const RULE`.

## 2. What is flagged

| Docstring | Section | Flagged when | Compared with |
| :--- | :--- | :--- | :--- |
| Function or method | `Args` kind (`Args`, `Arguments`, `Parameters`, `Params`, `Keyword Args`, `Keyword Arguments`, `Other Parameters`) | entry has a parenthesized type (D7) and names an annotated parameter | parameters of that function, `*` / `**` stripped (D6) |
| Class | `Args` kind | same | parameters of the class's own `__init__`; if the class body defines no `__init__`, its annotated class-body attributes (dataclass, `NamedTuple`, attrs fields) (D9) |
| Class | `Attributes` | entry has a parenthesized type and names an annotated attribute | class-body `name: T` and `self.name: T` in `__init__` (D9) |
| Function or method | `Returns` | the text before the first `:` of the first body line equals the return annotation (D4) | return annotation |
| Function or method | `Yields` | same, against the yielded type or the whole annotation (D4) | first type argument of an `Iterator` / `Iterable` / `Generator` / `AsyncIterator` / `AsyncIterable` / `AsyncGenerator` annotation, or the whole annotation |

Not inspected (D5): `Raises` and `Other` sections, `Attributes` sections in function docstrings, module docstrings.

## 3. Algorithm

1. For each `PythonFunctionSignature` with a `docstring()`:
   - Build `annotated: name -> ()` from `parameters` whose `type_node` is `Some`.
   - For each `Args`-kind section and each entry with `type_text = Some(text)`: skip if `text` has no type (D7); look up `entry.name.trim_start_matches('*')`; report on `docstring.line_node(entry.line_index)`.
   - If `return_type_node` is `Some`: for the first `Returns` section and the first `Yields` section, take `body[0].text`, split at the first `:`, compare the trimmed prefix with the annotation text (whitespace removed on both sides, D4); report on `line_node(body[0].index)`.
2. For each `PythonClassInfo` with a `docstring`: same entry loop for `Args` (against `__init__` parameters, or annotated attributes when no `__init__`) and `Attributes` (against annotated attributes). Needs the helper change H1 (§11).

`type_text` has no type (D7) when every top-level comma-separated token of it, case-insensitively, is `optional` or starts with `default`.

## 4. Message template

```rust
const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Docstring entry `{name}` states type `{expression}`, which the code already annotates.",
    rationale: "Type checkers verify the annotation but not the docstring, so the docstring type drifts from it unnoticed and readers cannot tell which one is right.",
    suggestion: "Remove the type from the docstring entry and keep only what the annotation does not say, such as units, valid ranges or what `None` means.",
};
```

- `{name}`: the entry name as written (`*args`, `count`); for `Returns` / `Yields`, the header as written (`Returns`) (D8).
- `{expression}`: the parenthetical as written, with its parentheses (`(int, optional)`), or the prefix (`bool`).
- One sentence each; `Remove` is a listed verb; no new placeholder. The summary holds for a mismatching type too (D2).

## 5. Declaration

| Field | Value | Why |
| :--- | :--- | :--- |
| `languages` | `[Language::Python]` | D11 |
| `options` | `RuleOptions::code_rule` default (`Ban`), no list or threshold | D13 |
| `classification.topics` | `[Topic::DOCSTRINGS]` (new, owned by the lead, shared by the batch) | D10, Q4 |
| `precision` | `Exact` | D10 |
| `consensus` | `Opinionated`: pydoclint's and Pylint's defaults ask for docstring types, and Napoleon-rendered docs show only docstring types unless Sphinx renders annotations | tag guide §2.3 |
| `impacted_quality` | `Maintainability`: a stale type misleads readers; no runtime mechanism | tag guide §2.4 |
| `target` | `RuleTarget::All` | Test helpers with typed docstrings repeat types the same way. |

## 6. `RuleDoc` draft

- `summary`: "Flags docstring entries that state a type the code already annotates."
- `what_it_does`: Flags a Google-style `Args` or `Attributes` entry written `name (type): …` when the parameter or attribute it names has a type annotation, and a `Returns:` or `Yields:` section whose first line starts with the annotated type itself (`bool: True if …` under `-> bool`). Class docstrings are checked too: `Args` entries against `__init__`, `Attributes` entries against class-body and `__init__` annotations. Entries for unannotated parameters or attributes are not flagged, and neither is `(optional)` alone.
- `why_is_this_bad`: Only the annotation is checked by type checkers; the docstring copy goes stale when the annotation changes, and a reader who sees `count (int)` above `count: float` cannot tell which is right. Google style asks for a docstring type only where the code has no annotation, and for `Returns` only for type information "that the type annotation does not provide". Use the entry for what the annotation cannot say: units, valid ranges, what `None` means.
- `known_problems`: A `Returns` / `Yields` prefix is flagged only when it is textually the annotation, so a stale prefix (`int:` under `-> float`) or a spelled-out one (`list of str:` under `-> list[str]`) is missed. Projects whose rendered Sphinx docs rely on docstring types lose them unless Sphinx renders annotations (`autodoc_typehints = "description"`).
- `references`: Google Python Style Guide §3.8.3 (`https://google.github.io/styleguide/pyguide.html#383-functions-and-methods`); pydoclint DOC111 (`https://jsh9.github.io/pydoclint/violation_codes.html`).
- `examples` (Python):

  ```python
  def fetch(url: str, attempts: int) -> bytes:
      """Fetches `url`, retrying on failure.

      Args:
          url: The address to fetch.
          attempts (int): Maximum number of requests.
      """
  ```

  `flagged_span`: `attempts (int): Maximum number of requests.`; `fixed`: the same with `attempts: Maximum number of requests.`

## 7. Decisions

- **D1. One rule for `Args`, `Attributes`, `Returns` and `Yields`.** Same reason (an unchecked copy of an annotation) and same fix (delete the copy), so the split test of `rule_design_guide.md` §1 keeps them together; Google §3.8.4 makes `Attributes` follow `Args` formatting.
- **D2. A type that disagrees with the annotation is the same finding, without comparing types.** The fix is the same deletion, and the annotation is the copy a type checker verifies. Comparing `Sequence[str]` with `sequence of str` or `Optional[int]` with `int | None` would make the rule heuristic. The summary shows `{expression}` so a reader notices a disagreement.
- **D3. Unannotated parameters and attributes are never flagged.** Google requires the docstring type there (improves on DOC111, `02_references.md` G1).
- **D4. `Returns` / `Yields` only when the prefix equals the annotation**, compared with all whitespace removed. `X: …` is ambiguous with prose (`Note: …`, `Count: …`), so equality is what keeps the rule exact; mismatches are a documented false negative. For `Yields`, the yielded type is the first type argument of an iterator or generator annotation (helper H2); the whole annotation also matches.
- **D5. `Raises`, `Other` sections, `Attributes` in function docstrings and module docstrings are out of scope.** A `Raises` entry name is itself the exception type; the others have no annotation to compare with.
- **D6. Entries match parameters by name with `*` / `**` stripped**, so `args (int)` and `*args (int)` both match `*args: int` (as the legacy linter). A signature cannot have two parameters with the same bare name, so the match is unambiguous. The receiver is matched like any parameter.
- **D7. A parenthetical has a type unless all its top-level comma-separated tokens are `optional` or start with `default`** (case-insensitive). `(int, optional)` and `(int, default=3)` are flagged whole: the default is in the signature too. `(optional)` and `(default: 3)` alone are not (E3, Q3).
- **D8. Diagnostic position: the whole entry line** (`PythonDocstring::line_node`), falling back to the docstring when lines cannot be mapped. One finding per entry. A tighter span on the parenthetical is helper request H3 (optional).
- **D9. Class `Args` entries are matched against `__init__` parameters only; against annotated class-body attributes only when the class defines no `__init__`.** An annotated attribute does not make an unannotated `__init__` parameter's type explicit (E7). The fallback covers dataclass-like classes, whose generated `__init__` parameters are exactly their annotated fields.
- **D10. Classification: topic `docstrings` only, `Exact`.** The rule inspects docstrings; annotations are only the reference. It is exact under the Google / Napoleon grammar, where the parenthesis after the name is the type slot: a non-type word there other than `optional` / `default` (`(deprecated)`) is outside that grammar.
- **D11. Python only.** Rustdoc has no typed-parameter syntax; see §10 for the ROADMAP suggestion.
- **D12. Ownership with `docstring_optional_arg`: this rule owns the parenthetical, that rule owns the description.** `x (int, optional): Optional. The x.` gets one finding from each, on the same line, with independent fixes that do not undo each other. No shared helper beyond `docstrings.rs`.
- **D13. No options; default enforcement mode `Ban`.** A project that keeps docstring types for rendered docs disables the rule; there is no per-site reason to keep a repeated type.

## 8. Exemptions (one mutation-tested `pass` case each)

| ID | Exemption | `pass` case (sketch) |
| :-- | :--- | :--- |
| E1 | Parameter without annotation | `def scale(factor):` with `factor (float): …` |
| E2 | Entry that names no parameter (`Keyword Args` keys of `**kwargs`, stale names) | `def connect(**options: object):` with `Keyword Args:` / `timeout (float): …` |
| E3 | Parenthetical without a type (`optional`, `default …`) | `def scale(factor: float = 1.0):` with `factor (optional): …` |
| E4 | Attribute without annotation | class body `retries = 3`, `Attributes:` / `retries (int): …` |
| E5 | `Returns` / `Yields` prefix that differs from the annotation | `-> Mapping[str, int]` with `Returns:` / `dict: Counts by name.` |
| E6 | `Returns` / `Yields` without a return annotation | `def count(rows):` with `Returns:` / `int: The row count.` |
| E7 | Class `Args` entry whose `__init__` parameter is unannotated, even if the attribute is | `def __init__(self, count): self.count: int = count` with class `Args:` / `count (int): …` |

## 9. Open questions

- **Q1 (name).** `repeated-docstring-type` (proposed), `docstring-type-annotation` (the legacy linter's and pydoclint's wording, but suggests every docstring type is flagged), or `annotated-type-in-docstring`?
- **Q2 (scope).** Ship `Returns` / `Yields` (D4) in the first version, or start with `Args` + `Attributes` and add them later? They are the most common Napoleon form but need helper H2.
- **Q3 (`(optional)` alone).** Owned by nobody today (E3). It restates the signature default rather than a type. Leave it, give it to `docstring_optional_arg`, or widen this rule to "signature information in a docstring entry" (which would also condition on defaults, not annotations)?
- **Q4 (topic).** New `docstrings` topic for the batch (lead, `taxonomy.rs`); should this rule also carry `static-typing`?

## 10. ROADMAP suggestion (Rust)

> *Rust analogue of `repeated-docstring-type`*: a rustdoc `# Arguments` list that restates a parameter type (``* `count` - A `usize` giving …``). Rustdoc has no typed-parameter syntax, so the pattern is prose and rare; low priority, no known Clippy lint [U].

## 11. Helper API changes needed

| ID | Change | Why |
| :-- | :--- | :--- |
| H1 | A class's own `__init__` parameters and annotated attribute names, keyed by class identity rather than name: for example `PythonClassInfo::init_parameters() -> Option<Vec<PythonParameterInfo>>` and `PythonClassInfo::annotated_attribute_names() -> Vec<String>` (class-body `AnnAssign` targets plus `self.x: T` in `__init__`). | `extract_function_signatures` does not say which class a method belongs to, and `PythonAnnotatedAttribute` carries only `class_name`, which collides for same-name or nested classes. |
| H2 | `generator_yield_type(return_type_node: &AstNode) -> Option<AstNode>` in `ast/python/annotations.rs`: the first type argument when the annotation's base resolves to `Iterator`, `Iterable`, `Generator` or their `Async` forms (`typing` or `collections.abc`). | D4 for `Yields`; rules cannot read ruff nodes. Only if Q2 keeps `Yields`. |
| H3 (optional) | `DocstringEntry` byte range of the parenthetical within its line, or `PythonDocstring::line_node` taking a sub-range. | D8: point at `(int)` instead of the whole entry line. Not needed for v1. |

No change is needed to `parse_google_sections`: `type_text` already balances nested parentheses and keeps `, optional`, and continuation lines are never entries (fixes the legacy linter P1, P2, P3).

## 12. Test plan (Phase 4 sketch)

`fail` cases: function `Args` entry; `*args (int)` and `**kwargs (str)`; nested-parenthesis type; `(int, optional)`; `__init__` docstring `Args`; class docstring `Args` against `__init__`; dataclass `Args` (no `__init__`); `Attributes` against class body and against `self.x: T`; `Returns` prefix equal to the annotation; `Yields` prefix equal to the item type; a disagreeing parenthetical (`(str)` under `int`). `pass` cases: E1–E7, plus an untyped entry under an annotation, and a parenthetical inside a description continuation line. The duplicate-module harness (`{code}\n{code}`) redefines the classes; with H1 keyed by identity it needs no special handling.
