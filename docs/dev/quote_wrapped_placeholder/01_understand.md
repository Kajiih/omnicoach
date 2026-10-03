# Phase 1: Understand — `quote-wrapped-placeholder`

This document records **Phase 1 (Understand)** for porting the legacy linter's `QuoteWrappedPlaceholderRule` ([legacy_lints.py:2151-2221](../../../scratch/legacy_reference/legacy_lints.py#L2151-L2221)) to Omni as `quote-wrapped-placeholder`.

> Status: **VALIDATED** (2026-10-03). All decisions `D1`–`D9` and open questions `Q1`–`Q4` validated.

---

## 1. Problem Statement & What `QuoteWrappedPlaceholderRule` Detects

In diagnostic, logging, and error messages, developers frequently wrap interpolated values in manual single or double quotes (`f"Invalid value '{x}'"`, `"Invalid value '{x}'".format(x=x)`, `"Invalid value '%s'" % x`) so the value stands out in human-readable output.

Manual quote wrapping around a string-formatted (`str`) placeholder has four concrete failure modes:
1. **Unescaped embedded quotes**: If `x` is a string containing the outer quote character (such as `x = "can't"` in `f"Invalid value '{x}'"`), manual quotes produce broken delimiter boundaries (`Invalid value 'can't'`). Representation formatting (`!r` / `%r`) automatically switches quote delimiters or escapes inner quotes (`Invalid value "can't"`).
2. **Unescaped control characters and invisible whitespace**: If `x` contains newlines (`\n`), carriage returns (`\r`), tabs (`\t`), or NUL bytes (`\x00`), default string formatting emits raw control characters that split log lines or hide non-printable bytes. `repr()` escapes them (`'line1\\nline2'`, `'\\x00'`).
3. **Type erasure (`None`, booleans, and numbers look like strings)**: Manual quotes format `None` as `'None'`, `42` as `'42'`, and `True` as `'True'`, making them indistinguishable from the strings `"None"`, `"42"`, and `"True"` during debugging. Representation formatting (`{x!r}` / `%r`) preserves runtime types (`None` vs. `'None'`, `42` vs. `'42'`).
4. **Double-quoting**: If `x` is already a `repr`-formatted string, manual quotes produce `''foo''` or `'"foo"'`.

### 1.1 What the legacy linter's `QuoteWrappedPlaceholderRule` Detects

The legacy linter ([legacy_lints.py:2151-2221](../../../scratch/legacy_reference/legacy_lints.py#L2151-L2221)) inspects `ast.Constant` and `ast.JoinedStr` nodes for three patterns:

| # | Pattern | The legacy linter Detection Mechanism | The legacy linter Suggested Fix |
| :--- | :--- | :--- | :--- |
| **1** | **`.format()`-style placeholders in constant strings**:<br>`"Invalid value '{x}'"` or `'Invalid value "{}"'` | Runs `_QUOTES_RE = re.compile(r"'(?:\{[^{}]*\})'\|\"(?:\{[^{}]*\})\"")` on **every** `ast.Constant` string in the file. | ``Use `{...!r}` representation formatting instead of quotes.`` |
| **2** | **Printf-style (`%`) placeholders in constant strings**:<br>`"Invalid value '%s'"` or `'Invalid value "%s"'` | Runs `_PRINTF_QUOTES_RE = re.compile(r"'(%[-+0 #]*(?:\d+\|\*)?(?:\.(?:\d+\|\*))?[diouxXeEfFgGcrs])'\|...")` on **every** `ast.Constant` string in the file. | ``Use `%r` representation formatting instead of quotes.`` |
| **3** | **F-strings (`ast.JoinedStr`)**:<br>`f"Invalid value '{x}'"` or `f'Invalid value "{x}"'` | Checks each `ast.FormattedValue` at index `i`: if `values[i - 1]` is an `ast.Constant` ending with `'` or `"` and `values[i + 1]` is an `ast.Constant` starting with the **same** quote character. | ``Use `{x!r}` instead.`` |

---

## 2. False Positives & Structural Defects in the legacy linter's Implementation

A close inspection of [legacy_lints.py:2178-2221](../../../scratch/legacy_reference/legacy_lints.py#L2178-L2221) reveals four major classes of false positives and bugs:

### 2.1 Defect A: `_check_constant_string` Runs on Every `ast.Constant` String in the File
Because `node_types = (ast.Constant, ast.JoinedStr)` visits every string literal in the module regardless of whether it is ever formatted:
- **Plain unformatted strings & docstrings**: A docstring explaining template syntax (`"""Format string using '{name}' or '%s'."""`) or an error message mentioning literal syntax (`"Expected '{id}' placeholder"`) is flagged even though `.format()` or `%` is never called on it.
- **Shell, `awk`, `jq`, and regex literals**: `"awk '{print $1}'"` matches `_QUOTES_RE` (`'{print $1}'`) and is flagged as a quote-wrapped Python placeholder.
- **Escaped braces in `.format()` strings**: `"Literal '{{x}}'".format()` contains `'{x}'` as a substring inside `'{{x}}'` (which renders as literal `'{x}'` at runtime, not an interpolated field), yet `_QUOTES_RE` flags it.
- **F-string constant children visited twice**: In Python's `ast`, `ast.JoinedStr.values` holds `ast.Constant` nodes for the literal text segments. If an f-string contains literal `'%s'` or `'{foo}'` inside a raw/escaped segment, `_check_constant_string` fires on the f-string's child `ast.Constant`.

### 2.2 Defect B: Blindness to Existing Conversions (`!r`, `!s`, `!a`, `%r`), Debug `=`, and Format Specifiers (`:spec`, `%.2f`, `%d`)
- **Already using `!r` or `%r`**:
  - In `_check_joined_str`, the legacy linter never inspects `FormattedValue.conversion` or `FormattedValue.format_spec`. On `f"'{x!r}'"`, `ast.unparse(val.value)` strips `!r` and emits: ``F-string placeholder `x` is wrapped in quotes `'{x}'`. Use `{x!r}` instead.``
  - In `_QUOTES_RE`, `r"'(?:\{[^{}]*\})'"` matches `"{x!r}"` inside `'"{x!r}"'`, telling the developer to use `{...!r}` when `!r` is already present.
  - In `_PRINTF_QUOTES_RE`, the character class `[diouxXeEfFgGcrs]` includes **`r`**, so `"'%r'" % x` is flagged with: ``Printf-style placeholder wrapped in quotes `'%r'`. Use `%r` representation formatting instead of quotes.``
- **Incompatible format specifiers (`f"'{x:.2f}'"`, `f"'{dt:%Y-%m-%d}'"`, `"'%.2f'" % x`, `"'%04d'" % n`)**:
  - In Python, conversion flags (`!r`) run **before** format specifiers (`:spec`), converting the value to `str` via `repr(x)` before calling `__format__`.
  - Consequently, replacing `f"'{x:.2f}'"` with `f"{x!r:.2f}"` crashes at runtime with `ValueError: Unknown format code 'f' for object of type 'str'`, and replacing `f"'{dt:%Y-%m-%d}'"` with `f"{dt!r:%Y-%m-%d}"` crashes with `ValueError: Invalid format specifier`!
  - Even for string alignment (`f"'{s:>10}'"`), `f"{s!r:>10}"` changes the runtime output by shifting the padding outside the quotes.
  - Similarly, in printf formatting, replacing `'%.2f'` or `'%04d'` with `%r` discards numeric rounding/padding entirely. And the legacy linter's `_PRINTF_QUOTES_RE` omits mapping keys `(name)` (`'%(name)s'`), missing named printf placeholders while flagging numeric ones.

### 2.3 Defect C: Formal-Syntax & Structured Strings (HTML/XML, SQL, JSON/TOML, CLI Flags, Code Spans)
The legacy linter checks only the single character before and after `{x}`, ignoring whether the quotes are syntactic delimiters of a formal language rather than human-readable prose:
- **HTML / XML attributes (`f'<a href="{url}">'`, `f"<div class='{cls}'>"`)**: HTML attributes require literal `"` or `'` after `=`. Using `{url!r}` emits single-quoted Python `repr` syntax (or switches to `"` when `url` contains `'`), producing malformed HTML.
- **SQL queries (`f"SELECT * FROM t WHERE name = '{name}'"` or `VALUES ('{name}')`)**:
  - While interpolating into SQL is a security vulnerability covered by Ruff `S608` (`hardcoded-sql-expression`), recommending `{name!r}` does **not** prevent SQL injection and actively corrupts valid strings containing an apostrophe (`repr("O'Reilly")` produces `"O'Reilly"`, which ANSI SQL parses as a double-quoted column identifier rather than a string literal).
- **JSON / TOML / YAML / Config fragments (`f'{{"key": "{val}"}}'`, `f'mode = "{mode}"'`, `f'["{item}"]'`)**: JSON and TOML require double quotes `"..."`; Python's `{val!r}` outputs single quotes `'...'`, producing invalid JSON/TOML.
- **CLI flags and `key=value` arguments (`f'--output="{path}"'`)**: Quotes after `=` are shell/argument syntax, not prose quoting.
- **Markdown / code backtick spans (``f"Set `name = '{name}'` in config"``)**: When a placeholder sits inside backticks `` `...` ``, the quotes are part of the rendered code example.
- **Isolated quote wrapping (`f'"{x}"'` or `f"'{x}'"` with no surrounding text)**: Writing `f'"{x}"'` with zero prose words is an intentional delimiter-wrapping expression (such as quoting a CSV cell or TOML string), not a prose message.

---

## 3. Rule Principle & High-Precision Design in Omni

> **In a human-readable format message, a bare string placeholder must not be wrapped in manual single or double quotes (`'{x}'`, `"{x}"`, `'%s'`, `"%s"`); use representation formatting (`{x!r}`, `%r`) or backticks (`` `{x}` ``) instead.**

A placeholder is flagged **if and only if** all four conditions hold:

| # | Condition | How it eliminates false positives |
| :--- | :--- | :--- |
| **C1** | **Active formatting context**:<br>- **F-string**: `string` literal with `f`/`F` prefix (and **no** raw `r`/`R` or byte `b`/`B` prefix).<br>- **`.format()` string**: `string` literal (non-raw, non-bytes) that is the direct receiver of `.format(...)` or `.format_map(...)` (or first argument of `str.format(...)`).<br>- **Printf `%` string**: `string` literal (non-raw, non-bytes) that is the left operand of `%` (`binary_operator`), or the message argument of a `logging` / `logger` call (`debug`, `info`, `warning`, `warn`, `error`, `exception`, `critical`, `fatal`, `log`) that passes **at least one** format argument after the message string. | Eliminates 100% of false positives on plain unformatted strings, docstrings, regexes, `"awk '{print $1}'"`, byte strings (`b"%r"` prefixes `b'...'`), and zero-argument `logger.info("...")` calls. |
| **C2** | **Bare string conversion (no existing conversion or format spec)**:<br>- **F-string `interpolation`**: has **no** `type_conversion` (`!r`, `!s`, `!a`), **no** `format_specifier` (`:spec`), and **no** debug `=` token.<br>- **`.format()` placeholder**: unescaped `{field}` where `field` contains no `!`, `:`, `{`, or `}`.<br>- **Printf `%` placeholder**: unescaped `%s` or `%(name)s` (with no flags, width, or precision, and not `%%s` or `%r`/`%d`/`%f`). | Eliminates false positives on `f"'{x!r}'"`, `f"'{x:.2f}'"`, `f"'{dt:%Y-%m-%d}'"`, `f"'{x=}'"`, `"'%r'"`, `"'%.2f'"`, and escaped braces `'{{x}}'`. |
| **C3** | **Matching quote pair (`'...'` or `"..."`)**:<br>The placeholder is immediately preceded by `'` or `"` (`quote_char`) and immediately followed by the **same** `quote_char`. | Matches `'{x}'`, `"{x}"`, `'%s'`, `"%s"`, `'%(k)s'`, `"%(k)s"`. |
| **C4** | **Prose context (not HTML/SQL/JSON/config/code syntax)**:<br>1. **Surrounding prose word**: The format string's literal text (outside placeholders) contains at least one alphabetic word ($\ge 2$ ASCII letters) and does not begin with a SQL statement keyword (`SELECT`, `INSERT`, `UPDATE`, `DELETE`, `CREATE`, `ALTER`, `DROP`, `WITH`, `REPLACE`, `PRAGMA`).<br>2. **Left prose boundary**: Immediately before the opening quote is start-of-string, whitespace, or `(`; the last non-whitespace token before the opening quote is **not** `=`, `{`, `[`, `<`, `/`, `\`, or a SQL operator keyword (`VALUES`, `LIKE`, `ILIKE`, `RLIKE`, `REGEXP`, `GLOB`, `IN`).<br>3. **Right prose boundary**: Immediately after the closing quote is end-of-string, whitespace, or sentence punctuation (`.`, `,`, `;`, `:`, `!`, `?`, `)`) that is at end-of-string or followed by whitespace/`'`/`"`.<br>4. **Not inside backticks**: The literal text preceding the opening quote has an even number of `` ` `` characters. | Eliminates false positives on HTML attributes (`href="{url}"`), SQL (`WHERE name = '{name}'`, `VALUES ('{x}')`, `LIKE '{x}'`), JSON/TOML (`{"{k}": "{v}"}`, `key = "{v}"`, `["{x}"]`), CLI flags (`--out="{p}"`), file extensions (`"{stem}".py`), isolated quoting (`f'"{x}"'`), and markdown code spans (`` `` `'{x}'` `` ``). |

---

## 4. Goals & Explicit Non-Goals

### 4.1 Goals

| ID | Goal | Reason |
| :--- | :--- | :--- |
| **G1** | Detect quote-wrapped placeholders across Python f-strings, `.format()` / `.format_map()` calls, `%`-formatted strings, and multi-argument `logging` calls. | Unified antipattern and rationale across all three Python string-formatting mechanisms ([Rule Design Guide §1](../rule_design_guide.md)). |
| **G2** | Eliminate the legacy linter's false positives on unformatted strings, docstrings, regexes, format specifiers, HTML/XML, SQL, JSON/TOML, CLI flags, and code spans via **C1–C4**. | High signal-to-noise ratio in CI without forcing suppressions on valid code. |
| **G3** | Provide an exact, per-finding `{replacement}` in the diagnostic suggestion (`{x!r}`, `{!r}`, `%r`, `%(name)r`) while noting backticks (`` `{x}` ``) as the alternative for code identifiers. | Actionable pit-of-success guidance ([Rule Design Guide §2](../rule_design_guide.md), [Naming and Message Style Guide §2.4](../naming_and_message_style_guide.md)). |
| **G4** | Keep all Tree-sitter CST traversal inside `code_lint::ast::python`, exposing a clean helper to `src/code_lint/rules/quote_wrapped_placeholder.rs`. | Conforms to `tests/architecture_conformance.rs` and [Rule Design Guide §6](../rule_design_guide.md). |

### 4.2 Explicit Non-Goals

| ID | Non-Goal | Reason |
| :--- | :--- | :--- |
| **NG1** | Rust `format!("invalid '{x}'")` → `{x:?}` conversion (see §5 **D2** and `02_references.md` §1.3). | In Rust, `{}` (`Display`) and `{:?}` (`Debug`) are distinct traits: on non-string `Display` types (`enum`, `struct`, numbers, `Url`, `Version`), `{x:?}` either omits quotes entirely or dumps internal struct fields. Without `rustc` type inference (`TyCtxt`), CST analysis cannot know if `x` is a string. |
| **NG2** | Cross-statement dataflow tracking of format template variables (`tpl = "Invalid '{x}'"; tpl.format(x=1)`). | Inspecting strings that are not syntactically bound to a formatting operation reintroduces the legacy linter's Defect A on SQL/shell/HTML template constants. |
| **NG3** | Detecting SQL injection in f-strings (`f"SELECT ... WHERE x = '{x}'"`). | Dedicated security antipattern covered by Ruff `S608` (`hardcoded-sql-expression`), where the fix is parameterized queries (`cursor.execute("... WHERE x = ?", (x,))`), not `{x!r}`. |
| **NG4** | Converting `%` or `.format()` calls into f-strings. | Covered by Ruff `UP031` (`printf-string-formatting`), `UP032` (`f-string`), and Pylint `C0209`. |

---

## 5. Numbered Decisions (`D1`–`D9`)

| ID | Decision | Rationale |
| :--- | :--- | :--- |
| **D1** | **Rule name**: `quote-wrapped-placeholder` (file `src/code_lint/rules/quote_wrapped_placeholder.rs`, `pub const RULE`). | Follows [Naming and Message Style Guide §1](../naming_and_message_style_guide.md): 3 words, kebab-case, singular noun phrase naming the flagged construct, no polarity prefix. |
| **D2** | **Language & target scope**: `languages: &[SupportLang::Python]`, `target: RuleTarget::All`. | Python's `!r` / `%r` calls `object.__repr__` (present on every Python object, quoting strings and preserving non-string types). Applies to both source and test files (assertion messages in tests benefit equally). See `02_references.md` §1.3 for full Rust `Display` vs. `Debug` analysis. |
| **D3** | **Single unified rule across f-strings, `.format()`, and `%` formatting**: Parameterize the diagnostic with `{expression}` (e.g. `'{x}'`, `'%s'`) and `{replacement}` (e.g. `{x!r}`, `%r`, `%(name)r`). | Passes the Split Test ([Rule Design Guide §1](../rule_design_guide.md)): why it is harmful and how to fix it (use representation formatting `{replacement}` or backticks) are identical across all three syntaxes. |
| **D4** | **Options & enforcement mode**: `RuleOptions::code_rule(())` (`EnforcementMode::Ban` by default, configurable to `require-explanation`, no numeric or list options). | No arbitrary numeric threshold or call list is needed; teams with an unusual domain string can use `# omni:ignore [quote-wrapped-placeholder] -- reason` or `enforcement-mode = "require-explanation"`. |
| **D5** | **Classification**:<br>- `topics: &[Topic::LITERALS]` (or a new `Topic::STRING_FORMATTING` — see **Q1**)<br>- `precision: Precision::Heuristic`<br>- `consensus: Consensus::Opinionated`<br>- `impacted_quality: ImpactedQuality::Maintainability` | - `Precision::Heuristic`: **C4** uses lexical context around the quotes to distinguish human-readable prose from formal/markup syntax ([Tag Guide §2.2](../tag_guide.md)).<br>- `Consensus::Opinionated`: In user-facing messages where a `str` value is guaranteed not to contain quotes or control characters, some developers intentionally write `'{name}'` to force single quotes even when `name` contains an apostrophe ([Tag Guide §2.3](../tag_guide.md)).<br>- `ImpactedQuality::Maintainability`: Improves diagnostic clarity and log/error message readability rather than fixing a direct runtime crash ([Tag Guide §2.4](../tag_guide.md)). |
| **D6** | **Syntactic gating for non-f-strings (`C1`)**: Only inspect non-f-string literals when they are (a) the receiver of `.format(...)` / `.format_map(...)` or first arg of `str.format(...)`, (b) the left operand of `%`, or (c) the message argument of a `logging` / `logger` call (`debug`, `info`, `warning`, `warn`, `error`, `exception`, `critical`, `fatal`, or 2nd arg of `log`) that has $\ge 1$ trailing format argument. | Prevents false positives on docstrings, regexes, shell/awk strings, and unformatted constants. |
| **D7** | **Conversion & format-specifier exemption (`C2`)**: Only flag bare placeholders with no `!` conversion (`!r`, `!s`, `!a`), no `:format_spec`, and no `=` debug specifier; for printf `%`, only flag bare `%s` and `%(name)s`. | Prevents runtime `ValueError` crashes on `f"'{x:.2f}'"` / `f"'{dt:%Y-%m-%d}'"` and redundant warnings on `f"'{x!r}'"` / `"'%r'"`. |
| **D8** | **Prose boundary check (`C4`)**: Require at least one prose word outside placeholders, require prose boundaries before the opening quote and after the closing quote, skip strings inside backticks, and skip SQL / HTML / JSON / `key="val"` assignments. | Eliminates false positives on `<a href="{url}">`, `SELECT ... WHERE x = '{x}'`, `{"{k}": "{v}"}`, `--flag="{v}"`, and `f'"{x}"'`. |
| **D9** | **Diagnostic span & template**:<br>- Anchor each diagnostic on the `string` literal `AstNode` (or `interpolation` node for f-strings — see **Q2**), reporting `{expression}` (e.g. `'{x}'`, `'%s'`) and `{replacement}` (e.g. `{x!r}`, `%r`).<br>- Template:<br>  - `summary`: ``Format placeholder `{expression}` is wrapped in literal quotes.``<br>  - `rationale`: ``Manual quotes around a formatted value do not escape embedded quotes or control characters and make non-string values such as `None` or numbers indistinguishable from strings.``<br>  - `suggestion`: ``Replace the quoted placeholder with `{replacement}`, or wrap it in backticks when formatting a code identifier.`` | Complies with every rule in [Naming and Message Style Guide §2–3](../naming_and_message_style_guide.md) and `tests/registry.rs` (`SUGGESTION_VERBS` includes `"Replace"`, `PLACEHOLDERS` includes `"expression"` and `"replacement"`, zero un-backticked quotes in prose). |

---

## 6. Open Questions for Phase 2 (`Q1`–`Q4`)

- **Q1 (SOTA Linter Coverage & Taxonomy Topic)**:
  - How do Ruff, Pylint, Flake8 plugins, `wemake-python-styleguide`, Refurb, and Rust Clippy handle quote-wrapped placeholders, `!r` / `%r` conversions, and format string context detection?
  - Should `quote-wrapped-placeholder` use existing `Topic::LITERALS` (and update its `scope_note` if needed) or introduce a dedicated `Topic::STRING_FORMATTING` topic?
- **Q2 (Rust `format!("invalid '{x}'")` vs. `format!("invalid {x:?}")` Deep-Dive)**:
  - What happens on Omni's own Rust codebase (`src/` and `tests/`) if Rust format macros are checked, and why does `Display` (`{}`) vs. `Debug` (`{:?}`) behave differently from Python's `str` vs. `repr` (`!r`)?
- **Q3 (Tree-Sitter Python CST Representation & Span Choice)**:
  - How does `tree-sitter-python` represent f-strings (`string`, `string_content`, `escape_sequence`, `interpolation`, `type_conversion`, `format_specifier`), concatenated strings (`concatenated_string`), and `.format()` / `%` / `logging` calls?
  - Should the diagnostic span be anchored on the enclosing `string` node across all three formatting styles, or on `interpolation` for f-strings and `string` for `.format()` / `%`?
- **Q4 (Edge Cases in Prose vs. Structured Syntax Detection)**:
  - How do **C1–C4** behave on edge cases such as implicit string concatenation (`"Invalid " "'%s'" % x`), multiline f-strings, escaped quotes (`f"Invalid \"{x}\""`), nested f-strings (`f"{f'{x}'}"`), and SQL / HTML / JSON / shell snippets?
