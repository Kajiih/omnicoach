# Phase 6: Review and Audit — Signature & Attribute Collection Type Rules

Three independent read-only reviewers who did not write the code. Each was given the project rules, the design, naming and tag guides, `ROADMAP.md`, and docs 01–05:

| Reviewer | Area | Transcript |
| :--- | :--- | :--- |
| PY | Python collector (`ast/python.rs`, `ast/statements.rs`) | `8315508a-…` |
| RL | The 7 rule modules, registration, snapshot | `cc9d986f-…` |
| TD | `rule_test!` suites, collector unit tests, docs coherence | `a6d42539-…` |

The reviewers had no shell. The orchestrator re-checked every High/Med behavior finding against the code: RL-01, RL-02 and RL-03/PY-4 are confirmed, and `parse_param_parts` predates this feature.

**Status: decided and applied (see §3).** IDs below merge duplicates. Source IDs are in brackets.

---

## 1. Triage

### Behavior

| ID | Sev | Finding | Proposed | Reason |
| :--- | :--- | :--- | :--- | :--- |
| B1 | **High** | Suggestions tell users to take `AbstractSet` "from `collections.abc`", but no such name exists there. Following the text raises `ImportError`. [RL-01] | **Fix** | Write "`Set` from `collections.abc` imported as `AbstractSet`", matching PYI025 and the rationale text. |
| B2 | **High** | `CovariantPositions` recurses into invariant `list`/`set`/`dict` values but not into `Mutable*`. Doc comment on `SINGLE_ARG_COVARIANT_CONTAINERS` is wrong. Tier 2 return/attribute also look at nested `Mutable*` types, but mutation is only tracked on the outer container. As a result `rows: Sequence[MutableMapping[...]]` mutated via `for r in self.rows: r["n"] += 1` is flagged, and `list[MutableSequence[int]] → list[Sequence[int]]` is unsound. [RL-02, PY-3] | **Fix** | Recurse only through truly covariant constructors (drop `list`/`List`/`set`/`Set`/`dict`/`Dict`). Tier 2 return/attribute use `TransparentWrappersOnly`, like Tier 2 parameter. Update test a15. |
| B3 | Med | `specific-collection-parameter` compares the full path with `"Sequence"`, so `typing.Sequence[int]` / `collections.abc.Sequence[int]` are never narrowed to `Collection`. [RL-03, PY-4] | **Fix** | Compare the terminal segment. |
| B4 | Med | `@<f>.register` (singledispatch) implementations are flagged by a `Ban` rule. The annotation is the dispatch key, so changing it changes behavior, and `Ban` gives no way to explain the choice. [PY-1, RL-08] | **Fix** | Exempt decorators whose terminal attribute is `register`. Property setters → Defer (rare, explainable via the getter). |
| B5 | Med | `collect_locally_mutated_return_functions` keeps one file-wide name→callee map: an assignment in one function overwrites another's, so the wrong callees get exempted or flagged. Walrus bindings are ignored. [PY-2, PY-12] | **Fix** | Key bindings by the enclosing function. Treat `named_expression` like `assignment`. Bare-name callee matching across classes (`cfg.get(...)`) → Defer (needs resolution). |
| B6 | Low | `scope_shadows_parameter` skips the whole nested `def`, including defaults and decorators, which are evaluated in the enclosing scope (`def g(x=x): …`). [PY-6] | **Fix** | Skip only the shadowing def's `body`. |
| B7 | Low | Every dunder except `__init__`/`__new__` is exempt, including the user-designed `__call__`. [PY-7] | **Fix** | Carve out `__call__`. |
| B8 | Low | An attribute annotated both at class level and as `self.x: list[int]` in `__init__` gets two diagnostics. [PY-13a] | **Fix** | Dedup by name, keep the first. |
| B9 | Nit | `raise ValueError() from NotImplementedError` counts as a stub body. [PY-21] | **Fix** | Inspect only the `raise` operand. |
| B10 | Med | Several rules can flag the same annotation (`-> list[MutableSequence[int]]`), and fixing one finding can raise the next (mutable → concrete → specific). [RL-05] | **Fix (docs)** | After B2, the remaining overlap is intended. State it in each rule's `why_is_this_bad`. |
| B11 | Low | `mutable-collection-parameter` flags unused parameters, while `specific` exempts them. [RL-13] | **Reject** | Different claims: "not mutated" holds for an unused parameter, but "only iterated" cannot be inferred from it. |
| B12 | Med | One explanation comment above `def` silences every finding in that header, across rules. [RL-07] | **Defer** | Framework-wide `RequireExplanation` behavior, not specific to these rules. ROADMAP entry. |
| B13 | Low | Known heuristic gaps: aliases (`import typing as t`, `List as L`), string annotations, a local `class Set` counted as std, `defaultdict`/`deque`/`Counter`/`OrderedDict`, ABC/Protocol exempting concrete helper methods, private classes / `TypedDict` attributes, rebuilding the same file-wide data in each rule. [PY-8, PY-9, PY-10, PY-16, PY-19, RL-18, RL-OOS-3/4, TD-OOS-3] | **Defer** | Each needs import resolution or a scope decision. ROADMAP entries, plus a "Known limitations" line in the rule docs. |
| B14 | Low | Read-only uses (`f"{x}"`, `if_clause`, `reversed`) are treated as escapes, and receivers are detected by name only. [PY-11, PY-18] | **Reject** | Conservative by design (misses findings, never false positives), or rare. |

### Rule text and metadata

| ID | Sev | Finding | Proposed | Reason |
| :--- | :--- | :--- | :--- | :--- |
| M1 | Med | `specific` reuses `{class}` for the suggested ABC, which the style guide forbids. Suggestions list 3–6 alternatives where the guide asks for one canonical replacement. [RL-04, RL-09] | **Fix** | Add a `{replacement}` placeholder (registry and guide), computed from the token: `list`→`Sequence`, `dict`→`Mapping`, `set`/`Set`→`AbstractSet`, `MutableX`→`X`, plus the `Collection`/`Iterable` choice in `specific`. |
| M2 | Low | Summary says "annotated with concrete collection type `Mapping[str, list[int]]`", but the outer type isn't concrete. Multiple tokens render as a single `` `list, set` `` code span. [RL-10] | **Fix** | "annotation `{expression}` contains concrete collection type `{token}`", one diagnostic per token. |
| M3 | Low | Summaries say "without an explanation", which is false under `ban`. Docs don't say where the comment must go. [RL-11] | **Fix** | Follow the `suppressed_exception` wording. |
| M4 | Low | `concrete-collection-return` is `Heuristic` but uses the same matcher as the `Exact` parameter rule. [RL-12, TD-OOS-4] | **Fix** | → `Exact`. The `Set` import check is deterministic, so the parameter rule stays `Exact`. |
| M5 | Nit | Inaccurate rationale wording: "sets, dictionary views" for Collection→Iterable; "immutable collections"; "synthesized constructors" for `__init__` attributes. [RL-15, RL-16] | **Fix** | Reword. |
| M6 | Nit | The `mutable-*` names don't cover `list`; "specific" in the name vs "narrower" in messages. [RL-20] | **Reject** | Renaming costs more than it gives. |

### Tests

| ID | Sev | Finding | Proposed | Reason |
| :--- | :--- | :--- | :--- | :--- |
| T1 | **High** | `rule_test!` checks only spans, so nothing checks the suggested class: removing the `len`/`in`, truthiness or multi-pass detectors would not fail any test. No unit tests exist for `analyze_parameter_collection_capability`, `is_parameter_mutated_or_escaping`, `collect_locally_mutated_return_functions`, `collect_public_class_attributes`, `collect_{mutable,specific}_collection_types`. [TD-1, TD-2, RL-06, PY-22] | **Fix** | `#[rstest]` unit tests that check the returned enum or paths, one per detector, plus repros for B3–B9. |
| T2 | Med | Vacuous or bundled cases: Tier 3 stub/Protocol functions never use the parameter, so they pass through the Unused path. `f3_f4_f5_*` mixes 4 detectors in one case. In E11, `helper(items)` and `.extend(items)` mask each other. Exemption pass cases are bundled. [TD-6, TD-7, TD-8, TD-12, RL-17] | **Fix** | One behavior per case. Each exempt function uses the parameter once. Run a mutation check (disable each exemption → its case fails). |
| T3 | Med | Missing named cases: `NotRequired`/`ReadOnly`, more covariant ABCs, `Generator`/`Coroutine` arg positions, `Callable[..., list]`, qualified `subscript` forms (`typing.Sequence[int]`), aliased `Set as S`, `__new__`, Tier 2/3 `ABC`/`@overload`/`@abstractmethod`/dunder exemptions, `del self.a[k]`, `cls.a`, `fn()[k] = v`, `yield`, lambda shadowing. [TD-9, TD-10, TD-11, TD-14] | **Fix** | 03 promises one named case per matrix row. |
| T4 | Low | F9 (`match` list pattern) may be vacuous. [TD-15] | **Fix (verify)** | Covered by the T2 mutation check. |
| T5 | Low | Matrix-prefixed names (`e5_`, `f3_f4_f5_`) plus `// E5:` comments in 2 files, while the rest of the repo uses descriptive names. `_passes` suffix. [TD-13] | **Fix** | Descriptive names, drop the comments. |

### Code quality

| ID | Sev | Finding | Proposed | Reason |
| :--- | :--- | :--- | :--- | :--- |
| Q1 | Low | `statements.rs:58` reads the Python field `definition` in a language-agnostic module, without checking the node kind. No test. [PY-14] | **Fix** | Gate on `decorated_definition` through the Python vocabulary. Add a unit test. |
| Q2 | Low | `is_exempt_dunder_method` / `has_exempt_signature_decorator` duplicate the private copies in `identical_positional_types.rs`. [RL-14] | **Fix** | Migrate that rule to the shared helpers. Merging all 7 `check_file` bodies → **Reject** (premature). |
| Q3 | Nit | In new code: `builtins.Set` doesn't exist; repeated `std::collections::` paths; `remove` used where `get` would do; redundant `is_comment_kind` after `!is_extra()`. [PY-20, RL-OOS-3] | **Fix** | Readability. A shared `named_children()` helper → Reject (pre-existing repo-wide pattern). |
| Q4 | Med | Two diverging use-site classifiers: `is_safe_readonly_parameter_reference` and `record_reference_capability`. [PY-5] | **Defer** | A unified `classify_use` is a real refactor. T1 tests make it safe later. ROADMAP entry. |
| Q5 | Low | Body walkers are unbounded recursion and run once per parameter. [PY-15] | **Defer** | Matches existing collectors. Note it in ROADMAP. |

### Docs

| ID | Sev | Finding | Proposed | Reason |
| :--- | :--- | :--- | :--- | :--- |
| D1 | **High** | 03 CUJ2/C6 and CUJ3/D3/D5 say the concrete return/attribute rules pass when the value is mutated. The code and §3.1 say "always flag". 04 "Deviations" doesn't record this. [TD-3, TD-4, TD-20, RL-19] | **Fix** | Move C6/D5 to Tier 2b/2c in 03. Record the deviation in 04. |
| D2 | **High** | 05 claims "one behavior per case, 100% Matrix A–F coverage". [TD-5] | **Fix** | Correct it now. Re-assert only after T1–T3. |
| D3 | Low | Stale line anchors (`registry.rs#L325…`, `python.rs#L571…`). [TD-16] | **Fix** | Drop the line anchors. |
| D4 | Low | The lists of iterable builtins disagree between 03 F2, 03 §4.3.3 and the rule doc. [TD-17] | **Fix** | Align them with the code. |
| D5 | Low | Stale status banners in 03 and 04. `@fixture` exemption is overstated for `SourceOnly` rules. [TD-18, TD-OOS-2] | **Fix** | Update. |
| D6 | Nit | 03 lists `Set` as covariant without saying it means `collections.abc.Set`. [TD-19] | **Fix** | Write `AbstractSet`. |

## 2. Out-of-Scope Defects (pre-existing; ROADMAP)

| ID | Finding |
| :--- | :--- |
| OOS-1 | `parse_param_parts` (pre-existing): four near-copies, dead `field("name")`, unreachable `dfs()` fallback. [PY-17] |
| OOS-2 | `is_nested_function_raw` treats methods of a class defined inside a function as nested functions. |
| OOS-3 | `is_docstring_raw` accepts any string expression-statement, not only the first one in a body. |
| OOS-4 | Safe-builtin checks match by name, so shadowed `len`/`list` are trusted. |

## 3. Outcome

The user asked to fix every finding that is RICR and not speculative complexity, and to move the rest to `ROADMAP.md` or drop them. Every correctness fix started with a case that failed before the fix.

### 3.1 Correctness fixes (failing case first)

| ID | Failing case (before the fix) | Fix |
| :--- | :--- | :--- |
| B2 | `a19_invariant_concrete_outer_not_recursed` (unit); `nested_mutable_elements_not_checked` (mutable-attribute); `nested_mutable_type_not_checked` (mutable-return) | Covariant recursion drops `list`/`List`/`set` and the `dict`/`Dict` value position. `collect_mutable_collection_types` is top-level only for all of Tier 2. `AnnotationTraversalDepth` is private. |
| B3 | `qualified_typing_sequence_suggests_collection` | Compare the terminal path segment. |
| B4 | `singledispatch_register_exempt` | `register` added to the exempt decorators. |
| B5 | `binding_in_another_function_does_not_exempt_callee`, `locally_mutated_via_walrus_binding` | Bindings keyed by enclosing function; walrus bindings tracked. Bare-name callee matching stays as `known_gap_same_named_callee_exempts_function`. |
| B6 | `shadowing_nested_def_default_captures_parameter` | A shadowing nested `def`/`lambda` still has its parameters (defaults) analyzed in the enclosing scope. |
| B7 | `call_dunder_is_checked` | `__call__` carved out of the dunder exemption. |
| B9 | `raise_from_not_implemented_cause` (unit) | Inspect only the `raise` operand. |

### 3.2 Other fixes

- **B1, M1**: new `{replacement}` placeholder (registry and style guide), filled by `read_only_collection_replacements` with one fully qualified name (`collections.abc.Sequence` / `Mapping` / `Set`). In `specific-collection-parameter` it is `collections.abc.Collection` / `Iterable`.
- **M2, M3, M5**: all 7 templates and docs reworded. Changed from the proposal: M2 keeps one diagnostic per annotation, listing all tokens. One diagnostic per token would put two diagnostics on the same span, which contradicts 03 §3.5 and the `rule_test!` contract.
- **M4**: `concrete-collection-return` is now `Exact` (CLI snapshot updated).
- **B10 (docs)**: B2 removed the main overlap (`list[MutableSequence[int]]`). The remaining intended cascade (mutable → specific) is stated in the `mutable-collection-parameter` doc.
- **Q1**: `decorated_definition()` added to the Python vocabulary (Rust stub returns `None`). `header_line_range` dispatches through it. New unit case `python_decorated_function`.
- **Q2**: `PythonFunctionSignature::has_imposed_signature()` is shared with `identical-positional-types`, whose private copies were removed. **Behavior change for that rule**: `__call__` is now checked and `@<function>.register` is exempt (cases `call_dunder_flagged`, `decorator_singledispatch_register_exempt`).
- **Q3**: removed `builtins.Set` and the redundant `is_comment_kind`; `std::collections` imports.
- **T1, T3**: `python.rs` unit tests for `analyze_parameter_collection_capability` (one case per detector), `is_parameter_mutated_or_escaping`, `collect_locally_mutated_return_functions`, `collect_public_class_attributes`, `collect_{mutable,specific}_collection_types`, `read_only_collection_replacements`, plus Matrix A cases a20–a23. New rule cases: `new_constructor_is_checked`, `aliased_collections_abc_set_import_still_flags_typing_set`, and known gaps `known_gap_string_annotation_not_parsed`, `known_gap_typing_module_alias_not_resolved`.
- **T2, T4**: bundled exemption cases split into one exemption per case, and the Tier 3 exempt functions use the parameter. Mutation check: each exemption was disabled in turn (each decorator, dunder, `Protocol`/`ABC` for functions and attributes, stub), and each one fails at least one named case (1–7 cases each). The shared `PythonFunctionSignature` methods are not re-tested for every exemption in every rule. The check found two `abstractmethod_exempt` cases masked by an `abc.ABC` base, now fixed. T4 (F9): the `match` detector now also has a unit case (`pattern_matching`), but it was not mutation-checked on its own.
- **T5**: `e*_`/`f*_` prefixes, `// E5:` comments, and the `_passes` suffix removed.
- **D1–D6**: 03 corrected inline (CUJ2/CUJ3, C6/D5 → **FLAG**, B2 traversal, precision, `{replacement}`, F2 builtins, `AbstractSet` wording, fixture note). Deviation recorded in 04. Overclaim corrected in 05. Line anchors dropped in 02/03. Status banners updated.
- **Wording follow-up (user review)**: templates and docs no longer state the default mode or offer "add a comment", which is false in `ban` mode; docs say what excuses a finding *in* `require-explanation` mode (superseded: placement is now documented once in `EnforcementMode::DOC`, see `docs/dev/explanation_hint/plan.md`). Suggestions frame the annotation as a deliberate contract and are conditional on intent ("if `{function}` is not meant to mutate", "unless … a deliberate part of the contract"). Heuristic parts are marked: Tier 1 `{replacement}` is the read-only counterpart offered as a starting point (usage is not inspected); Tier 2/3 summaries say "appears to"; return and attribute docs state that only same-file callers or class methods are checked.

### 3.3 Reversed or adjusted verdicts

- **B8 → Reject**: duplicate class-level plus `__init__` annotations are rare, and type checkers already flag the redeclaration.
- **B11, B14, M6**, the shared `named_children()` helper, and merging the 7 `check_file` bodies: rejected as proposed.

### 3.4 Deferred to `ROADMAP.md`

- B12 (header-wide explanation scope).
- B13 (alias and string annotation resolution, extra concrete containers, `Protocol`/`ABC` helper scope, private classes / `TypedDict`, property setters).
- B5 remainder (callee resolution).
- Q4 (unified use-site classifier).
- Q5 (body walk cost).
- Mode-aware explanation hint generated by the framework (also covers `suppressed-exception`): since implemented, see `docs/dev/explanation_hint/plan.md`.
- OOS-1 to OOS-4 (pre-existing collector defects, under "Architecture & Conformance").

### 3.5 Deferred items follow-up

Each fix started with a case that failed on the previous code.

- **Property setters**: `@<property>.setter` is an imposed-signature decorator, because the value type mirrors the getter's return type, which the return rules check (case `property_setter_exempt`).
- **`TypedDict`**: exempt from `mutable-collection-attribute` only, since a `TypedDict` has no methods that could mutate its keys (case `typed_dict_keys_exempt`). `concrete-collection-attribute` still flags it (case `typed_dict_concrete_key_still_flagged`).
- **Extra concrete containers**: `defaultdict`, `deque`, `Counter`, and `OrderedDict` (bare, `collections.`, or the `typing` aliases) are concrete. `deque` maps to `Sequence`; the others map to `Mapping`.
- **OOS `is_nested_function_raw`**: methods of a function-local class are no longer nested functions. A `def` inside such a method still is. The local class itself is the `nested-class` candidate in `ROADMAP.md`.
- **OOS `parse_param_parts`**: the dead `field("name")` lookup was removed (no behavior change).
- **Dropped**:
  - `Protocol`/`ABC` concrete-helper scope: overriders inherit the helper's signature (LSP), so the exemption is correct.
  - Private classes: they stay checked.
  - Dedicated `unaliased-collections-abc-set-import` rule: Ruff `PYI025` covers it.
  - OOS `is_docstring_raw`: it also matches PEP 257 attribute docstrings.
  - OOS shadowed builtins: rare.
- **Merged**: import alias resolution in annotations, into "Import-Aware Qualified Call Resolution".
