# Phase 1: Understand — `repeated-literal`

This document records **Phase 1 (Understand)** for the candidate rule `repeated-literal` (`ROADMAP.md` → Candidate Rules, listed there under its pre-ADR-010 name `no-repeated-literals`).

> Status: **VALIDATED** (2026-10-03). Historical record: later phases supersede it where they differ (D2 target in 02, exemptions in 03 §7, Phase 6 fixes in 06).

---

## 1. Problem Statement & Sources

Repeating the same non-trivial string or numeric literal across a file scatters a single domain value across multiple sites without naming what it means. When the value changes (an endpoint path, a timeout, a header name, a status code, a numeric bound), updating one site while missing another silently desynchronizes the file.

Sources:
- **Python Tip of the Week #069 ("Prefer constants over wild values", `#no_magic`)**:
  - Extract repeated or magic literals into module-level named constants (`SCREAMING_SNAKE_CASE`).
  - Counter-examples from the tip:
    1. **Self-evident trivial values / delimiters** (`_ZERO = 0`, `_TWO = 2`, `_COMMA = ","`, `_EMPTY = ""`): naming a character or trivial number after itself adds noise without domain meaning.
    2. **Same value, different meaning**: two unrelated constants that happen to have the same literal value (`MAX_RETRIES = 3` and `TUPLE_ARITY = 3`) must remain separate constants rather than being merged into `_THREE = 3`.
- **`ROADMAP.md` (`no-repeated-literals`)**:
  - Multi-language rule (Python and Rust) flagging repeated string and numeric literals in a file above a configurable occurrence threshold, with a harness extension in `rule_test!` so per-file aggregation is tested without weakening the single-occurrence regression guard.

---

## 2. Rule Principle (What Is Enforced)

> **A non-trivial string or numeric literal that represents a shared value across a file must be defined once in a named constant, rather than duplicated inline.**

A literal occurrence in a file is flagged when:
1. **Non-trivial literal (`C1`)**: It is a string literal with $\ge 2$ characters containing at least one alphanumeric character, or a numeric literal outside the trivial set (`0`, `1`, `-1`, `2`, `0.0`, `1.0`, `-1.0`).
2. **Extractable syntactic position (`C2`)**: It is not in a position where the language grammar or type system forbids a constant identifier (e.g., docstrings, Python type annotations / `Literal[...]`, Python f-strings with `{...}` interpolations, Rust `#[...]` attributes, or Rust compile-time string macros such as `format!` / `println!` / `panic!` / `assert!`).
3. **Repeated wild occurrence (`C3`)**:
   - Let `const_defs` be the occurrences of that literal value as the initializer of a named constant (`UPPER_CASE = <lit>` in Python, `const` / `static` in Rust), and `wild_uses` be all other eligible occurrences in the file.
   - If `!const_defs.is_empty()`: a constant already exists for this value; once `1 + wild_uses.len() >= min-occurrences`, **every** occurrence in `wild_uses` is flagged as an un-extracted duplicate. (If `wild_uses` is empty—even when `const_defs.len() >= 2`—zero diagnostics are emitted, respecting Tip #069's *"same value, different meaning"*.)
   - If `const_defs.is_empty()`: no constant exists yet; once `wild_uses.len() >= min-occurrences`, **every duplicate occurrence after the first** (`wild_uses[1..]`) is flagged.

---

## 3. Goals & Non-Goals

### 3.1 Goals

| ID | Goal | Reason |
| :--- | :--- | :--- |
| **G1** | Detect repeated string and numeric literals in Python and Rust across all files (`RuleTarget::All`). | Both source and test files benefit from extracting shared constants or parameterizing repeated test cases. |
| **G2** | Distinguish constant definitions from wild inline uses (`Option 3a`). | Catches forgotten inline duplicates when a constant already exists (`CONST = "x"` + raw `"x"`), allows distinct constants to share a coincidental value (`MAX_RETRIES = 3` and `TUPLE_ARITY = 3`), and flags every duplicate occurrence (`2..=k`) when all occurrences are wild. |
| **G3** | Exempt non-alphanumeric delimiter strings, single-character strings, trivial numbers, and non-extractable syntax contexts without an arbitrary string-length cutoff. | Avoids both Tip #069's `_COMMA = ","` false positives and arbitrary `min-length = 5` false negatives on short domain strings like `"json"`, `"ban"`, `"utf8"`, or `"GET"`. |
| **G4** | Extend `rule_test!` cleanly for per-file value-aggregation rules. | Duplicating `{code}\n{code}` merges identical literals into one per-file group; `rule_test!` must verify multi-group completeness without a blanket opt-out that could hide early-return bugs. |
| **G5** | Pass `test_self_dogfooding_code_lint` across the entire repository. | Every enabled rule in Omni must pass on `src/` and `tests/`. |

### 3.2 Explicit Non-Goals

| ID | Non-Goal | Reason |
| :--- | :--- | :--- |
| **NG1** | Cross-file literal deduplication. | Omni's code-lint architecture is strictly per-file (`ParsedFile`). |
| **NG2** | Flagging a single occurrence of a "magic number" (`min-occurrences = 1`). | Covered by Ruff `PLR2004` (`magic-value-comparison`) and `primitive-duration`; a single occurrence is not a *repeated* literal. |
| **NG3** | Semantic string concatenation or arithmetic evaluation (`"a" + "b"` vs. `"ab"`, `60 * 60` vs. `3600`). | Literal normalization compares literal values, not constant-folded compound expressions (time-unit arithmetic has its own `ROADMAP.md` entry). |
| **NG4** | Autofix. | Naming a constant requires human domain judgement. |

---

## 4. Numbered Decisions

| ID | Decision | Rationale |
| :--- | :--- | :--- |
| **D1** | **Rule name**: `repeated-literal` (file `src/code_lint/rules/repeated_literal.rs`, const `RULE`). | Follows ADR 010 and `naming_and_message_style_guide.md` §1 (singular noun phrase naming the flagged pattern, no `no-` prefix). |
| **D2** | **Scope**: Both string and numeric literals in Python and Rust (`SupportLang::Python`, `SupportLang::Rust`), `RuleTarget::All`. | Matches `ROADMAP.md` and Tip #069; holding tests to the same standard encourages parameterization (`#[rstest]`, `@pytest.mark.parametrize`) and named test fixtures. |
| **D3** | **Options & enforcement mode**: `RuleOptions::code_rule(MIN_OCCURRENCES)` with `EnforcementMode::Ban` and `min-occurrences` (`CountOption`, default `2`). | `min-occurrences = 2` catches 2-site duplications (producer + consumer, check + message) that `3` misses; `Ban` matches the other 22 code rules. |
| **D4** | **Classification**: `Precision::Heuristic`, `Consensus::Universal`, `ImpactedQuality::Maintainability`, topic `&[&NAMING]` (or a dedicated constants/literals topic if appropriate — checked in Phase 2). | Same literal text in two distant places can occasionally be coincidental; `Precision::Heuristic` accurately reflects that. |
| **D5** | **Trivial literal exemptions**:<br>- **Strings**: Exempt strings with `< 2` characters or with zero alphanumeric characters (`!content.chars().any(char::is_alphanumeric)`, e.g. `""`, `" "`, `"\n"`, `", "`, `"::"`).<br>- **Numbers**: Exempt integer values `0`, `1`, `-1`, `2` and float values `0.0`, `1.0`, `-1.0` (including negative unary literals `-1` / `-1.0`). | Directly encodes Tip #069's `_ZERO` / `_TWO` / `_COMMA` / `_EMPTY` counter-examples without ignoring short domain strings like `"json"` or `"ban"`. |
| **D6** | **Syntactic exemptions**:<br>- **Python**: module/class/function docstrings; type annotations (parameter, return, and variable type annotations, plus `typing.Literal[...]` and `TypeVar` / `NewType` name strings); f-strings that contain `{...}` `interpolation` nodes (while still counting string literals *inside* `{...}` and interpolation-free f-strings).<br>- **Rust**: `#[...]` and `#![...]` attributes; compile-time string-literal macros (`format!`, `println!`, `eprintln!`, `write!`, `writeln!`, `panic!`, `assert!`, `assert_eq!`, `assert_ne!`, `debug_assert!`, `debug_assert_eq!`, `debug_assert_ne!`, `concat!`, `env!`, `option_env!`, `include_str!`, `include_bytes!`, `unreachable!`, `todo!`, `unimplemented!`, plus `violation_template!` / `rule_test!` / `architecture_component!`). | Every exempted construct is one where the language or compiler either forbids a `const` identifier or uses the string as metadata/documentation rather than a value. |
| **D7** | **Constant-definition awareness & Option 3a reporting**:<br>- A literal directly initializing a named constant (`UPPER_CASE = ...` in Python, `const` / `static` in Rust) is recorded as a constant definition (`const_defs`); all other eligible literals are recorded as `wild_uses`.<br>- If `!const_defs.is_empty()` and `1 + wild_uses.len() >= min_occurrences`, flag all `wild_uses`.<br>- If `const_defs.is_empty()` and `wild_uses.len() >= min_occurrences`, flag `wild_uses[1..]` (every duplicate after the first). | Ensures `CONST = "x"` + raw `"x"` flags the raw `"x"`, two unrelated constants `A = 3; B = 3` emit 0 diagnostics, and a 2-occurrence wild pair flags the 2nd occurrence (1 diagnostic). |
| **D8** | **Per-file `rule_test!` verification**: Extend `rule_test!` so per-file aggregation rules are verified to report multiple distinct groups rather than running `format!("{code}\n{code}")` with identical literal values. | Preserves the anti-early-return guarantee of `rule_test!` while supporting per-file value aggregation. |

---

## 5. Open Questions for Phase 2 (Gather Resources and References)

- **Q1 (External SOTA Survey)**: How do SonarQube (`S1192`), Pylint (`R0801` / magic-value extension), Flake8/Ruff (`PLR2004`), ESLint (`no-magic-numbers` / `sonarjs/no-duplicate-string`), and Clippy normalize literal values (e.g., `1_000` vs `1000`, `'foo'` vs `"foo"`, raw strings `r"foo"` vs `"foo"`) and handle composite constant initializers (e.g., `ALLOWED = ["foo", "bar"]` or `match` arms)?
- **Q2 (Self-Dogfooding Audit on Omni)**: When we run the proposed `repeated-literal` logic across Omni's own `src/` and `tests/`, which files trigger diagnostics, and what do those hits reveal about composite constant initializers (`const LIST: &[&str] = ...`), `match` arms, or test fixtures?
- **Q3 (`rule_test!` Harness Design for Per-File Value Aggregation)**: What is the cleanest, minimal way for `assert_every_occurrence_reported` (or `rule_test!`) to test that a per-file rule reports all groups without per-case boilerplate?
