# Phase 4: Execution Log — `repeated-literal`

Records **Phase 4 (Execute)**. Each task ended with standard verification (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`). All tasks ended green.

---

## 1. Tasks

| # | Result |
| :--- | :--- |
| T0 | ROADMAP: the `no-repeated-literals` candidate is replaced by an implemented `repeated-literal` entry with three follow-ups (exempt-macro values, negative numbers in token trees / union patterns, the `ast` file opt-outs). |
| T1 | `LiteralValue`, `LiteralRole`, `LiteralOccurrence` and `collect_literal_occurrences` in `ast.rs`. Python collector with unit tests for roles and parsed values. |
| T2 | Rust collector with unit tests for roles, values (suffixes, raw / byte / C strings) and integer overflow. |
| T3 | `RepeatCheck { SameCode, DistinctLiterals }` threaded through the harness. `with_distinct_literals` rewrites the second copy's literals in place (H2). Unit tests cover rewriting and the three rejection cases. |
| T4 | `rules/repeated_literal.rs`, registered last in `CODE_RULES`. `rule_test!(RULE, repeat: DistinctLiterals, …)` with 43 named cases (61 after Phase 6). `cli__list_rules.snap` updated. |
| T5 | Dogfooding: constants extracted in the four planned files plus `sleep_in_tests.rs` and `diff.rs`. `omni:disable-file` added to `ast.rs`, `ast/python.rs` and `ast/rust.rs`. |

## 2. Deviations From the Plan

- **Escapes count as one unit in `is_trivial`.** `"\n"` and `"\t"` are trivial (one non-alphanumeric character), matching the intent of D5 rather than the raw source length.
- **The rewrite does more than toggle case (03 §3.5).** A string toggles its first unescaped ASCII letter or changes its first digit; a number changes its first digit after the sign and base prefix, into `3..=9` so it stays non-trivial, and a hex letter swaps within its pair. Equal values spelled differently (`0xFF`, `255`) and binary or octal digits are rejected loudly.
- **`ast.rs` is disabled instead of `ast/statements.rs`.** `statements.rs` has no finding, so a disable there would itself be flagged as unused. `ast.rs` repeats `"function"`.
- **Two extra dogfood files.**
  - `sleep_in_tests.rs`: a `CALLEE` constant.
  - `diff.rs`: `JJ_BINARY` / `GIT_BINARY` constants, so the program that runs and the binary named in errors cannot drift.
- **One line suppression beyond the plan's metric.** Git's `"diff"` subcommand matches Jujutsu's by coincidence. Making one constant for both would couple two unrelated CLIs, so it uses `omni:ignore [repeated-literal]` with a reason.
- **Python collector restructured for `single-letter-name`.** The walker's `push` closure triggered that rule on its `'a` lifetime. Negative `case` patterns are now a `"case_pattern"` arm of `literal_value`.
- **Known imprecision** (superseded in Phase 6: such numbers are now skipped, not miscounted). `-N` inside a Rust `token_tree`, or directly inside a Python `union_pattern`, counts as `N`. It is tracked in `ROADMAP.md`.
- **Extra test cases.**
  - `repeated_type_names_in_cast` (Python).
  - `test_module_literals_are_ignored` (Rust). Its module is named `integration`, because `tests/registry.rs` bans the text `mod tests` in rule files. Detection keys on `#[cfg(test)]`, not on the module name.

## 3. Verification

- **Mutation spot-checks**, each restored afterwards:
  - Dropping the docstring prune fails `pass::repeated_docstrings`.
  - Dropping the Rust `attribute_item` prune fails `pass::repeated_attribute_arguments`.
- **Manual checkpoint.** Ran `omni-code-lint` on a scratch Python fixture:
  - `"db.internal"`, 3 inline copies and no constant: copies 2 and 3 are flagged.
  - `30` with `TIMEOUT_S = 30`: the single inline use is flagged.
  - A repeated log message and a repeated `case -404:` are flagged.
  - The docstring is not flagged. This matches D7 (3a).
