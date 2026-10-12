# Trivial override docstring: 04 Execution log

> [!IMPORTANT]
> **Status: VALIDATED** (lead, 2026-10-12; implemented). Phase 4 of 7, worker C. Design: [03_design.md](03_design.md).

## What was built

| Item | File |
| :--- | :--- |
| `deferral-docstring` (`CodeRule<ListOption>`, `banned` phrase list with `{base}` / `{method}` tokens) | `src/code_lint/rules/deferral_docstring.rs` |
| `copied-base-docstring` | `src/code_lint/rules/copied_base_docstring.rs` |
| `partial-override-docstring` | `src/code_lint/rules/partial_override_docstring.rs` |
| `PythonFunctionSignature::has_override_decorator()`: public wrapper of the existing import-resolved check | `src/code_lint/ast/python/functions.rs` |
| `local_base_classes`, `local_ancestors`, `inherited_method_docstring` (D9, D10, D11) | `src/code_lint/ast/python/classes.rs` |
| `{sections}` placeholder | `naming_and_message_style_guide.md` §3, `tests/registry.rs` |
| Mutations and unmutated cases | `scripts/exemptions/docstrings.py` |

All three rules: Python only, default `ban`, topic `docstrings`, `Precision::Exact`, `Consensus::Opinionated`, `ImpactedQuality::Maintainability`, span = the docstring literal, `RuleTarget::All`. Each walks `extract_classes` and `PythonClassInfo::methods()`, so only `@override` methods directly in a class body are inspected (D8, D13). No rule reads another rule's option or constants; Rule 3 recognizes a summary copy (E15) with the shared `normalized()` / `summary_paragraph()` helpers.

## Deviations from 03 and clarifications

- **E5 is not mutated (lead, 2026-10-12).** The design's mutation ("treat an undocumented definition as an empty docstring") cannot be killed: an empty inherited docstring has no section and equals no non-empty override. The E5 `pass` cases (`undocumented_ancestor_exempt`, Rules 2 and 3) are listed under `Unmutated`; the D9 skip itself is protected by an `undocumented_nearer_ancestor_skipped` `fail` case per rule and the mutation "stop at the nearest ancestor defining the method".
- **`local_ancestors` is public (lead, 2026-10-12).** Rule 1's `{base}` needs the enclosing class's in-file ancestors (D6), and `inherited_method_docstring` walks the same order; one helper avoids two walks.
- **`{base}` forms.** A base is matched as written without type arguments (`pkg.Base`) and by its last dotted segment (`Base`); in-file ancestors by name. Backticks and the `:class:` role are stripped from the whole text before matching, as D7 lists.
- **`()` is removed anywhere in the text**, not only at its end, so that `{method}` inside a phrase (``Implements the `_edit_leaf()` interface.``) matches as D6 describes.
- **D7 dedent** comes from `normalized()`'s whitespace collapse, which makes `inspect.cleandoc` dedenting unnecessary for equality.
- **D10 got its own `pass` case and mutation** (`base_bound_to_last_earlier_definition`, `rposition` → `position`): the doubled `fail` cases do not distinguish the first and the last earlier definition when both copies are identical.
- **E6** has one dedicated case per rule, the base defined after the subclass (the only variant a name lookup could wrongly resolve); imported and dotted bases are unresolvable by construction.
- **E14** trusts a summary whose first word is exactly `Return`, `Returns`, `Yield` or `Yields`, as `undocumented-return-value` D13 lists them.
- **E16** exempts a `Raises:` section only when it has entries and all are `NotImplementedError`; a prose-only `Raises:` section is still required.
- **Option kind.** The design's `kind: Banned` is `ListKind::Deny`, whose keys are `banned` / `extend-banned` / `remove-banned`.
- **References.** Google Python Style Guide §3.8.3.1 for all three; PEP 698 for Rule 1; `inspect.getdoc` for Rules 2 and 3.

## Tests

| Rule | `pass` | `fail` | Mutations |
| :--- | ---: | ---: | :--- |
| `deferral-docstring` | 6 | 7 | E7, E8, E9, E10 |
| `copied-base-docstring` | 11 | 6 | E1, E2, E3, E4, E9, E10, D10; shared E6, D9 |
| `partial-override-docstring` | 13 | 5 | E9, E10, E11, E12, E13, E14, E15, E16; shared E6, D9 |

21 mutations in total. Unmutated: 7 positive layouts (no docstring, a difference over a base without contract sections, a complete override docstring) and the 2 E5 cases. Two unit tests in `classes.rs` pin positional base binding and the depth-first ancestor order.

## Open issues

- The ancestor walk is depth-first, not C3: in a diamond, a shared base can be searched before a right-hand sibling that also documents the method (D9 accepts the approximation).
