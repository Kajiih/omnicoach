# Docstring optional argument: 04 Execution log

> [!NOTE]
> **Status: VALIDATED** (lead, 2026-10-12; implemented). Batch D (docstrings), worker phase 4 of 7. Design: [03_design.md](03_design.md).

## What was built

- Rule `unexplained-optional-argument` in `src/code_lint/rules/unexplained_optional_argument.rs`, registered in `src/code_lint/rules.rs`: Python only, topic `docstrings`, `RuleOptions::code_rule(())` (default `ban`), `Precision::Heuristic`, `Consensus::Opinionated`, `ImpactedQuality::Maintainability`, `RuleTarget::All`. Template, doc summary and example as in 03 §2 D10, D13 and §8.
- Helper `PythonParameterInfo::default_node` in `src/code_lint/ast/python/functions.rs`, set by `build_parameter_with_default` (`None` for variadics).
- Class docstrings: `Args:` entries are matched with the parameters of the `__init__` found by `PythonClassInfo::methods()`; no new class helper.
- Exemption mutations: 10 entries for this rule in `scripts/exemptions/docstrings.py` (E1–E9, with E9 split into a function case and a constructor case), plus 2 `Unmutated` entries (positive layouts; module docstrings and the parser-owned P1 continuation line).
- Tests: 13 `pass` and 17 `fail` cases, plus the generated `documented_examples` and `language_completeness`.

## Deviations from 03, with reasons

- **Lead decisions 1–4** (03 §0) are implemented: `{phrase}` reused, exact `(optional)` slot read and `(int, optional)` not, E9 in v1, Q1 and Q3 left out.
- **Clause end for `(optional)`.** D3 says "a trailing `(optional)`". It is matched at a clause end (end of the description, or before `.` / `;` and whitespace), the same test as the nullability markers, so `The timeout (optional). Must be positive.` is flagged too.
- **"A sentence that is only `Optional.`"** is matched as a clause: `optional` preceded by the start or `.` / `;` plus whitespace, and followed by a clause end. `The timeout; optional.` is therefore flagged as well.
- **E8** is tested with a `Returns:` and a `Raises:` entry in one case. `Returns:` / `Yields:` have no entries in the shared parser, so only the `Raises:` part kills the mutation; the `Returns:` part documents the boundary.
- **E9 for `Attributes:`.** Attributes are never matched with a signature (03 §0 decision 3), so the `class_attributes_entry` fail case uses an `__init__` with `retries: int = 3` to pin that.
- **Rule docs** name the chain with `annotated-type-in-docstring` (decision 2) and list the text-only `Attributes:` / `**kwargs` behavior in `known_problems`.
- **Suggestion punctuation.** D10's example sits inside the backticks with its period; the registry requires the field to end with a period, so it reads ``such as `If None, reads the default config file`.``.
- **E7 case** uses `retries=None`: with `retries=3`, E9 also exempted it and its mutation survived.

## Verification

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo doc --no-deps --document-private-items`: pass.
- Self-dogfooding: `test_self_dogfooding_code_lint` (in `cargo test`) reports nothing; a first draft defined `none_words` above its caller `is_explained`, which `callee-before-caller` caught. `cargo run -- code src/` itself was not run (no permission in the worker session).
- Mutations: `scripts/exemption_mutations.py` could not be run (no permission). Each of the 10 edits listed for this rule was applied by hand and the `pass` cases run with `RUSTFLAGS=--cap-lints=warn`, as the script does: 10/10 killed, each failing exactly its listed case (the E9 edit also fails the constructor case). The lead should re-run the script.
