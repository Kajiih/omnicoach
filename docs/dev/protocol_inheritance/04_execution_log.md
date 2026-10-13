# Explicit Protocol Inheritance: 04 Execution Log

> [!IMPORTANT]
> **Status: PENDING** (worker, 2026-10-14). Phase 4 of 7. Design: [03_design.md](03_design.md).

## What was built

| Item | File |
| :--- | :--- |
| `uninherited-protocol` (`CodeRule<CountOption>`, `min-methods` default `2`, default `ban`) | `src/code_lint/rules/uninherited_protocol.rs` |
| `PythonClassInfo::is_protocol()`, `is_abc()`, `is_named_tuple()` (H1, H2, H3) | `src/code_lint/ast/python/classes.rs` |
| `is_protocol_or_abc_class` split into private `is_protocol_class` and `is_abc_class` (behaviour unchanged) | `src/code_lint/ast/python/classes.rs` |
| `is_fake_class_name` moved from `fake_without_protocol.rs` and made public (H5) | `src/code_lint/ast/python/classes.rs` |
| Registration | `src/code_lint/rules.rs`, `tests/snapshots/cli__list_rules.snap` |
| Mutations and unmutated cases | `scripts/exemptions/protocol_inheritance.py` (new cluster), `scripts/exemptions/tests_cluster.py` (paths of the moved `Fake*` name mutations) |

Python only, `RuleTarget::All`, topic `static-typing`, `Precision::Heuristic`, `Consensus::Opinionated`, `ImpactedQuality::Reliability`, span = the class name (D11).

The check walks `extract_classes` once. Each `Protocol` gets its method set: counted methods of the `Protocol` and of its `local_ancestors` (D3). A counted method is a public `def` / `async def` defined directly in the class whose name is not also a `@property` / `@cached_property` getter, so setters and deleters of a property are excluded with it (D4, E5, E6). For each checked class (`is_checked_class`: E2, E3, E9), `uninherited_protocols` keeps the `Protocol`s that are bound by name (D10), have at least `min-methods` methods (E4), are a subset of the class's counted methods (D5), and are not named by one of the class's `local_ancestors` (E1). It then drops any matched `Protocol` that is an in-file ancestor of another matched one (E8).

## Deviations from 03 and clarifications

- **E3b (`TypedDict`) is not implemented.** A `TypedDict` body may only contain annotations; defining a method raises `TypeError` at class creation. So it can never match a `Protocol` with `min-methods` methods, and a `pass` case could not kill its mutation. H3's `is_typed_dict()` was therefore not added.
- **H4 already existed** as `PythonFunctionSignature::is_property_getter()` (`property`, `builtins.property`, `functools.cached_property`, `cached_property`). It was reused instead of adding `is_property()`. `abstractproperty` is not recognised; it has been deprecated since Python 3.3.
- **An unimported `Protocol` base counts as `typing.Protocol` (D2 refinement).** `PROTOCOL_PATHS` keeps the bare `Protocol` entry of the former `PROTOCOL_AND_ABC_PATHS`, so the split of `is_protocol_or_abc_class` changes no existing rule. This also matches how `is_structural_marker` and `fake-without-protocol` treat unbound names, and lets the documented example omit `from typing import Protocol` (per the `Example` contract). Names that resolve elsewhere (`from asyncio import Protocol`) or are defined in the file are still rejected (E7).
- **`is_abc()` does not accept an `ABCMeta` base.** H2 listed it, but the existing helper accepted only `ABC` bases and `metaclass=ABCMeta`. A class whose base is `ABCMeta` is a metaclass, not an abstract class, so the existing behaviour was kept.
- **E1 is checked through `local_ancestors` only (D10 simplification).** A protocol counts as inherited when one of the class's `local_ancestors` has the protocol's name. A separate direct-base name check would add nothing: `local_ancestors` already resolves direct bases (including `Store[int]`). The only extra case it would cover is a base defined after the subclass, which raises `NameError` at runtime.
- **E9 uses the existing predicate unchanged:** `is_fake_class_name(&class.name)` and every base `is_structural_marker`. `has_collaborator_base` (H5) is the negation of that one-liner, so no helper was added.
- **`RuleTarget::All`** is the name of the design's `RuleTarget::AllFiles`.
- **`what_it_does` wording:** it omits the `typing_extensions` detail (covered by the `NAME_RESOLUTION` reference), and it states the most-derived reporting (E8) so readers can predict the single finding.

## Tests

| `pass` | `fail` | Mutations (all killed) |
| ---: | ---: | :--- |
| 14 | 10 | E1a, E1b, E2a, E2b, E3a, E3c, E4, E5, E6, E7 (in `classes.rs`), E8 (on the `most_derived_protocol_reported` `fail` case), E9 |

`missing_protocol_method` is listed as `Unmutated` (positive layout). The `fail` cases cover: a class defined before its `Protocol`, `async` methods, extra methods, `typing_extensions.Protocol[T]`, `typing.Protocol`, an unrelated base (D6), `Fake*` with a helper base (D9), methods from an extended `Protocol` (D3) and most-derived reporting (D8). The `{code}\n{code}` repetition exercises D10 in every `fail` case.

`python3 scripts/exemption_mutations.py uninherited_protocol`: 14/14 killed. The coverage check also reports 7 unlisted `pass` cases of other rules (`bare_multiline_string`, `inline_public_attribute_annotation`, `mutable_dataclass`, `nullable_collection_return`, `unslotted_dataclass`). They are not related to this rule and were left alone.

## Verification

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings -D clippy::pedantic`, and `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items`: clean.
- `cargo test`: passes, including `tests/registry.rs` and self-dogfooding. Self-dogfooding caught `uncolocated-helper` on the split helpers; they moved to the trailing helper section. `test_pass_cases_test_one_behaviour` rejected the name `private_and_dunder_methods_not_counted` (one filter, `starts_with('_')`, covers both), so the case is now `underscore_prefixed_methods_not_counted`. The only failure is `list_tags`, caused by the lead's concurrent `suppression-directives` topic edit in `taxonomy.rs`.

## ROADMAP

Not edited by the worker (the lead owns `ROADMAP.md` during this batch); the proposed text is in the worker's final report.
