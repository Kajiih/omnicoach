# AST policy extraction: 01 Understand

> [!NOTE]
> **Status: VALIDATED (2026-10-05). Executing §6; step 1 landed. Q2 open (blocks step 4).**
> Scope: the ROADMAP item "Rule-shaped items left in `ast/python`", plus the same leak found in `ast/rust.rs` and `ast.rs`.
> Line numbers are from the inventory at change `tvvyzump` and will drift.

## 1. Problem

`ast/python` should expose language constructs ("this annotation is a union of these branches", "this is a module-level assignment"). It should not return rule verdicts ("this return type is a nullable collection, suggest `Sequence`"). When it does, the abstraction is shaped by the rules that use it, and changing one rule's policy means editing the shared AST layer.

## 2. Goals and non-goals

- **Goals**
  - Every item in `ast/` is a language fact, testable without naming a rule.
  - Every exemption, vocabulary and suggestion text lives in the rule (or rules) that own it.
  - No behavior change: every `rule_test!` case and the dogfood test pass unchanged.
- **Non-goals**
  - The typed-CST decision (ROADMAP "Typed CST via `type-sitter`").
  - Fixing rule false positives or negatives found along the way (record them instead).
  - Re-enabling `call-before-definition`.

## 3. The ROADMAP item is partly wrong

- It lists items as single-rule policy. Several are shared by 3–7 rules: `is_concrete_collection_constructor` (`annotations.rs:57–69`), `MUTABLE_COLLECTION_ABCS` (`annotations.rs:210`), `read_only_collection_replacements` (6 consumers), and `is_exempt_from_signature_rules` / `is_exempt_from_body_usage_rules` (`functions.rs:86–99`, 7 rules). Moving them "into the rule" is not possible; they need a shared home (Q1).
- It says to do the work with the typed-CST decision. Most items do not depend on it (Q4).
- It misses items found by the sweep (§4, "Additional").

## 4. Inventory

| ID | Item | Location | Consumers | Fact part | Policy part | Size, risk |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| A | Collection annotations | `annotations.rs` | 3–7 rules | unwrap/generic/union helpers, variance tables | the vocabularies; `collect_specific_collection_types` (`["Sequence","Collection"]`, :380, 1 consumer); replacement text and shape match (397–400); `immutable_constant_collection_replacements` (1 consumer); nullable: `READONLY_AND_IMMUTABLE_COLLECTION_CONSTRUCTORS` (213–226), all-branches verdict, variadic tuple check, signature exemption | ~120 lines changed, ~60 moved; medium |
| B | `ForwardCall`, `collect_forward_calls` | `scopes.rs` ~346–790 | `call-before-definition` (disabled) | sibling functions and the calls between them | callee-later check, `can_reach` recursion exemption, constructor exemption, dedup | low (rule disabled) |
| C | `PythonMutableModuleConstant` | `python.rs:308–449` | `mutable-module-constant` | module-level assignments, collection displays, call callee path | dunder skip, constant/`Final` gate, vocabulary, `Mapping` exemption, mutable constructor list, display→type mapping | medium; depends on A |
| D | `PythonInlinePublicAttributeAnnotation` | `classes.rs:416–480` | `inline-public-attribute-annotation` | annotated `self.x: T = ...` in methods | field-synthesizing class skip, bare `Final` skip, `_` filter (shared with `collect_public_class_attributes`) | ~20 lines; low |
| E | Rust nullable collection return | `rust.rs:1493–1600` | `nullable-collection-return` | `Option<T>` return types | collection list, trait exemption, slices; its own skip of inline test ranges may duplicate `runner.rs:57–63` | medium |

**Additional policy found by the sweep:**

- `is_fake_class_name` / `is_contract_base` (`classes.rs:30–100`): `fake-without-protocol` only.
- `LiteralValue::is_trivial` (`ast.rs:310–333`): `repeated-literal` and test utilities.
- Rust `LITERAL_EXEMPT_MACROS` logging macros (`rust.rs:1188–1222`).
- `find_unwrapped_multiline_strings` exemptions (`ast.rs:480`, `python.rs:672`, `rust.rs:712`).
- `collect_environ_subscripts` display text (`python.rs:608–637`).
- `NullableCollectionReturn` dispatcher (`ast.rs:524–543`): disappears once A and E are done.
- `collect_public_class_attributes` filters.
- `analyze_parameter_collection_capability`: borderline.

**Checked and legitimately facts:** mutation and escape analysis, `MUTATING_METHODS`, the logging vocabulary, `format_strings.rs` and `strings.rs`, `statements.rs`, the `semantic/` engines (the minimum word count in `comments.rs` belongs to the contract layer).

## 5. Proposed construct APIs (sketch, for design)

- A: `CollectionKind`, `CollectionShape`, `CollectionConstructor`, `collect_collection_constructors`; `PythonUnion`, `PythonTypeBranch`, `return_type_union`.
- B: `ScopeFunctions`, `ScopeFunction`, `SiblingCall`.
- C: `PythonModuleAssignment`, `collect_module_level_assignments`, `PythonCollectionDisplay`, `call_callee_path`.
- D: `PythonInstanceAttributeAnnotation`, `collect_instance_attribute_annotations`.

Existing unit tests that pin behavior and must keep passing: `python.rs:2018–2060`, `2132–2187`, `2189–2240`, `2595–2626`; `rust.rs:1876`.

## 6. Proposed order

Each step is one atomic commit, smallest and least coupled first:

1. D.
2. `fake-without-protocol` helpers.
3. B (or defer or delete, Q3).
4. A part 1: collection taxonomy and the shared-helper home (Q1, Q2).
5. C.
6. A part 2 and E, then delete `NullableCollectionReturn`.
7. Optional: the remaining sweep items.

## 7. Decisions and open questions

- **D1.** No behavior change in this work. Bugs found are recorded, not fixed.
- **D2.** The ROADMAP item is corrected once this doc is validated.
- **D3 (was Q1).** Policy shared by several rules goes in a non-rule helper module under `rules/`, with an architecture entry. The main goal is still to keep that module minimal: prefer general construct-level logic in `ast/` that rules share, and keep only true shared policy in the helper module.
- **D4 (was Q3).** Treat `call-before-definition` as enabled: extract the `ForwardCall` policy into the rule like any other (step 3).
- **D5 (was Q4).** The typed-CST work happens later; this work does not wait for it.
- **Q2.** Where does the knowledge "`list` is a mutable concrete collection, `Sequence` is a read-only abstract one, `frozenset` is immutable" live? It is a fact about Python's standard library, not a rule's choice, so the inventory proposes keeping it in `ast/` as a classification (for example `CollectionKind::{ConcreteMutable, AbstractReadOnly, Immutable}` for a type name). Rules would then decide what to flag and which replacement to suggest. The alternative is to move these lists into the rules (or the D3 helper module) as policy.
