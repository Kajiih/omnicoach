# Phase 2: Gather Resources and References — `quote-wrapped-placeholder`

This document records **Phase 2 (Gather Resources and References)** for `quote-wrapped-placeholder`. It builds on [01_understand.md](01_understand.md) (decisions `D1`–`D9`, open questions `Q1`–`Q4`).

> Status: **VALIDATED** (2026-10-03). Refinements `R-D5`, `R-D6`, and `R-D9` accepted.
>
> **Superseded in part (2026-10-05):** SQL strings and isolated quoted placeholders (`f"'{value}'"`) are no longer exempt; SQL injection is left to Ruff `S608`. The prose and quote heuristics moved from `ast/python/quote_wrapped.rs` into the rule file, on top of the construct-level `ast/python/format_strings.rs`. The rule's `what_it_does` is authoritative.

Confidence markers: ✅ verified against official docs/source this session · ⚠️ synthesized from tool behavior/ecosystem discussions.

---

## 1. External State of the Art (`Q1`, `Q2`)

### 1.1 Do Ruff, Pylint, Flake8, `wemake-python-styleguide`, or Refurb Have This Rule?

**No existing mainstream Python or Rust linter enforces `quote-wrapped-placeholder`.**
However, several SOTA rules inspect adjacent string-formatting constructs and provide direct architectural lessons on how to scope format-string checks without false positives:

1. **Ruff `RUF010` (`explicit-f-string-type-conversion`)** ✅
   - **What it detects**: Explicit calls to `str(x)`, `repr(x)`, or `ascii(x)` inside an f-string interpolation (`f"{repr(x)}"`) when the interpolation has no existing conversion flag, recommending `f"{x!r}"`, `f"{x!s}"`, or `f"{x!a}"`.
   - **Relevant guardrails**:
     - Skips any `FormattedValue` that already carries a `conversion` flag (`!r`, `!s`, `!a`).
     - For `str(x)` when a `format_spec` is present, checks compatibility because conversions run *before* `__format__`.
   - **Gap**: `RUF010` only checks `f"{repr(x)}"`, never `f"'{x}'"` or `"'{x}'".format(...)` or `"'%s'" % x`.

2. **Ruff `F501`–`F525` (`pyflakes` format rules) & Pylint `W1300`–`W1310` (`stdlib` string format checkers)** ✅
   - **How SOTA tools identify `.format()` and `%` format strings**:
     - Neither Ruff/Pyflakes nor Pylint **ever** runs `.format()` or `%` regexes across all `ast.Constant` strings in a file (unlike the legacy linter's `_check_constant_string`).
     - Instead, they strictly gate inspection on syntactic use:
       1. **`str.format` / `str.format_map`**: A string literal that is the `func.value` receiver of a `.format(...)` or `.format_map(...)` `Call` node.
       2. **`%` operator**: A string literal that is the `left` operand of a `BinOp(op=Mod)`.
       3. **`logging` calls**: A string literal passed as the `msg` argument of a `logging` / `logger` call (`debug`, `info`, `warning`, `warn`, `error`, `exception`, `critical`, `fatal` at arg index 0, or `log` at arg index 1) when positional format arguments follow `msg`.
   - **Why this matters**: Adopting this exact syntactic gate (**C1** in `01_understand.md`) eliminates 100% of the legacy linter's false positives on docstrings, regexes, shell/awk literals (`"awk '{print $1}'"`), and unformatted string constants.

3. **Ruff `G001`–`G004` (`flake8-logging-format`) & Pylint `W1201`–`W1203` (`logging-not-lazy` / `logging-fstring-interpolation`)** ✅
   - **Why `%s` -> `%r` remains important in modern Python**:
     - Even in Python 3.12+ codebases that use f-strings everywhere else, `logging` calls routinely use lazy `%` formatting (`logger.warning("Invalid token '%s'", token)`) to satisfy `G004` / `W1203`.
     - Supporting `%s` and `%(name)s` on `%` expressions and multi-argument `logging` calls ensures `quote-wrapped-placeholder` works seamlessly alongside `flake8-logging-format` without forcing developers to convert lazy log calls to f-strings.

4. **Ruff `S608` (`hardcoded-sql-expression`, Bandit `B608`)** ✅
   - **What it detects**: Constructing SQL queries via f-strings, `.format()`, or `%` (such as `f"SELECT * FROM users WHERE name = '{name}'"`).
   - **Why `quote-wrapped-placeholder` must exempt SQL strings**:
     - Replacing `'{name}'` with `{name!r}` in SQL does **not** fix SQL injection (`S608`'s fix is parameterized query binding `?` / `%s` passed to `cursor.execute`), and Python's `repr("O'Reilly")` switches outer delimiters to double quotes (`"O'Reilly"`), which ANSI SQL interprets as a column identifier rather than a string literal.

5. **`wemake-python-styleguide` (`WPS305`, `WPS306`, `WPS237`), `flake8-quotes` (`Q000`–`Q004`), and Refurb (`FURB183` / `RUF027`)** ✅
   - `WPS237` checks expression complexity inside `{...}`; `flake8-quotes` checks outer string literal delimiters (`'` vs `"`); `RUF027` checks missing `f` prefixes on strings referencing in-scope variables. None inspects quote-wrapped placeholders.

---

### 1.2 SOTA Comparison Table (`R1`–`R6`)

| ID | Reference | Key Ideas | Adopt / Adapt / Reject | Why |
| :--- | :--- | :--- | :--- | :--- |
| **R1** | **The legacy linter `QuoteWrappedPlaceholderRule`** ([legacy_lints.py:2151-2221](../../../scratch/legacy_reference/legacy_lints.py#L2151-L2221)) ✅ | Flags `'{x}'` / `"{x}"` in f-strings and `.format()` strings (suggesting `{x!r}`) and `'%s'` / `"%s"` in printf strings (suggesting `%r`). | **Adopt** core 3-syntax coverage (f-string, `.format()`, `%`).<br>**Reject** scanning unformatted `ast.Constant` nodes, flagging `!r`/`%r`/`:spec`/`%d`/`%.2f`, and flagging formal-syntax strings (HTML, SQL, JSON, CLI flags). | The legacy linter's unscoped `ast.Constant` regexes and blindness to conversions, format specifiers, and formal delimiters cause severe false positives (§2 of `01_understand.md`). |
| **R2** | **Ruff `RUF010` (`explicit-f-string-type-conversion`)** ✅ | Recommends `!r` conversion flag in f-strings; skips interpolations that already specify a conversion flag or incompatible format spec. | **Adopt** conversion & format-specifier exemption (**C2** / **D7**). | In Python, `!r` converts to `str` *before* `:format_spec`, so adding `!r` to `{x:.2f}` or `{dt:%Y-%m-%d}` raises `ValueError` at runtime. |
| **R3** | **Ruff `F501`–`F525` & Pylint `W1300`–`W1310`** ✅ | Only inspect non-f-string literals when syntactically bound to `.format(...)`, `.format_map(...)`, `%` (`BinOp`), or multi-arg `logging` calls. | **Adopt** syntactic context gating for non-f-strings (**C1** / **D6**). | Eliminates all false positives on docstrings, regexes, shell/awk commands, and plain strings. |
| **R4** | **Ruff `G001`–`G004` (`flake8-logging-format`)** ✅ | Distinguishes `logger.debug/info/warning/error/exception/critical` (msg at arg 0, format args at `1..`) from `logger.log` (level at arg 0, msg at arg 1, format args at `2..`). | **Adopt** `logging` call argument indexing and require $\ge 1$ format argument after `msg`. | In CPython `logging`, `LogRecord.getMessage()` only applies `%` formatting when `self.args` is non-empty. |
| **R5** | **Ruff `S608` (`hardcoded-sql-expression`)** ✅ | Identifies SQL strings by SQL statement/clause keywords (`SELECT`, `INSERT`, `UPDATE`, `DELETE`, `WHERE`, `VALUES`, `LIKE`). | **Adapt** as an exemption in **C4** (**NG3**). | SQL interpolation is a security bug (`S608`), not a `!r` style issue; `{name!r}` produces double-quoted identifiers when `name` contains `'`. |
| **R6** | **Rust Clippy (`uninlined_format_args`, `format_in_format_args`)** ✅ | Analyzes `format_args!` macros with `rustc` type info; never suggests `{x:?}` for `'{x}'`. | **Adopt** Python-only scope (**D2** / **NG1**). | See §1.3 below for why `{x}` $\to$ `{x:?}` is unsound without `rustc` type inference. |

---

### 1.3 Why Rust `format!("invalid '{x}'")` vs. `format!("invalid {x:?}")` Is Excluded (`Q2`)

We audited Omni's own Rust codebase (`src/` and `tests/`) for `'{}'` / `'{x}'` / `\"{x}\"` inside Rust formatting macros and evaluated how Rust's `Display` (`{}`) vs. `Debug` (`{:?}`) compares to Python's `str` vs. `repr` (`!r`):

1. **In Rust, `Debug` (`{:?}`) does NOT quote non-string types, and dumps internal struct layouts**:
   - In Python, `repr()` on `str` adds quotes (`'foo'`), and on `None`, `int`, or `bool` prints `None`, `42`, `True`.
   - In Rust, `{x:?}` only adds quotes if `x` is `str`, `String`, `Path`, `OsStr`, `CStr`, or `char`.
   - If `x` is an `enum` or `struct` that implements `Display`:
     - In [tests/architecture_conformance.rs:629](../../../tests/architecture_conformance.rs#L629):
       `"Component '{component}' has no root source files declaring it in src/"`
       Here `component` is an `ArchitectureComponent` enum (`#[derive(Debug, strum::Display)]`). Under `Display` (`'{component}'`), it formats as `'CodeLintAst'`. Under `Debug` (`{component:?}`), `#[derive(Debug)]` formats enum variants **without quotes** (`Component CodeLintAst has no root...`)!
     - For standard library or ecosystem types like `url::Url`, `semver::Version`, `uuid::Uuid`, or `std::io::Error`, `'{x}'` formats the human-readable `Display` string in quotes (`'1.2.3'`), whereas `{x:?}` dumps raw internal struct fields (`Version { major: 1, minor: 2, patch: 3, pre: Prerelease(""), build: BuildMetadata("") }`).
   - If the argument is `path.display()` (as in [src/code_lint/runner.rs:284](../../../src/code_lint/runner.rs#L284): `anyhow::anyhow!("Failed to read file '{}': {}", path.display(), error)`), `.display()` is an adapter explicitly constructed for `{}`.
   - If a type implements `Display` but not `Debug`, changing `{x}` to `{x:?}` is a compile error (`E0277`).
2. **Without `rustc`'s `TyCtxt`, a Tree-sitter CST rule cannot know the type of `x`**:
   - Because Omni analyzes syntax trees without a cross-crate Rust type solver, it cannot distinguish `x: &str` from `x: ArchitectureComponent` or `x: semver::Version`.
3. **Self-dogfooding audit on Omni (`src/` and `tests/`)**:
   - Omni has **0** Python files in `src/` and `tests/`, and **15** Rust occurrences of quote-wrapped format placeholders across 6 files ([src/test_utils.rs](../../../src/test_utils.rs), [src/code_lint/runner.rs](../../../src/code_lint/runner.rs), [src/diff.rs](../../../src/diff.rs), [src/rule_selection.rs](../../../src/rule_selection.rs), [src/rule_declaration/options.rs](../../../src/rule_declaration/options.rs), [tests/registry.rs](../../../tests/registry.rs), [tests/architecture_conformance.rs](../../../tests/architecture_conformance.rs)).
   - Keeping `quote-wrapped-placeholder` scoped to `SupportLang::Python` (**D2**, **NG1**) is 100% sound and passes `test_self_dogfooding_code_lint` with zero false positives.

---

## 2. Internal Codebase Architecture & Tree-Sitter Analysis (`Q1`, `Q3`, `Q4`)

### 2.1 Taxonomy Topic (`Q1`)

In [src/rule_declaration/taxonomy.rs:177-185](../../../src/rule_declaration/taxonomy.rs#L177-L185):
```rust
    /// How literal values are written in code.
    pub(crate) const LITERALS: Self = Self {
        label: "literals",
        parent: None,
        description: "How literal values are written in code.",
        scope_note: "Writing string and number literals (multiline strings, magic numbers). Not \
                     identifiers or formatting APIs.",
        synonyms: &[],
    };
```
We have two clean options for `Classification::topics`:
- **Option A (Recommended — Zero changes outside `quote_wrapped_placeholder.rs` and its registration)**: Use `topics: &[Topic::LITERALS]` (currently used by `bare-multiline-string` and `repeated-literal`), since `quote-wrapped-placeholder` inspects how string literals and f-string literals are written.
- **Option B**: Add a new `Topic::STRING_FORMATTING` (`label: "string-formatting"`) to `taxonomy.rs` and `docs/dev/tag_guide.md` §5 (which could also house the legacy linter's `LoggerPositionalPlaceholderRule` if ported).

### 2.2 Tree-Sitter Python CST Structure (`Q3`)

1. **`string` Node Structure in `tree-sitter-python`**:
   - Every Python string literal (plain, raw, byte, or f-string) is a `string` node whose first child is `string_start` (`"`, `'`, `"""`, `f"`, `rf"`, `b"`, etc.) and whose last child is `string_end`.
   - Prefix inspection on `string_start.text()`:
     - Let `prefix` be the alphabetic characters before the opening quote character (`'` or `"`).
     - **Skip raw strings**: `prefix.contains(['r', 'R'])` (regexes, Windows paths, LaTeX, AST patterns where `repr()`'s backslash doubling breaks semantics).
     - **Skip byte strings**: `prefix.contains(['b', 'B'])` (`b"%r"` formats as `b"b'...'"`, which is not a drop-in replacement for `b"'%s'"`).
     - **F-string**: `prefix.contains(['f', 'F'])`.
2. **F-String `interpolation` Children**:
   - Inside an f-string `string` node, direct children with `child.kind() == "interpolation"` represent `{...}` expressions in source order.
   - Using the byte ranges of `string_start`, each direct `interpolation` child, and `string_end`, we can slice `file.source_text()` directly into alternating literal text slices `segments[0..=N]` and `interpolations[0..N]`:
     - `segments[0] = &source[string_start.end .. interpolations[0].start]`
     - `segments[i] = &source[interpolations[i-1].end .. interpolations[i].start]`
     - `segments[N] = &source[interpolations[N-1].end .. string_end.start]`
   - Why slicing between `interpolation` byte ranges is superior to inspecting `string_content` child nodes:
     - In `tree-sitter-python`, escape sequences (`\"`, `\'`, `\n`) and escaped braces (`{{`, `}}`) split `string_content` into multiple sibling CST nodes (`string_content`, `escape_sequence`, `escape_interpolation`).
     - Slicing `source` between adjacent `interpolation` ranges gives the complete, contiguous literal segment between `{...}` placeholders in one step, and handles both unescaped quotes (`f"Invalid '{x}'"`) and escaped quotes (`f"Invalid \"{x}\""`) uniformly!
   - Checking **C2** on an `interpolation` node:
     - `interp.field("type_conversion").is_none()` (no `!r`, `!s`, `!a`),
     - `interp.field("format_specifier").is_none()` (no `:...`),
     - `!interp.children().any(|c| c.kind() == "=")` (no debug `f"{x=}"`),
     - `let Some(expr) = interp.field("expression")`, with `expr.kind() != "string"` (skip nested f-strings like `f"'{f'{x}'}'"`).
     - Computed replacement: `format!("{{{expr_text}!r}}")`, where `expr_text = expr.text().trim()`.
3. **Syntactic Context Gating for Non-F-Strings (`C1`)**:
   - When a `string` node has no `f`/`F`, `r`/`R`, or `b`/`B` prefix:
     - **`.format()` / `.format_map()` context**:
       - Walk up through any `parenthesized_expression` ancestors.
       - Match if the parent is an `attribute` node (`object == string`, `attribute == "format" | "format_map"`) whose parent is a `call` node (`function == attribute`), **or** if the string is the first positional argument of `str.format(...)`.
     - **Printf `%` context**:
       - Walk up through any `parenthesized_expression` ancestors.
       - Match if:
         1. The parent is a `binary_operator` with `operator == "%"` and `left == string`, **or**
         2. The parent is `argument_list` of a `call` whose `function` is an `attribute` on a logger-like receiver (`logging`, `logger`, `log`, `_logger`, `_log`, `self.logger`, `self.log`, `cls.logger`, `cls.log`) calling:
            - `debug` | `info` | `warning` | `warn` | `error` | `exception` | `critical` | `fatal` where the string is positional argument `0` and positional argument `1` exists, **or**
            - `log` where the string is positional argument `1` and positional argument `2` exists.
4. **Diagnostic Span Choice (`Q3`)**:
   - Anchoring the diagnostic on the **`string` literal `AstNode`** across all three styles (f-strings, `.format()`, and `%`):
     - Keeps the span consistent across f-strings, `.format()` calls, and `%` expressions (since `.format()` and `%` placeholders live inside raw string text and have no separate `interpolation` CST node).
     - Works cleanly with `rule_test!` fail cases (both standalone f-strings and sub-expression `=> r#"..."#` slices).
     - If a single string contains multiple quote-wrapped placeholders (such as `f"Copied '{src}' to '{dst}'"`), emitting one diagnostic per placeholder (or one per string on the first offending placeholder — see §3 **R-D9**) must be decided carefully for `rule_test!`:
       - In `rule_test!`, each `fail` case asserts **exactly 1 diagnostic** (`let [diagnostic] = diags.as_slice()`). Any single-placeholder fail case (`f"Invalid value '{x}'"`) emits 1 diagnostic under both schemes, and multi-placeholder strings can be tested in unit tests on the AST collector!

---

### 2.3 Edge Cases in Prose vs. Structured Syntax Detection (`Q4`)

Let's trace how **C3** (quote matching) and **C4** (prose boundary validation) handle every edge case:

| Case | Code | Desired Outcome | How C1–C4 Handle It |
| :--- | :--- | :--- | :--- |
| **1. Standard f-string** | `f"Invalid value '{x}'"` | **Flag** (`'{x}'` $\to$ `{x!r}`) | C1 (f-string), C2 (bare `{x}`), C3 (`'...'`), C4 (prose word `"Invalid"`, space before `'`, end-of-string after `'`). |
| **2. Escaped quotes in f-string** | `f"Invalid value \"{x}\""` | **Flag** (`\"{x}\"` $\to$ `{x!r}`) | C3 recognizes trailing `\"` before `{x}` and leading `\"` after `{x}` as a matching double-quote pair. |
| **3. `.format()` named / positional / empty** | `"Invalid '{x}'".format(x=v)`<br>`"Invalid '{}'".format(v)`<br>`"Invalid '{0}'".format(v)` | **Flag** (`{x!r}`, `{!r}`, `{0!r}`) | C1 (`.format()` receiver), C2 (no `!` or `:` inside `{...}`, not escaped `{{...}}`), C3, C4. |
| **4. Printf `%s` and `%(name)s`** | `"Invalid '%s'" % v`<br>`"Invalid '%(key)s'" % d`<br>`logger.error("Bad '%s'", v)` | **Flag** (`%r`, `%(key)r`) | C1 (`%` left operand or multi-arg logger call), C2 (bare `%s` or `%(key)s`, not `%%s`), C3, C4. |
| **5. Plain unformatted string / docstring** | `"""Use '{x}' or '%s'."""`<br>`s = "Invalid '{x}'"` | **Pass** | Excluded by **C1** (not an f-string and not in `.format()`, `%`, or multi-arg logger call). |
| **6. Zero-arg logger call** | `logger.info("Found '%s' literal")` | **Pass** | Excluded by **C1** (no trailing format arguments after the message string). |
| **7. Raw or byte string** | `rf"pattern '{x}'"`<br>`r"regex '{x}'".format(x=p)`<br>`b"Invalid '%s'" % b` | **Pass** | Excluded by **C1** (`r`/`R` and `b`/`B` prefixes skipped). |
| **8. Already has `!r` / `!s` / `!a` / `=` / `%r`** | `f"Invalid '{x!r}'"`<br>`f"Invalid '{x=}'"`<br>`"Invalid '{x!r}'".format(x=v)`<br>`"Invalid '%r'" % v` | **Pass** | Excluded by **C2** (existing conversion flag or `%r`). |
| **9. Has format specifier or numeric `%`** | `f"Bound '{x:.2f}'"`<br>`f"Date '{dt:%Y-%m-%d}'"`<br>`"Count '%d'" % n`<br>`"Trunc '%.5s'" % s` | **Pass** | Excluded by **C2** (`format_specifier` present, or printf specifier is not bare `%s` / `%(name)s`). |
| **10. Escaped braces in `.format()` / f-string** | `"Literal '{{x}}'".format()`<br>`f"Literal '{{'{x}'}}'"` | **Pass** | Excluded by **C2** / **C4** (`{{` and `}}` are literal braces, not format fields or prose boundaries). |
| **11. HTML / XML attribute** | `f'<a href="{url}">Click</a>'`<br>`f"<div class='{cls}'>"` | **Pass** | Excluded by **C4** (`=` immediately precedes opening quote; `>` follows closing quote). |
| **12. CLI flag / `key="val"` config** | `f'--output="{path}"'`<br>`f'mode = "{mode}"'` | **Pass** | Excluded by **C4** (last non-whitespace char before opening quote is `=`). |
| **13. SQL query** | `f"SELECT * FROM t WHERE k = '{v}'"`<br>`f"INSERT INTO t VALUES ('{v}')"`<br>`f"SELECT * FROM t WHERE k LIKE '{v}'"` | **Pass** | Excluded by **C4** (starts with SQL statement keyword `SELECT`/`INSERT` or preceded by `=` / `VALUES` / `LIKE`). |
| **14. JSON / dict / list syntax** | `f'{{"key": "{val}"}}'`<br>`f'["{item}"]'` | **Pass** | Excluded by **C4** (preceded by `{` / `[` / `:` inside braces, or no prose word outside). |
| **15. Isolated quote-wrapping** | `f'"{x}"'`<br>`f"'{x}'"` | **Pass** | Excluded by **C4** (zero prose words outside the placeholder). |
| **16. Inside markdown backticks** | ``f"Set `key = '{val}'` in file"``<br>``f"Expected `'{x}'` token"`` | **Pass** | Excluded by **C4** (odd number of backticks `` ` `` before opening quote). |
| **17. File extension / path / URL concatenation** | `f"File '{stem}'.py"`<br>`f"Host '{host}':{port}"` | **Pass** | Excluded by **C4** (closing quote is followed by `.py` or `:{port}` without whitespace). |
| **18. Mismatched quotes** | `f"Invalid '{x}\""` | **Pass** | Excluded by **C3** (opening and closing quotes do not match). |

---

## 3. Proposed Refinements to `01_understand.md` Before Phase 3

| ID | Refinement | Rationale |
| :--- | :--- | :--- |
| **R-D5** | Use `topics: &[Topic::LITERALS]` in `Classification`. | `Topic::LITERALS` already exists in `taxonomy.rs` and covers string literal rules (`bare-multiline-string`, `repeated-literal`) without requiring edits outside the rule and its registration. |
| **R-D6** | Support escaped matching quotes (`\"{x}\"` and `\'{x}\'`) in addition to unescaped quotes (`"{x}"` and `'{x}'`), and support concatenated string literals (`concatenated_string` in `.format()` / `%` / `logging` contexts). | Developers writing double-quoted f-strings (`f"Invalid value \"{x}\""`) often escape `\"` rather than switching outer quotes; the legacy linter's `ast.JoinedStr` caught this because Python's `ast` unescapes constants. |
| **R-D9** | Anchor diagnostics on the `string` `AstNode`, and emit one diagnostic per quote-wrapped placeholder in source order with `("expression", ...)` and `("replacement", ...)`. | Matches `CodeRule::diagnostic_at_node` and `rule_test!` expectations while reporting every offending placeholder in a file. |

---

## 4. Sources
- PEP 3101 — Advanced String Formatting (`!r`, `!s` conversion flags): https://peps.python.org/pep-3101/
- PEP 498 — Literal String Interpolation (f-strings): https://peps.python.org/pep-0498/
- Ruff `RUF010` (`explicit-f-string-type-conversion`): https://docs.astral.sh/ruff/rules/explicit-f-string-type-conversion/
- Ruff `F501`–`F525` (Pyflakes format rules): https://docs.astral.sh/ruff/rules/#pyflakes-f
- Ruff `G001`–`G004` (`flake8-logging-format`): https://docs.astral.sh/ruff/rules/#flake8-logging-format-g
- Ruff `S608` (`hardcoded-sql-expression`): https://docs.astral.sh/ruff/rules/hardcoded-sql-expression/
- Pylint `W1300`–`W1310` & `C0209` (`stdlib` string formatting checker): https://pylint.readthedocs.io/en/stable/user_guide/messages/warning/bad-format-string.html
