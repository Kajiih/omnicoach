# `mutable-module-constant`: plan (light workflow)

> Status: **implemented**.

## 1. Problem

Python has no `const` keyword for deep immutability. Module-level constants (`ALLOWED = ["a", "b"]`, `PORTS: dict[str, int] = {"http": 80}`, `TAGS: Final = {"a", "b"}`) are frequently initialized with or annotated as mutable collections (`list`, `dict`, `set`, `MutableSequence`, `MutableMapping`, `MutableSet`, or mutable `collections` containers).

Even when annotated with `typing.Final` ([PEP 591](https://peps.python.org/pep-0591/)), `Final` only prevents **rebinding the variable name** (`ALLOWED = ...`); it does not prevent mutating the collection in place (`ALLOWED.append("c")`, `PORTS["http"] = 8080`, `TAGS.clear()`), which passes both Mypy and Pyright without error and corrupts shared module state across all importers at runtime.

## 2. Prior art & ecosystem facts

- **`wemake-python-styleguide` `WPS407` (`MutableModuleConstantViolation`)**: flags module-level `UPPER_CASE` constants assigned a `list`/`dict`/`set` literal, comprehension, or call, recommending `tuple`, `frozenset`, and `types.MappingProxyType`. Known friction ([WPS #1624](https://github.com/wemake-services/wemake-python-styleguide/issues/1624)): `WPS407` ignores type annotations, so `PORTS: Mapping[str, int] = {"http": 80}` is flagged even when statically protected as a read-only `Mapping`.
- **Ruff (`RUF012` `mutable-class-default`, `B006` `mutable-argument-default`)**: covers class attributes and function parameter defaults only; Ruff has no rule for module-level constants.
- **PEP 814 (`frozendict`, Python 3.15, accepted Feb 2026)**: adds built-in `frozendict` to `builtins` as a standalone, truly immutable, hashable mapping implementing `collections.abc.Mapping` (constructed via `frozendict({...})` or `frozendict(k=v)`). On Python < 3.15, the standard library alternatives are `types.MappingProxyType({...})` (runtime read-only view) and `collections.abc.Mapping[K, V]` (static read-only type contract).
- **Existing Omni rules (`*-collection-attribute`)**: inspect `ClassDef` bodies and `self.<attr>` in `__init__`, explicitly skipping module-level assignments.
- **Rust**: `const` and `static` enforce deep immutability at the language level (`static mut` requires `unsafe` and is denied by `static_mut_refs`; interior mutability in `const` is covered by `clippy::declare_interior_mutable_const`). Scope is therefore **Python only (`SupportLang::Python`)**.

## 3. Design

| Decision | Choice |
| :--- | :--- |
| **Rule name & file** | `mutable-module-constant` (`src/code_lint/rules/mutable_module_constant.rs`) |
| **Target & default mode** | `SupportLang::Python`, `RuleTarget::SourceOnly`, `RuleOptions::code_rule(())` (`enforcement-mode = "ban"` by default) |
| **Classification** | `topics: &[Topic::GLOBAL_STATE, Topic::STATIC_TYPING]`, `Precision::Heuristic`, `Consensus::Opinionated`, `ImpactedQuality::Reliability` |
| **Target scope (Q1: Option A)** | Module-level `assignment` statements (including inside top-level `if`/`elif`/`else`/`try`/`except`/`finally`/`with` blocks via `CONSTANT_TRANSPARENT_STATEMENTS`) whose LHS is a single identifier in `UPPER_SNAKE_CASE` / `_UPPER_SNAKE_CASE` (`is_constant_name`) **or** that carry a `Final` / `Final[...]` annotation (`has_final_annotation`, unwrapping `Annotated[..., ...]` and parentheses). |
| **Exemptions** | 1. Class-level and function-local assignments.<br>2. Dunder names (`__all__`, including `__all__: Final = [...]`).<br>3. Non-identifier targets (`config.ALLOWED = ...`, `A, B = ...`).<br>4. Type aliases (`type X = list[str]` is `type_alias_statement`; `X: TypeAlias = list[str]` has a `subscript` on the RHS, not a literal or call).<br>5. Unqualified `Set` when `from collections.abc import Set` is present in the file.<br>6. Dict literal, dict comprehension, or `dict(...)` call when annotated with `Mapping` (including `Final[Mapping[K, V]]`). |
| **Detection (Q2: Option 2a)** | 1. **Annotation check**: if `type_node` contains a concrete mutable collection (`is_concrete_collection_constructor`) or an abstract mutable collection (`MUTABLE_COLLECTION_ABCS` in covariant positions), flag `type_node`.<br>2. **Value check** (when `type_node` is absent or not flagged): inspect `right` (unwrapping parentheses and skipping extra comment nodes):<br>  - `list`, `list_comprehension` → `"list"`<br>  - `set`, `set_comprehension` → `"set"`<br>  - `dictionary`, `dictionary_comprehension` → `"dict"` (exempt when `type_node` has a read-only `Mapping` annotation)<br>  - `call` whose callee (unwrapping generic subscript `list[str]()`) satisfies `is_runtime_mutable_collection_constructor` → callee path (`"dict"` also exempt when `type_node` has a read-only `Mapping` annotation; `defaultdict`, `Counter`, `OrderedDict`, `list`, `deque`, `set` are always flagged). |
| **Replacements (Q3)** | `immutable_constant_collection_replacements` maps sequence types to `tuple`, set types to `frozenset`, and mapping types to `frozendict`. `why_is_this_bad` explains `frozendict` (Python 3.15+), `types.MappingProxyType`, and `collections.abc.Mapping` for dictionary constants. Suggestion recommends replacing the `{token}` type or value of `{name}` with `{replacement}` and annotating `{name}` with `Final`, or renaming `{name}` to `_`-prefixed lowercase without `Final` if it is mutable module state. |

## 4. Tests (written first)

- **`src/code_lint/ast/python.rs` (`mod tests`)**:
  - `test_immutable_constant_collection_replacements`: unit tests sequence (`tuple`), mapping (`frozendict`), set (`frozenset`), and mixed deduplicated replacements.
  - `test_collect_mutable_module_constants`: unit tests `collect_mutable_module_constants` directly (`(name, matched_types, target_node.text())`), including `Mapping` exemption vs `defaultdict`, multiline parenthesized expressions with comments, and `Annotated[Final[...], ...]`.
- **`src/code_lint/rules/mutable_module_constant.rs` (`rule_test!`)**:
  - **Fail cases**:
    - `unannotated_list_literal`, `parenthesized_mutable_literal`, `multiline_parenthesized_with_comment`, `unannotated_dict_literal`, `unannotated_set_literal`
    - `list_comprehension_value`, `set_comprehension_value`, `dict_comprehension_value`
    - `mutable_constructor_call`, `subscripted_constructor_call`, `collections_deque_call`
    - `private_upper_snake_case_constant`
    - `bare_final_with_mutable_list`, `bare_final_with_mutable_dict`, `lowercase_final_with_mutable_list`, `annotated_wrapped_final_on_lowercase`
    - `bare_annotation_without_value`, `annotated_concrete_list`, `annotated_final_concrete_dict`, `union_mapping_and_concrete_dict_flagged`
    - `annotated_mutable_sequence`, `nested_mutable_in_covariant_tuple_annotation`
    - `sequence_annotation_with_mutable_list_literal`, `abstract_set_annotation_with_mutable_set_literal`, `mapping_annotation_with_defaultdict_call`, `mapping_annotation_with_mutable_value_type`, `unqualified_set_annotation_without_abc_import`
    - `inside_top_level_if`, `inside_top_level_try_except`
  - **Pass cases**:
    - `immutable_literals_and_calls`
    - `mapping_annotated_dict_literal_exempt`, `mapping_annotated_dict_comprehension_exempt`, `mapping_annotated_dict_call_exempt`
    - `dunder_all_exempt`
    - `lowercase_module_state_not_flagged`
    - `class_and_function_scope_not_flagged` (including `FINAL_ROLES: Final = ["a", "b"]` in class scope)
    - `non_identifier_target_not_flagged`
    - `type_alias_not_flagged`
    - `collections_abc_set_import_exempts_set_annotation`

## 5. Verification & exit criteria

1. Red tests fail before implementation and pass after.
2. Per-exemption mutation check: all 12 exemption/unwrap mutations (`is_module_level`, `CONSTANT_TRANSPARENT_STATEMENTS`, `left.kind() != "identifier"`, dunder `__all__`, `!is_constant_name && !is_final`, `abc_set_imported`, `without_parentheses`, `is_read_only_mapping` on `dictionary`, `dictionary_comprehension`, and `dict()` call, `!child.is_extra()` in `without_parentheses`, and `Annotated` unwrapping in `has_final_annotation`) are killed by the test suite.
3. Full check suite passes: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps --document-private-items`.
4. Two independent pedantic reviewers (`Code and test reviewer`, `User-facing text reviewer`) audit the diff.

## 6. Deviations from initial plan

- **Runtime constructor helper (`is_runtime_mutable_collection_constructor`)**: Instead of reusing `is_concrete_collection_constructor` for RHS `call` expressions, added a narrower `is_runtime_mutable_collection_constructor` matching only runtime types (`list`, `dict`, `set`, `defaultdict`, `deque`, `Counter`, `OrderedDict` in `builtins.` / `collections.`), avoiding deprecated typing aliases (`List()`, `Dict()`, `Set()`) that are not idiomatic runtime constructors.
- **Single-token `{replacement}` in `immutable_constant_collection_replacements`**: To keep `{replacement}` concise and strictly single-type per matched collection (`tuple`, `frozenset`, `frozendict`) without repeating `(or types.MappingProxyType or collections.abc.Mapping for a dict)` on non-dict findings, `immutable_constant_collection_replacements` returns `"frozendict"` for mapping types while `RuleDoc::why_is_this_bad` explains all three dictionary options (`frozendict`, `types.MappingProxyType`, and `collections.abc.Mapping`).

## 7. Review summary

Two independent pedantic reviewers (`Code and test reviewer` and `User-facing text reviewer`) audited the implementation:
- **Code & test fixes applied**:
  1. Fixed `without_parentheses` in `src/code_lint/ast/python.rs` to filter `!child.is_extra()` so multiline parenthesized expressions with leading comments unwrap to the inner expression rather than the `comment` node.
  2. Extended `has_final_annotation` in `src/code_lint/ast/python.rs` to unwrap `Annotated[Final[...], ...]` and parentheses.
  3. Extracted `MUTABLE_COLLECTION_ABCS` and `format_collection_replacements` in `src/code_lint/ast/python.rs` to eliminate duplication.
  4. Added `test_collect_mutable_module_constants` in `src/code_lint/ast/python.rs` and 7 additional `rule_test!` cases in `src/code_lint/rules/mutable_module_constant.rs`.
- **User-facing text fixes applied**:
  1. Removed the unconditional `(or types.MappingProxyType or collections.abc.Mapping for a dict)` parenthetical from `TEMPLATE.suggestion`, added `"type"` to `TEMPLATE.summary`, refined `TEMPLATE.rationale` to cover both typed and initialized constants, and added `"without Final"` to the rename alternative in `TEMPLATE.suggestion` and `RuleDoc::why_is_this_bad`.
  2. Listed attribute and unpacking targets (`config.ALLOWED = ...`, `A, B = ...`) explicitly in `RuleDoc::what_it_does`.
