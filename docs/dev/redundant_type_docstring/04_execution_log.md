# Repeated docstring type: 04 Execution log

Rule: `annotated-type-in-docstring` (`src/code_lint/rules/annotated_type_in_docstring.rs`), built by worker A of Batch D on 2026-10-12.

## What was built

- **Rule.** Python only, `RuleTarget::All`, `RuleOptions::code_rule(())` (default `ban`), topic `docstrings`, `Exact` / `Opinionated` / `Maintainability`. One finding per entry or section, on the docstring line (`PythonDocstring::line_node`).
  - Function docstrings: `Args`-kind entries against annotated parameters (`DocstringEntry::bare_name`), `Returns:` against the return annotation, `Yields:` against the yielded item type or the whole annotation.
  - Class docstrings: `Args`-kind entries against the annotated parameters of `PythonClassInfo::constructor()`, or against annotated class-body fields when there is no `__init__`; `Attributes:` entries against annotated `PythonClassInfo::attribute_definitions()`.
- **Helper** (lead-approved, owned by worker A): `generator_yield_type` in `src/code_lint/ast/python/annotations.rs` (H2). It uses the class helpers of [class_docstring](../class_docstring/04_execution_log.md).

## Deviations from 03

| Change | Reason |
| :--- | :--- |
| Name `annotated-type-in-docstring` instead of `repeated-docstring-type`. | User decision (Q1). |
| Template `` `{section}:` section states type `{phrase}`, which the code already annotates. `` instead of `{name}` / `{expression}`. | `{name}` would hold a section header for `Returns:` / `Yields:`. Lead decision: one template, existing placeholders, the entry name is on the flagged line. |
| D7 narrowed: a parenthetical has no type only when it is exactly `optional` or starts with the word `default`. `(int, optional)` and `(optional, default 3)` are flagged; `(defaultdict[str, int])` is a type. | User decision: `(optional)` / `(default …)` belong to `unexplained-optional-argument`. |
| `Attributes:` entries are compared with each attribute's first definition (class body, `__init__` and the post-init hooks of D8), not just class-body and `__init__` annotations. | Reuses `attribute_definitions()`. A later annotation of an attribute first assigned without one is not seen (false negative). |
| H1 replaced by `PythonClassInfo::constructor()` and `attribute_definitions()`. | Class-identity helpers owned by the same worker. H3 (tighter span) not built. |

## Tests and mutations

- Rule: 9 `pass` cases (one positive layout, one per E1–E7, one continuation line the parser never reads as an entry) and 14 `fail` cases.
- Helper: 6 `generator_yield_type` cases in `annotations.rs`.
- `scripts/exemptions/docstrings.py`: 8 mutations (E3 has two, one for `(optional)` and one for `(default …)`); the positive layout and the continuation-line case are listed as unmutated. All 8 killed, checked by applying each edit and running the cases (`scripts/exemption_mutations.py` could not run in the worker's sandbox; the lead should re-run it).
