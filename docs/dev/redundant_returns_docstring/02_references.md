# 02 — References: docstring `Returns:` / `Yields:` sections that start with `Returns`

> [!NOTE]
> Status: **VALIDATED** (lead, 2026-10-12; implemented). Batch D (docstrings), worker phase 2 of 7. Previous: [01_understand.md](01_understand.md).

## 1. Style guides

### Google Python Style Guide §3.8.3 (`scratch/pyguide.md`)

| Line | Quote | Bearing |
| :--- | :--- | :--- |
| 2105–2107 | "*Returns:* (or *Yields:* for generators): Describe the semantics of the return value, including any type information that the type annotation does not provide." | The section describes the **value**, not the call. `Returns:` and `Yields:` are one section kind. |
| 2108–2111 | "It may also be omitted if the docstring starts with "Return", "Returns", "Yield", or "Yields" (e.g. `"""Returns row from Bigtable as a tuple of strings."""`) *and* the opening sentence is sufficient to describe the return value." | A `Returns …` **summary line** is endorsed for functions, so the rule must not touch the summary. |
| 2115–2116 | "describe such a return value as: "Returns: A tuple (mat_a, mat_b), where mat_a is ..., and ..."" | The canonical form after the header is a noun phrase. |
| 2149–2150 | `Returns:` / `A dict mapping keys to the corresponding table row data fetched.` | Same. |
| 2158 | `Returned keys are always bytes.` (later line of the same section) | A later line may start with a `Return…` word; only the opening matters. |
| 2219–2220 | `Returns:` / `A context manager providing a reference to a remote file.` | Same. |
| 2081–2084 | "The docstring for a `@property` data descriptor should use the same style as the docstring for an attribute or a function argument (`"""The Bigtable path."""`, rather than `"""Returns the Bigtable path."""`)." | The related property rule: different construct (summary line of a property), different reason (attribute style). See [03_design.md](03_design.md) D7. |
| 2122–2129 | "*Raises:* List all exceptions that are relevant to the interface followed by a description. Use a similar exception name + colon + space or newline and hanging indent style as described in *Args:*." | `Raises:` holds `Name: description` entries, so `Raises ValueError if …` is a malformed entry, not a stutter. See D6. |

The guide never states "do not start the section with Returns" in words; the rule generalizes its definition ("semantics of the return value") and every example it gives.

### PEP 257

PEP 257 asks for an imperative **summary line** ("Return the pathname …"); it defines no sections and says nothing about section content.

### Sphinx Napoleon (Google style)

Napoleon renders `Returns:` as a "Returns" field, so `Returns: Returns True …` appears verbatim in generated docs. Napoleon's Google style also accepts a type prefix on the first line (`bool: True if successful.`), which the legacy linter does not see through (see [01_understand.md](01_understand.md) §4.5).

## 2. Existing linters

Verified against the official documentation; nothing covers the check.

| Tool | Closest rules | Covers this check? |
| :--- | :--- | :--- |
| Ruff `D` (pydocstyle) | `D401` non-imperative-mood: "Checks for docstring first lines that are not in an imperative mood" (summary line only; "disabled when using the google conventions"). `D404` (first word `This`), `D405`–`D417` (section header formatting and missing Args, `D417`). | **No.** No D rule reads section content beyond `D417` argument names. `D401` even exempts properties: methods decorated with `property-decorators` "can use a non-imperative summary line" (Ruff settings, `lint.pydocstyle.property-decorators`). |
| Ruff `DOC` (pydoclint port, preview) | `DOC102`, `DOC201`, `DOC202`, `DOC402`, `DOC403`, `DOC501`, `DOC502` | **No.** Presence or absence of sections, not their wording. |
| pydoclint | `DOC201`–`DOC203` (Returns presence and type consistency), `DOC402`–`DOC405` (Yields), `DOC5xx` (Raises); option `treat-property-methods-as-class-attributes` | **No.** `DOC203` compares the *type* of the Returns section with the annotation; nothing reads the description. (`scratch/pydoclint_violation_codes.md` §3, §5.) |
| Pylint `docparams` | `missing-return-doc` (W9011), `redundant-returns-doc` (W9008: "Please remove the return/rtype documentation from this method"), `redundant-yields-doc` (W9010) | **No.** Note the **name collision**: Pylint's `redundant-returns-doc` means "a Returns section on a function that returns nothing", not the legacy linter's meaning. The omnicoach name must avoid "redundant returns". |
| Pylint `docstyle` | `docstring-first-line-empty` (C0199), `bad-docstring-quotes` (C0198) | **No.** |
| darglint (archived) | Section presence and argument matching | **No.** |
| Vale / prose linters | Generic repetition rules (`Vale.Repetition` flags a repeated *adjacent* word) [U: not re-verified] | **No.** The header and the first word are separated by a colon and a line break. |

### Replacing configuration

None exists. The closest companion configuration, for a project that also wants section presence checked (not a replacement):

```toml
[tool.ruff.lint]
extend-select = ["D", "DOC"]   # DOC rules are preview-only
preview = true

[tool.ruff.lint.pydocstyle]
convention = "google"
```

## 3. Gaps in the legacy linter to improve on

| # | Legacy linter behavior | Kind | Example |
| :-- | :--- | :--- | :--- |
| G1 | Flags the noun reading `Return code …`, `return value …`, `Return type …` | False positive | `Returns:` / `Return code based on current program state.` (google3, see [01_understand.md](01_understand.md) §3) |
| G2 | Misses a typed first line | False negative | `Returns:` / `bool: Returns True if ready.` |
| G3 | Only checks the first `Returns:` / `Yields:` section of a docstring (the `break` ends the scan) | False negative | A generator documenting both `Yields:` and `Returns:` |
| G4 | Ignores `Return:` / `Yield:` headers, which Napoleon accepts | False negative | `Return:` / `Returns the row.` |
| G5 | Reports on the `def` line | Location | The editor marks the signature, not the offending line. |

## 4. Prior art for the name

- Go's `golint` called a repeated package prefix "stutter" (`log.LogError`); Clippy's `module_name_repetitions` is the Rust equivalent. "Stutter" is understood vocabulary for "repeats the enclosing name".
- Pylint `redundant-returns-doc` (W9008) is taken with another meaning (see §2).
