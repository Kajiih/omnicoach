# Class docstring completeness: 04 Execution log

Rule: `undocumented-public-attribute` (`src/code_lint/rules/undocumented_public_attribute.rs`), built by worker A of Batch D on 2026-10-12.

## What was built

- **Rule.** Python only, `RuleTarget::SourceOnly`, `RuleOptions::code_rule` (default `ban`) with one `allowed` list (default `["model_config"]`), topic `docstrings`, `Heuristic` / `Opinionated` / `Maintainability`. One finding per missing name, on the target of its first definition.
- **Helpers** in `src/code_lint/ast/python/classes.rs`:
  - `PythonClassInfo::attribute_definitions()` and `PythonAttributeDefinition` (`name`, `node`, `annotation`, `is_in_class_body`, `is_init_var`): first definition per name, in source order, per D8.
  - `PythonClassInfo::defines_function(name)` (E7) and `PythonClassInfo::is_enum()` (E8, H3).
  - `PythonClassInfo::constructor()`: the last non-`@overload` `__init__` of the class body. Withdrawn from this design (H1), but on the user's helper list and used by `annotated-type-in-docstring`.

## Deviations from 03

| Change | Reason |
| :--- | :--- |
| Option A of 03b: arguments stored on `self` are attributes; an `Attributes:` section in the `__init__` docstring does not count, and the suggestion names the class docstring. | User decision. |
| New exemption: `dataclasses.InitVar` pseudo-fields (bare or subscripted, resolved through imports). | User decision (03b §5 side finding). |
| No "stored unchanged" flag on `PythonAttributeDefinition`. | It was requested for `undocumented-parameter`, which the user dropped; no consumer left. |
| `PythonAttributeDefinition::is_in_class_body` added. | `annotated-type-in-docstring` compares class `Args:` entries with class-body fields when there is no `__init__` (its D9). |
| The receiver is `self` only, not any first parameter (D8). | `method_receiver_name_ast(…, false, …)` only recognizes `self`; `def __init__(this)` is not read. Not a named exemption. |
| §7's "two attributes → two findings" case became single-attribute cases. | `rule_test!` requires exactly one finding per `fail` case. |
| Extra `fail` cases: `attributes_section_in_init_docstring_not_counted`, `nested_class_checked`. | Pin the user decision and D13. |

A missing class docstring (D3, part of E5) is a structural `continue`, not a mutation-tested exemption; the summary-only half of E5 is.

## Tests and mutations

- Rule: 9 `pass` cases (one positive layout, one per E5–E11, one for `InitVar`) and 8 `fail` cases.
- Helpers: 6 unit tests in `classes.rs` (class-body targets, initializer targets without nested scopes, first definition wins, `constructor`, `is_enum`, `defines_function`).
- `scripts/exemptions/docstrings.py`: 8 mutations (6 in the rule file, 2 in `classes.rs` for E10 and E11); `every_attribute_listed` is listed as unmutated. All 8 killed, checked by applying each edit and running the cases (`scripts/exemption_mutations.py` could not run in the worker's sandbox; the lead should re-run it). The first draft of `nested_scope_assignments_exempt` assigned `other.leak`, which the helper never reads; it now assigns `self.leak` so the mutation kills it.
