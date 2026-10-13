# Docstring optional argument: 03 Design

> [!NOTE]
> **Status: VALIDATED** (lead, 2026-10-12; implemented).
> Inputs: [01_understand.md](01_understand.md), [02_references.md](02_references.md). Shared helper: `src/code_lint/ast/python/docstrings.rs` (lead). Canonical rule file: `src/code_lint/rules/nullable_collection_return.rs`. Execution: [04_execution_log.md](04_execution_log.md).

## 0. Lead decisions (2026-10-12)

These override the rest of this document where they differ.

1. **Placeholder.** The batch uses one prose placeholder, `{phrase}`, which already exists (§6 needs no new row).
2. **Type slot.** A type slot that is exactly `(optional)` is this rule's marker (D7). `(int, optional)` belongs to `annotated-type-in-docstring` and is not read here (E7). Fixing that finding leaves `(optional)`, which this rule then flags: an intended chain, stated in the rule docs.
3. **Q2 accepted (R8).** Omission markers are exempt when the matched parameter has a non-`None` default (`retries: int = 3`): E9 is in v1. It uses the new `PythonParameterInfo::default_node`; class docstrings match `Args:` entries with the `__init__` found by `PythonClassInfo::methods()` (containment, no new class helper). `Attributes:` entries and `**kwargs` keys stay text-only.
4. **Q1 and Q3 are not in v1.** `= None` parameters with no marker and no explanation (Q1) and `Returns:` / `Yields:` wording (Q3) become ROADMAP candidates, added by the lead.

## 1. Verdict and name

**Implement**: a port narrowed to the real antipattern (R1, unexplained optionality) and broadened to the phrasings the legacy linter misses. One rule.

**Name: `unexplained-optional-argument`** (file `unexplained_optional_argument.rs`, const `RULE`). It names the pattern ("an optional argument left unexplained"), reads as "ignore unexplained-optional-argument", has three words and no polarity. `Attributes:` entries are covered too; Google formats them "the same" as `Args` (pyguide §3.8.4), so "argument" is kept for the common case. Rejected: `optional-argument-wording` (names a topic, not a defect), `restated-nullability` (R2), `undocumented-none-meaning` (also good, but misses the omission half: `Optional` on a parameter that has a default).

## 2. Decisions

- **D1. Antipattern = R1.** The rule flags a description that says a value is optional or nullable and does not say what the code does when it is `None` or omitted. R2 (any restatement of nullability) is type information, already the `redundant_type_docstring` worker's ground, and its harm is one redundant word. Consequence: an explained description passes even if it starts with `Optional` (E2, E3).
- **D2. One rule, one template.** Omission markers (`Optional`) and nullability markers (`or None`), in `Args:` or `Attributes:`, fail the split test together: same harm (meaning of the absent value unknown), same fix (state it). The matched words are a `{phrase}` placeholder.
- **D3. Markers.** A description is *marked* when it contains one of:
  - *Omission markers* (case-insensitive):
    - `Optional` as the first word, followed by a non-letter (space, `:`, `.`, `,`, `[`, `)`), including inside a leading `(`/`[` (`(Optional) …`, `[optional] …`);
    - a sentence that is only `Optional.`;
    - a trailing `(optional)`.
  - *Nullability markers* (`None` case-sensitive, optionally in backticks), only at a **clause end** (end of description, or `.` / `;` followed by whitespace or end, after an optional `)`):
    - `or None`;
    - `| None`;
    - `can be None`, `may be None`, `could be None`;
    - `Defaults to None`, `Default: None`, `Default is None`, `default None`, `(default: None)`.
- **D4. Not markers**, by design:
  - `if any`, `if available`, `if applicable`: they say what `None` means (there is none);
  - `Optionally …`: describes a behavior (`sort: Optionally sorts the output.`), kept from the legacy linter;
  - `optional` after the first word, including after an article (`An optional callback`, `Whether the field is optional.`, `The optional dependencies`): `optional` is often domain vocabulary (pip extras, optional fields). Accepted false negative;
  - `or None` followed by more text (`or None to use the default`, `or None if unset`): the continuation explains, kept from the legacy linter;
  - `Defaults to 3.`: the signature default is self-explanatory; only `None` defaults are markers.
- **D5. Scope.**
  - Sections: every header the shared parser maps to `DocstringSectionKind::Args` (`Args`, `Arguments`, `Parameters`, `Params`, `Keyword Args`, `Keyword Arguments`, `Other Parameters`) and `Attributes`. Not `Returns`, `Yields` or `Raises` (Q3).
  - Docstrings: all functions and methods (nested, async, `@override`, `@overload`, properties, stubs) and all classes. `@override` gets no exemption: §3.8.3.1 lets an override skip its docstring, not write a vague one.
  - Not module docstrings: they have no `Args:`, and module `Attributes:` are rare. The legacy linter did not check them either.
- **D6. The trigger is text only; no signature lookup in v1.**
  - `Keyword Args:` entries are `**kwargs` keys with no parameter to look up.
  - `Attributes:` entries have no signature.
  - Unannotated code has no annotation to compare with.
  - Keying on the signature is the subject of Q1 and Q2.
- **D7. Type slot.** The parenthesized slot belongs to `redundant_type_docstring` (its D12: "this rule owns the parenthesized slot, that rule owns the description").
  - Exception: this rule also treats a type slot that is exactly `optional` (case-insensitive) as an omission marker. That fills the gap their Q3 leaves open (`x (optional): …` "is owned by neither today"), and `optional` alone carries no type.
  - Napoleon's `(int, optional)` is not read (E7). Annotated, it is their finding. Unannotated, it is the documented Napoleon form, and flagging it would double-report once they flag the type.
  - Lead: confirm with that worker's Q3.
- **D8. Explanation exemption.** After removing the marker spans, the description counts as explained, and passes, if it contains:
  - `None` as a word (case-sensitive, optionally backticked), as in `If None, waits forever.`, `None disables retries.`, `Defaults to None, which disables retries.` (E2);
  - or an omission phrase (case-insensitive): `if omitted`, `when omitted`, `if not `, `when not `, `unless `, `if unset`, `if absent`, `if missing`, `if empty`, `by default`, `defaults to`, `default:`, `default is`, `otherwise`, `falls back` (E3).
  - Both lists are fixed. No option until a project asks: they are English grammar, not domain vocabulary (rule_design_guide §5 targets domain lists).
- **D9. Location and count.** One diagnostic per entry, at the entry's first line (`PythonDocstring::line_node(entry.line_index)`), naming the first marker found. A marker on a continuation line still points at the entry line, which is where the reader looks for `name:`.
- **D10. Template** (Python only, so no `base` and one variant per field):

  | Field | Text |
  | :--- | :--- |
  | summary | Description of `{name}` says `{phrase}` but not what a `None` or omitted value does. |
  | rationale | Callers learn that the value can be `None` or left out, but not what the code does then, so they read the implementation or guess. |
  | suggestion | Replace `{phrase}` with what a `None` or omitted `{name}` does, such as `If None, reads the default config file.` |

  `{phrase}` is a new placeholder (§6).
- **D11. Classification.**
  - Topic: a new `docstrings` topic (batch-level, lead).
  - `Precision::Heuristic`: a marker stands in for "meaning missing", and the meaning of absence is sometimes obvious (`Optional callback invoked on completion.`).
  - `Consensus::Opinionated`: Napoleon's own example writes `Defaults to None.`
  - `ImpactedQuality::Maintainability`.
- **D12. Options and target.** `RuleOptions::code_rule(())` (enforcement mode `ban` by default, no options). `RuleTarget::All`: the wording standard is the same for test helpers.
- **D13. Doc summary.** "Flags docstring argument and attribute descriptions that call a value optional or nullable without saying what `None` or omission does."
- **D14. Python only.** Rust: ROADMAP suggestion only (§7).

## 3. Exemptions (each gets one mutation-tested `pass` case)

| # | Exemption | `pass` case (description) |
| :-- | :--- | :--- |
| E1 | Nullability marker not at a clause end | `path: A file path, or None to read from stdin.` |
| E2 | Explained by `None` outside the marker | `timeout: Optional timeout in seconds. If None, waits forever.` |
| E3 | Explained by an omission phrase | `retries: Optional retry count; by default the client's setting.` |
| E4 | `None` is case-sensitive | `tags: Tags to keep, or none.` |
| E5 | `optional` only counts as the first word | `strict: Whether a missing field is optional.` |
| E6 | `Optionally` is not a marker | `sort: Optionally sorts the output.` |
| E7 | Napoleon `(type, optional)` slot not read | `retries (int, optional): Number of attempts before giving up.` |
| E8 | `Returns:` / `Yields:` / `Raises:` not read | `Returns:` / `    The row, or None.` |
| E9 | Only if Q2 is accepted: omission marker on a parameter with a non-`None` default | `retries: Optional number of retries.` with `retries: int = 3` |

Regression cases, not exemptions (the shared parser owns the behavior, so a rule-side mutation cannot disable them): the legacy linter's P1 (`Format: Optional prefix.` on a continuation line passes), P2 and P3 (fail cases below).

### 3.1 `fail` cases (sketch)

- leading `Optional timeout.`;
- `Optional: the timeout.`;
- `(Optional) The timeout.`;
- `The timeout. Optional.`;
- `The timeout (optional).`;
- `The parent node or None.`;
- ``The parent node or `None`.``;
- `A float | None.` (the legacy linter's P5);
- `The parent; can be None.`;
- Napoleon's `The second parameter. Defaults to None.`;
- description on the next line (P2);
- marker on a wrapped line (P3);
- `Keyword Args:` entry;
- class docstring `Attributes:` entry;
- class docstring `Args:` entry;
- `x (optional): The count.` (D7);
- `ARGS:` header.

## 4. Algorithm

```text
for docstring in function docstrings + class docstrings:
  for section in docstring.sections() where kind in {Args, Attributes}:
    for entry in section.entries:
      markers = omission_markers(entry.description)
              + nullability_markers(entry.description)
              + [ "(optional)" if entry.type_text eq_ignore_case "optional" ]
      if markers is empty: continue
      rest = entry.description with marker spans removed
      if mentions_none(rest) or has_omission_phrase(rest): continue
      report at docstring.line_node(entry.line_index), name=entry.name, phrase=markers[0]
```

Matching is plain string scanning over the joined description: `regex` is not a dependency (`Cargo.toml`), and the marker and phrase sets are short fixed lists.

## 5. Overlap and hand-offs

| Neighbor | Relation |
| :--- | :--- |
| `redundant_type_docstring` | Disjoint: it owns the type slot, this rule owns the description plus a bare `(optional)` slot (D7). `Optional[int] count.` at the start of a description is flagged here as an omission marker; it is type text in the description, which that rule does not read [U: its `03_design.md` is not written yet]. |
| `redundant_returns_docstring` | Disjoint while Q3 stays "no". |
| pydoclint `check-arg-defaults` | No conflict: it governs `default=` in the type slot only. |
| Ruff D417 | Complementary: D417 requires an entry; this rule judges its wording. |

**Perverse incentive** (rule_design_guide §3): deleting the marker (`Optional timeout.` → `Timeout.`) silences the rule without adding meaning. The suggestion points to the meaning, and `known_problems` says so. The structural fix is Q1.

## 6. Shared changes needed (lead)

- **Docstring helper: none for v1.** `PythonFunctionSignature::docstring()`, `PythonClassInfo::docstring`, `sections()`, `DocstringEntry { name, type_text, description, line_index }` and `line_node` cover the algorithm.
- **Placeholder `{phrase}`**: "A phrase quoted from a docstring", rendered `` `{phrase}` ``. Add a row to naming_and_message_style_guide.md §3 and to the vocabulary test in `tests/registry.rs`. `redundant_returns_docstring` may want the same placeholder.
- **Topic `docstrings`** in `src/rule_declaration/taxonomy.rs` (batch-level).
- **Only if Q1 or Q2 is accepted:**
  - `PythonParameterInfo::default_node: Option<AstNode>` (or `has_none_default()`) in `ast/python/functions.rs`.
  - For class docstrings, a structural link from a class to its `__init__` signature (for example `PythonClassInfo::init_signature()`). The link must come from containment, not from the class name: `rule_test!` doubles each fail case in one module, so two classes share every name.

## 7. ROADMAP suggestions

- **Rust analogue** (not planned): in rustdoc `# Arguments` / `# Parameters` lists, flag "Optional …" on an `Option<T>` parameter that does not say what `None` does. Weak: no standard section, and `Option` parameters are rare in idiomatic public APIs.
- **Unexplained `None` default** (if Q1 is "later"): a trigger without markers, keyed on `= None`.
- **`Returns:` "or None"** (if Q3 is "later").

## 8. Draft `RuleDoc`

- **what_it_does**: Flags an entry in a docstring's `Args:` (or `Keyword Args:`, `Parameters:`, …) or `Attributes:` section whose description says the value is optional (`Optional …`, `Optional.`, `(optional)`) or nullable (`… or None.`, `… | None.`, `Can be None.`, `Defaults to None.`) and does not otherwise mention `None` or what happens when the value is omitted (`If None, …`, `If omitted, …`, `by default …`). `or None` followed by more words (`or None to read from stdin`) and `if any` are not flagged. The Napoleon type slot `(int, optional)` is not read.
- **why_is_this_bad**: The signature already says that a parameter has a default or accepts `None`; the docstring is the only place that can say what that means: wait forever or use a library timeout, the root node or a detached one. Google's guide asks a docstring to give "enough information to write a call to the function without reading the function's code".
- **known_problems**:
  - Removing the marker silences the rule without adding the missing meaning.
  - A marker on a value whose absence is self-explanatory (`Optional callback run on completion.`) is flagged.
  - `optional` after the first word (`An optional callback`) is not.
- **Example** (Python):

  ```python
  def connect(host: str, timeout: float | None = None) -> Connection:
      """Opens a connection to `host`.

      Args:
          host: Server name or address.
          timeout: Optional timeout in seconds.
      """
  ```

  `flagged_span`: `timeout: Optional timeout in seconds.`; fixed line: `timeout: Seconds to wait for the handshake. If None, waits indefinitely.`

## 9. Open questions (need the user)

- **Q1. Also flag a parameter whose default is `None` when its description has no marker and no explanation** (`parent: The parent node.` for `parent: Node | None = None`)?
  - For: closes the perverse incentive and catches the most common form of the gap.
  - Against: much noisier, and needs a signature lookup and the `default_node` helper (§6).
  - **Recommendation: not in v1, ROADMAP candidate.**
- **Q2. Exempt omission markers when the matched parameter has a non-`None` default** (`retries: Optional number of retries.` for `retries: int = 3`)?
  - For: there, the signature's default is the explanation, so the rationale is false.
  - Against: needs `default_node` plus the class-to-`__init__` link (§6), and attributes and `**kwargs` keys stay text-only.
  - **Recommendation: yes**, as exemption E9 (§3). If declined, list it in `known_problems`.
- **Q3. Also read `Returns:` / `Yields:`** (`Returns: The row, or None.`)? Same harm (when is `None` returned?), but a different section with its own sibling worker.
  - **Recommendation: no for v1, ROADMAP candidate.**
