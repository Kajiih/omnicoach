# Phase 2: Gather Resources and References — `repeated-literal`

This document records **Phase 2 (Gather Resources and References)** for `repeated-literal`. It builds on the validated [01_understand.md](01_understand.md) (decisions D1–D8).

> Status: **VALIDATED** (2026-10-03). Decisions:
> - **R-D2** accepted: `RuleTarget::SourceOnly`.
> - **R-Dogfood**: extract constants in the 4 production files; `omni:disable-file [repeated-literal]` in `ast/python.rs`, `ast/rust.rs`, `ast/statements.rs` for now. Typed (`type-sitter`) vs grammar-validated node kinds is a `ROADMAP.md` investigation (Architecture & Conformance), since plain kind strings are not validated like enums.
> - **R-D4, R-D6, R-D7** carried into Phase 3 for confirmation in [03_plan.md](03_plan.md).
> - Historical record: 03 §7 supersedes the `case` exemption (patterns are counted), and 04 §2 the opted-out files (`ast.rs` instead of `ast/statements.rs`).

Confidence markers: ✅ verified against official docs/source this session · ⚠️ synthesized from tool behavior/ecosystem discussions.

---

## 1. External State of the Art (`Q1`)

### 1.1 Deep-Dive on SOTA Tools & Style Guides

1. **Go `goconst` (`jgautheron/goconst` in `golangci-lint`)** ✅
   - **Mechanism**: Walks the Go AST (`visitor.go`) inspecting `ast.BasicLit`. Unquotes string literals via `strconv.Unquote` (normalizing raw `` `foo` `` and interpreted `"foo"` strings to their underlying content).
   - **Existing `const` Handling (`match-constant` & `find-duplicates`)**:
     - Records `token.CONST` declarations in a separate `consts` map rather than the wild-occurrences `strs` map.
     - **`match-constant: true` (default)**: If a wild literal matches a value in `consts`, `goconst` flags the wild literal and suggests the existing `const` identifier.
     - **`find-duplicates: false` (default)**: Two `const` declarations sharing the same value are **not** flagged against each other unless `find-duplicates: true` is explicitly enabled—directly validating **D7 (Option 3a)** and Python Tip #069's *"same value, different meaning"*.
   - **False-Positive Lessons from `goconst`**:
     - **Length-only filtering (`min-len: 3`) fails on both ends**: It still flags repeated punctuation/formatting separators of length $\ge 3$ (`"---"`, `"..."`, `" / "`, `"```"`) while missing 2-character domain strings (`"id"`, `"ok"`, `"jj"`). **D5**'s `< 2` chars OR zero-alphanumeric rule (`!content.chars().any(char::is_alphanumeric)`) solves both.
     - **Blanket `ignore-calls: true` is too blunt**: Ignoring all function arguments hides real domain literals (`req.Header.Set("X-Trace-Id", ...)`). The actual source of call-site noise in Go was `fmt.Sprintf` / `log.Printf` / `errors.New` format strings—which in Rust corresponds specifically to compile-time formatting, logging, and assertion macros (**D6**).

2. **SonarSource `S1192` ("String literals should not be duplicated") & `S109` ("Magic numbers should not be used")** ✅
   - **`S1192` (`sonar-python`, `sonar-java`, `sonar-rust`, `sonarjs/no-duplicate-string`)**:
     - **Syntactic exemptions**: Module/class/function docstrings, PEP 484 type annotations (forward references and `Literal[...]`), f-strings containing interpolation expressions, imports, and attributes/annotations (`#[...]`, `@...`).
     - **Controversial string filters in `S1192` / `sonarjs`**: `sonar-python` ignores strings `< 5` chars; `sonarjs` ignores `< 10` chars and single words matching `/^\w*$/`. The `/^\w*$/` exemption was added to avoid flagging object keys, but it completely blinds the rule to single-word domain literals (`"production"`, `"timeout"`, `"bearer"`, `"utf8"`).
   - **`S109` & Kotlin `detekt` `MagicNumber`** ✅:
     - `S109` flags single occurrences (`min = 1`) of numbers outside `-1, 0, 1` and is disabled by default in Sonar's *"Sonar way"* profile due to noise.
     - Kotlin `detekt` refines `MagicNumber` with `ignoreNumbers = ['-1', '0', '1', '2']` (identical to **D5**'s integer exemption set) and exempts `const val`, `companion object` properties, enums, and annotations.

3. **Python Tip #069 (`#no_magic`), Ruff `PLR2004`, Pylint `R2004`, and `wemake-python-styleguide` `WPS226` / `WPS432`** ✅
   - **Python Tip of the Week #069 ("Prefer constants over wild values", `#no_magic`)** ✅:
     - Extract repeated/magic values to `SCREAMING_SNAKE_CASE` constants or `Enum` members.
     - Never extract self-named trivial values (`_ZERO = 0`, `_TWO = 2`, `_COMMA = ","`, `_EMPTY = ""`).
     - Never merge two unrelated constants that happen to share the same value (`MAX_RETRIES = 3` and `TUPLE_ARITY = 3`).
   - **Ruff `PLR2004` & Pylint `R2004` (`magic-value-comparison`)** ✅:
     - Exempts `0`, `1`, `-1`, `0.0`, `1.0`, `""`, `b""`, `"__main__"`. Only checks `Compare` nodes at threshold 1, which forces Ruff to ignore `str` and `bytes` by default (`allow-magic-value-types = ["str", "bytes"]`).
   - **`wemake-python-styleguide` `WPS226` (`OverusedStringViolation`)** ✅:
     - Direct Python precedent for per-file repeated string literals.
     - Tracks `str` and `bytes` values in separate namespaces (`b"foo"` $\ne$ `"foo"`), normalizes quotes/raw prefixes, and exempts all docstring positions (module, class, function, attribute, type-alias) and `typing.Literal[...]` annotations.
     - Historical false-positive issues (#1124, #1493): originally flagged delimiter strings (`", "`, `""`) and f-string segments until explicit exemptions were added.

---

### 1.2 Comparison Table (`R1`–`R8`)

| ID | Reference | Key Ideas | Adopt / Adapt / Reject | Why |
| :--- | :--- | :--- | :--- | :--- |
| **R1** | **Go `goconst`** (`golangci-lint`) ✅ | Separate `consts` vs wild `strs` tables; `match-constant: true` flags wild uses matching an existing `const`; `find-duplicates: false` allows two `const`s to share a value; `ignore-calls: true`. | **Adopt** `match-constant` + `find-duplicates: false` (**D7** Option 3a).<br>**Adapt** call exemption to compile-time macros only (**D6**).<br>**Reject** `min-len: 3` and `eval-const-expressions` (**NG3**). | `match-constant` + `find-duplicates: false` is the exact model of D7. Blanket `ignore-calls` hides real domain arguments; D6's Rust macro list targets format/panic/assert macros specifically. |
| **R2** | **SonarSource `S1192`** (`sonar-python`, `sonar-java`, `sonar-rust`) ✅ | Per-file duplicate string check; unquotes literals; ignores docstrings, PEP 484 type annotations, interpolated f-strings, imports, and `#[...]` attributes; ignores `< 5` chars. | **Adopt** AST exemptions (docstrings, type annotations, interpolated f-strings, Rust attributes) and quote normalization.<br>**Reject** `< 5` length cutoff. | A `< 5` length cutoff misses `"json"`, `"utf8"`, `"ban"`, `"GET"`, `"POST"`, `"id"`. D5's `< 2` chars or non-alphanumeric rule prevents delimiter noise without blinding short domain words. |
| **R3** | **SonarJS `no-duplicate-string`** ✅ | Ignores `< 10` chars, single words `/^\w*$/`, `"application/json"`, `import`/`export`, `'use strict'`, and `switch`/`case` test values. | **Reject** `< 10` chars and `/^\w*$/`.<br>**Adapt** `switch`/`case` insight for Python `match`/`case` (see §2.2). | Ignoring `/^\w*$/` defeats Tip #069 by ignoring every single-word identifier (`"timeout"`, `"production"`). |
| **R4** | **SonarSource `S109` & Kotlin `detekt` `MagicNumber`** ✅ | `detekt` defaults `ignoreNumbers = ['-1', '0', '1', '2']` and exempts `const val`, `companion object` properties, enums, annotations. `S109` (`min = 1`) is off by default due to noise. | **Adopt** `detekt`'s trivial number set (`0`, `1`, `-1`, `2`, plus `0.0`, `1.0`, `-1.0` in **D5**) and `min-occurrences = 2` (**D3**, **NG2**). | Confirms `0, 1, -1, 2` is the SOTA consensus trivial integer set and validates why single-occurrence magic-number linting is a non-goal (**NG2**). |
| **R5** | **Python Tip #069 (`#no_magic`)** ✅ | Extract repeated/magic values to `UPPER_CASE` constants or `Enum`s; never extract `_ZERO = 0`, `_TWO = 2`, `_COMMA = ","`, `_EMPTY = ""`; never merge two unrelated constants with the same value. | **Adopt** in full (**D5**, **D7**). | Normative design authority for this rule. |
| **R6** | **Ruff `PLR2004` & Pylint `R2004`** (`magic-value-comparison`) ✅ | Exempts `0`, `1`, `-1`, `0.0`, `1.0`, `""`, `b""`, `"__main__"`. Only checks `Compare` nodes at `min-occurrences = 1`. | **Reject** restricting to `Compare` nodes. (Note: `"__main__"` in `if __name__ == "__main__":` occurs once per file, so `min-occurrences = 2` naturally exempts it.) | Repeated literals across assignments, return values, dict keys, and call arguments are just as coupled as comparisons. |
| **R7** | **`wemake` `WPS226` (`OverusedStringViolation`)** ✅ | Per-file string & bytes counter; distinguishes `str` vs `bytes`; normalizes quotes; exempts all docstring positions and `Literal[...]` type annotations. | **Adopt** `str` vs `bytes` separation, quote normalization, and attribute/type-alias docstring awareness. | `b"foo"` and `"foo"` have different types and cannot share a single Python/Rust constant. |
| **R8** | **Checkstyle `MultipleStringLiterals` & ESLint `no-magic-numbers`** ✅ | Checkstyle defaults to `allowedDuplicates = 1` (i.e. `min-occurrences = 2`) and ignores annotations. | **Adopt** `min-occurrences = 2` default (**D3**). | Matches Checkstyle's `allowedDuplicates = 1` (`min-occurrences = 2`). |

---

## 2. Internal Codebase Architecture & Tree-Sitter Analysis (`Q1`)

### 2.1 Taxonomy, Placeholders, & Architectural Boundaries
1. **Taxonomy ([taxonomy.rs](../../../src/rule_declaration/taxonomy.rs))**:
   - `Topic::LITERALS` is already defined at [taxonomy.rs:178-185](../../../src/rule_declaration/taxonomy.rs#L178-L185) (`Tag::Literals`, parent `Tag::Style`, *"Writing string and number literals (multiline strings, magic numbers). Not identifiers or formatting APIs."*).
   - `Consensus::Opinionated` is the valid enum variant ([taxonomy.rs:268-276](../../../src/rule_declaration/taxonomy.rs#L268-L276)) and matches `repeated-index-access`, `bare-multiline-string`, and `primitive-duration`.
2. **Template Placeholders & Verbs ([registry.rs](../../../tests/registry.rs))**:
   - `SUGGESTION_VERBS` ([registry.rs:372](../../../tests/registry.rs#L372)) already includes `"Extract"`.
   - `PLACEHOLDERS` ([registry.rs:385-401](../../../tests/registry.rs#L385-L401)) already includes `"expression"` and `"count"`. Using ``Literal `{expression}` is repeated {count} times in this file.`` requires zero changes to `tests/registry.rs` and keeps quotes inside backticks as required by `test_violation_templates_follow_style_guide`.
3. **AST Encapsulation ([architecture_conformance.rs](../../../tests/architecture_conformance.rs))**:
   - Following `ast::collect_positional_reads` ([ast.rs:285-332](../../../src/code_lint/ast.rs#L285-L332)) and `ast::find_unwrapped_multiline_strings` ([ast.rs:334-345](../../../src/code_lint/ast.rs#L334-L345)), language-specific Tree-sitter traversal lives in `code_lint::ast::python` and `code_lint::ast::rust`, exposed via a language-neutral `ast::collect_literal_occurrences(file)` helper in [ast.rs](../../../src/code_lint/ast.rs).

### 2.2 Critical Tree-Sitter Grammar Pitfalls & Edge Cases
1. **Rust Tuple Field Indexing (`t.0`, `t.3`)**:
   - In `tree-sitter-rust`, tuple indexing (`row.0`, `row.3`) is a `field_expression` whose `field` child has `kind() == "integer_literal"` (see [rust.rs:1108](../../../src/code_lint/ast/rust.rs#L1108)).
   - Any `integer_literal` that is the `field` child of a `field_expression` **must be skipped**—it is a struct/tuple field access, not a numeric literal value.
2. **Rust Macro `token_tree` Traversal**:
   - Unlike compound AST expressions, lexical literal nodes (`string_literal`, `raw_string_literal`, `integer_literal`, `float_literal`) **are** emitted inside `macro_invocation` $\to$ `token_tree` (see `find_unwrapped_multiline_strings` in [rust.rs:629-701](../../../src/code_lint/ast/rust.rs#L629-L701)).
   - Therefore, the Rust collector must check enclosing macros via `resolve_preceding_macro_path` ([rust.rs:591-620](../../../src/code_lint/ast/rust.rs#L591-L620)) to apply the **D6** macro exemptions.
3. **Python PEP 634 `match` / `case` Pattern Semantic Trap**:
   - In **Rust**, `const FOO: &str = "foo"; match x { FOO => ... }` is valid Rust: an uppercase `const` identifier in a pattern is resolved as a constant value pattern.
   - In **Python**, `FOO = "foo"` followed by `match x: case FOO:` is a **semantic bug**! Per PEP 634, an unqualified name (even `UPPER_CASE`) in a `case` pattern is a **capture pattern** that matches *anything* and binds it to the local variable `FOO` (Python only treats *dotted* names like `case Color.RED:` as value patterns).
   - **Recommendation**: Exempt literals inside Python `case_pattern` AST nodes under **D6** (alongside `typing.Literal[...]`), because replacing `case "foo":` with a module-level constant `case FOO:` breaks program semantics in Python.
4. **Unary Negation (`-1`, `-42`, `-1.0`)**:
   - In Python, `-42` is a `unary_operator` with `operator == "-"` and `argument` of kind `integer` or `float`.
   - In Rust, `-42` is a `unary_expression` (in expressions) or `negative_literal` (in `match` patterns) with operator `"-"`.
   - When visiting a unary negation wrapping an integer/float literal, the collector must record the **parent unary node** with the negated value (`-1`, `-42`, `-1.0`) and skip its inner positive child so:
     1. `-1` and `-1.0` are recognized as trivial (**D5**) rather than visited as `1` or `1.0`;
     2. `-42` and `42` are distinct values; and
     3. the diagnostic span highlights `-42` rather than `42`.
5. **Composite Constant Initializers vs. Direct Scalar Constants**:
   - In both Rust (`const_item`, `static_item`, `enum_variant`) and Python (`UPPER_SNAKE_CASE` or `Final`-annotated assignments at module/class scope, plus `Enum` class members), constants are often composite structs, slices, tuples, or dicts (e.g., `pub const RULE: CodeRule = ...`, `ALLOWED_MODES = ("read", "write")`).
   - **Rule**:
     - Every literal inside the initializer subtree of a constant declaration is **exempt from `wild_uses`** (never flagged as a wild literal).
     - Only **direct scalar constant definitions** (`const FOO: &str = "foo"`, `FOO = "foo"`, `Variant = 10`) populate `const_defs` for the `1 const_def + 1 wild_use` check. Why? In Omni itself, `pub const RULE: CodeRule` contains metadata strings like `CountOption { key: "min-occurrences" }` or `Example { flagged_span: "..." }`; treating deep struct fields inside `CodeRule` as scalar `const_defs` would falsely flag a single wild use of the same word in `check_file`.

---

## 3. Self-Dogfooding Audit on Omni (`src/` and `tests/`) (`Q2`)

We audited Omni's own Rust codebase against the `repeated-literal` rules (`min-occurrences = 2`, **D5** trivial literal exemptions including non-alphanumeric strings, and **D6** syntactic exemptions).

### 3.1 Production Code Outside `src/code_lint/ast/`
With **D5** (non-alphanumeric strings like `", "`, `"--"`, and `"```"` exempted) and composite `const` initializers exempted from `wild_uses`, the entire production codebase outside `src/code_lint/ast/` has only **4 files** with repeated literals:

| File | Repeated Literal(s) | Occurrences | Analysis |
| :--- | :--- | :--- | :--- |
| [packed_assertion.rs](../../../src/code_lint/rules/packed_assertion.rs) | `"a comparison against a collection of boolean literals"`<br>`"construct"`<br>`"callee"` | 2× (L137, L169)<br>4× (L119, L136, L158, L168)<br>2× (L120, L139) | **True positive**: a 53-char diagnostic description and template keys duplicated across Rust and Python helpers. Easily extracted to `const`. |
| [environment_variable_in_function.rs](../../../src/code_lint/rules/environment_variable_in_function.rs) | `"expression"`<br>`"function"` | 2× (L194, L209)<br>2× (L194, L209) | Template parameter keys passed in two `rule.diagnostic_at_node(...)` calls. |
| [suppression.rs](../../../src/code_lint/suppression.rs) | `"rule"` | 2× (L467, L482) | Template parameter key passed in two `render_diagnostic` calls. |
| [comments.rs](../../../src/code_lint/semantic/comments.rs) | `3` | 2× (L146, L158) | **True positive**: `words < 3` and `words <= 3` in `is_substantive_explanation`. Extracting `const MIN_SUBSTANTIVE_WORDS: usize = 3;` improves clarity. |

### 3.2 Tree-Sitter Grammar Adapter Files (`src/code_lint/ast/python.rs` & `src/code_lint/ast/rust.rs`)
In [python.rs](../../../src/code_lint/ast/python.rs) (~60 repeated strings), [rust.rs](../../../src/code_lint/ast/rust.rs) (~26 repeated strings), and [statements.rs](../../../src/code_lint/ast/statements.rs) (2 repeated strings):
- Almost every repeated literal is a Tree-sitter CST node kind (`"identifier"` 24×, `"function_definition"` 12×, `"call"` 10×, `"token_tree"` 9×, `"attribute_item"` 7×) or field name (`"body"` 18×, `"name"` 18×, `"left"` 11×, `"value"` 11×).
- **Options for `src/code_lint/ast/`**:
  1. Extract `const` node-kind and field-name constants in `ast/python.rs`, `ast/rust.rs`, and `ast/statements.rs` (preventing typos like `"funcion_definition"` across 2,800 lines of CST traversal), **or**
  2. Add file-level `// omni:disable-file [repeated-literal] -- Tree-sitter CST node kinds and field names` to `ast/python.rs`, `ast/rust.rs`, and `ast/statements.rs`.

### 3.3 Test Files (`tests/*.rs`) and Inline `#[cfg(test)]` Modules (`RuleTarget::SourceOnly` vs `RuleTarget::All`)
- In **D6**, `rule_test!`, `#[case(...)]` attributes, and `assert*!` / `indoc!` macros are exempt.
- **However**, in unit and integration test suites ([cli.rs](../../../tests/cli.rs), [architecture_conformance.rs](../../../tests/architecture_conformance.rs), [runner.rs](../../../src/code_lint/runner.rs) `mod tests`, [python.rs](../../../src/code_lint/ast/python.rs) `mod tests`), independent `#[test]` functions repeatedly pass self-contained fixture paths, CLI flags, rule names, and code snippets to helper functions:
  - [runner.rs](../../../src/code_lint/runner.rs) (`#[cfg(test)] mod tests`): `"single-letter-name"` (8×), `"math.py"` (7×), `"x = 1"` (3×), `"main.py"` (3×), `"error-log-in-except"` (3×) — whereas production code in `runner.rs` (L1–298) has **0** repeated literals.
  - [cli.rs](../../../tests/cli.rs): `".omnilint.toml"` (8×), `"nested-function"` (7×), `"test_sample.py"` (6×), `"--list-rules"` (5×), `"--explain"` (4×), `"--tag"` (4×).
  - [architecture_conformance.rs](../../../tests/architecture_conformance.rs) (`#[cfg(test)] mod tests`): `"src/code_lint/ast.rs"` (7×), `"code_lint::ast"` (7×), `"src/code_lint/rules/my_rule.rs"` (4×).
- **Tradeoff to decide (`RuleTarget::SourceOnly` vs `RuleTarget::All`)**:
  - In tests, repeating `"math.py"` or `"x = 1"` across self-contained `#[test]` functions is standard DAMP (*Descriptive and Meaningful Phrases*) test hygiene; extracting `const X_EQ_1: &str = "x = 1";` hurts test readability.
  - Because [runner.rs](../../../src/code_lint/runner.rs) automatically skips test files and filters out inline `#[cfg(test)]` byte ranges for `RuleTarget::SourceOnly` rules (just like `nested-function`, `type-cast`, `environment-variable-in-function`, and `identical-positional-types`), changing **D2** from `RuleTarget::All` to **`RuleTarget::SourceOnly`** eliminates all test-fixture noise cleanly.
  - *(Note: If `RuleTarget::SourceOnly` is used, `check_file` should also ignore literals inside inline `#[cfg(test)]` ranges in Rust files so a literal in `#[cfg(test)]` cannot pair with a single literal in production code.)*

---

## 4. `rule_test!` Harness Design for Per-File Value Aggregation (`Q3`)

### 4.1 Why `assert_every_occurrence_reported` Fails When Literals Are Identical
In [test_utils.rs](../../../src/test_utils.rs) (`assert_every_occurrence_reported`):
1. Under **D7 (Option 3a)**, a 2-occurrence wild fail case (`connect("https://api.example.com")` + `fetch("https://api.example.com")`) emits **1 diagnostic** on the first pass (at the 2nd occurrence, `span`), and a `const_def + wild_use` fail case also emits **1 diagnostic** (at the wild occurrence, `span`). This matches `assert_rule_fail`'s single-diagnostic contract and `Example::flagged_span`!
2. However, on the second pass, `assert_every_occurrence_reported` runs the rule on `format!("{code}\n{code}")` and expects `vec![span, span + offset]` (2 diagnostics).
3. Because both copies in `format!("{code}\n{code}")` contain the **exact same literal values**, all 4 occurrences of `"https://api.example.com"` collapse into a **single** per-file `LiteralValue` bucket (`wild_uses` of length 4), so `wild_uses[1..]` flags occurrences 2, 3, and 4 (**3 diagnostics instead of 2**), AND fails to test that `check_file` reports multiple distinct literal buckets!

### 4.2 Clean Solution: Perturb Literal Values in the Second Copy of `assert_every_occurrence_reported`
If `assert_every_occurrence_reported` (for per-file literal rules, or via a byte-length-preserving literal perturbation in the second copy when `format!("{code}\n{code}")` is constructed) mutates one character/digit inside each string/numeric literal token of the second copy (e.g., `"https://..."` $\to$ `"ittps://..."`, `404` $\to$ `504`):
- Every AST node in the second copy keeps the **exact same byte length and byte span** (`span.start + offset..span.end + offset`).
- Copy 1's literals form bucket 1 (flagging `span`), and Copy 2's literals form **an independent bucket 2** (flagging `span.start + offset..span.end + offset`).
- `run_code_rule` returns `vec![span, span + offset]` (2 diagnostics), and any implementation that stops after the first bucket fails `assert_every_occurrence_reported`!

---

## 5. Proposed Refinements to `01_understand.md` Before Phase 3

| ID | Refinement | Rationale |
| :--- | :--- | :--- |
| **R-D2** | Switch `target` from `RuleTarget::All` to **`RuleTarget::SourceOnly`** (and skip `#[cfg(test)]` ranges during collection in Rust). | Self-dogfooding shows test suites (`tests/cli.rs`, `runner.rs` `mod tests`, etc.) repeat fixture filenames (`"math.py"`, `".omnilint.toml"`) and snippet strings (`"x = 1"`) across self-contained `#[test]` functions (DAMP), matching SOTA (`goconst`, `PLR2004`). |
| **R-D4** | Set `topics: &[Topic::LITERALS]` and `consensus: Consensus::Opinionated`. | `Topic::LITERALS` already exists in `taxonomy.rs:178-185`; `Consensus::Opinionated` is the valid enum variant. |
| **R-D6** | Add **Python `case_pattern`** (`match`/`case` patterns) and **Rust tuple field access** (`field_expression.field`) to syntactic exemptions. | In Python PEP 634, replacing `case "foo":` with a module constant `case FOO:` turns it into a capture pattern that silently matches everything. In `tree-sitter-rust`, `t.0` has `kind() == "integer_literal"`. |
| **R-D7** | Distinguish **direct scalar constant definitions** (which populate `const_defs`) from **composite constant initializers** (whose inner literals are exempt from `wild_uses` without acting as scalar `const_defs`). | Prevents metadata/doc structs (`pub const RULE: CodeRule`, `CountOption`) from colliding with a single wild literal in the same file, while still exempting all literals inside `const`/`static` tables from being flagged. |
| **R-Dogfood** | For Omni's own `src/`:<br>1. Extract constants in the 4 production files (`packed_assertion.rs`, `environment_variable_in_function.rs`, `suppression.rs`, `comments.rs`).<br>2. For `src/code_lint/ast/python.rs`, `src/code_lint/ast/rust.rs`, and `src/code_lint/ast/statements.rs` (~85 Tree-sitter grammar node kind & field name strings): choose between extracting `const` node-kind/field constants or adding `// omni:disable-file [repeated-literal]`. | Keeps `cargo run -- .` 100% clean on the repository. |

---

## 6. Sources
- Go `goconst`: https://github.com/jgautheron/goconst · https://golangci-lint.run/usage/linters/#goconst
- SonarSource `RSPEC-1192` ("String literals should not be duplicated"): https://rules.sonarsource.com/python/RSPEC-1192/
- SonarJS `no-duplicate-string`: https://github.com/SonarSource/eslint-plugin-sonarjs/blob/master/docs/rules/no-duplicate-string.md
- Kotlin `detekt` `MagicNumber`: https://detekt.dev/docs/rules/style/#magicnumber
- Ruff `PLR2004` (`magic-value-comparison`): https://docs.astral.sh/ruff/rules/magic-value-comparison/
- Pylint `R2004` (`magic-value-comparison`): https://pylint.readthedocs.io/en/stable/user_guide/messages/refactor/magic-value-comparison.html
- `wemake-python-styleguide` `WPS226` (`OverusedStringViolation`): https://wemake-python-styleguide.readthedocs.io/en/latest/pages/usage/violations/complexity.html
- Checkstyle `MultipleStringLiterals`: https://checkstyle.sourceforge.io/checks/coding/multiplestringliterals.html
