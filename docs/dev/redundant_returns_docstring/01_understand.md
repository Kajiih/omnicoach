# 01 — Understand: the legacy linter `RedundantReturnsDocstringRule`

> [!NOTE]
> Status: **PENDING** (lead validation). Batch D (docstrings), worker phase 1 of 7. Playbook: [rule_batch_playbook.md](../rule_batch_playbook.md).

## 1. The source check

The legacy linter, `scratch/legacy_reference/legacy_lints.py` lines 3135–3162:

```python
class RedundantReturnsDocstringRule(LintRule):
    """Enforces that docstring Returns descriptions do not start with 'Return(s)...'."""
    node_types = FUNCTION_NODES
    def check(self, node):
        docstring = ast.get_docstring(node)
        ...
        for line in docstring.splitlines():
            stripped = line.strip()
            if stripped in {"Returns:", "Yields:"}:
                in_returns = True
                continue
            if in_returns and stripped:
                if re.match(r"^(?:Return|Returns|Yield|Yields)\b", stripped, re.IGNORECASE):
                    word = stripped.split()[0]
                    self.visitor.report_warning(node.lineno,
                        f"Docstring return description should not start with '{word}...' "
                        "as the section header already specifies Returns/Yields.")
                break
```

Exact behavior:

| Aspect | The legacy linter |
| :--- | :--- |
| Scope | Every `def` / `async def` (methods, nested functions, properties, tests). Not classes or modules. |
| Section detection | A line that is exactly `Returns:` or `Yields:` after stripping. Case-sensitive; `Return:`, `Yield:` and `Returns :` are not sections. |
| Text checked | Only the **first non-blank line** after the header, then the loop stops (`break`), so only the first `Returns:` / `Yields:` section is ever checked. |
| Match | `^(Return\|Returns\|Yield\|Yields)\b`, case-insensitive. `Returned keys …` and `ReturnCode …` do not match (no word boundary); `return code, stdout …` does. |
| Report | One warning per function at `node.lineno` (the `def` line), naming the first word. |
| Rationale given | "the section header already specifies Returns/Yields". |

## 2. The antipattern

```python
def is_ready(self) -> bool:
    """Checks readiness.

    Returns:
      Returns True if the device is ready.
    """
```

The rendered docs (Sphinx Napoleon, IDE hovers, `help()`) read `Returns: Returns True if …`. The first word of the section repeats the header, and the sentence describes the *action* of the function (a second summary line) instead of the *value* the caller receives. Google style asks for the latter: "Describe the semantics of the return value" (§3.8.3, `scratch/pyguide.md` line 2106), and its own examples start the section with a noun phrase: "A dict mapping keys to the corresponding table row data fetched." (line 2150) and "A context manager providing a reference to a remote file." (line 2220).

The fix is mechanical: drop the leading verb and start with what the value is (`True if the device is ready.`, `A list of instances of BaseCL.`).

## 3. How common it is

Code Search over google3 Python (PCRE, case-insensitive, so the counts are upper bounds):

- `Returns:` followed by `Return(s) the|a|an|True|whether|None`: ~24k files. Samples: `Returns True iff the mutation succeeded.` (security/realms/turnup/handlers.py:1069), `Returns a list of instances of BaseCL.` (devtools/clstats/repository.py:301), `returns the alloc owner mdb.` (production/sisyphus/spanner/spanbot/framework/spanlib.py:2355), `Return the size of the mount.` (net/hostdatapath/usps/hooks/common/utils.py:2878).
- The same search with `Return code|value|type|status` finds the **noun** reading, which the legacy linter flags wrongly:
  - `Return code based on current program state.` (quality/views/extraction/kcube/bg/caller/conversation_context.py:368)
  - `return code, stdout and stderr of the command.` (cloud/cluster/testing/framework/windows_test_utils.py:1041)
  - `return value passed through from provided function.` (cloud/bigstore/testing/clients/json_generated_client.py:139)

The pattern is frequent, so the rule has signal; the noun reading is the main false-positive source to design out.

## 4. Questions the check raises

1. **Verb-led stutter vs. any restatement.** Is the antipattern "the section starts with the verb `Return(s)` / `Yield(s)`" or, more broadly, "the section restates that something is returned" (`This function returns …`, `The return value is …`, `Return value: …`)?
2. **Noun reading.** `Return code`, `Return value`, `Yield of the crop`: the same word is a noun or a noun modifier. Where is the line?
3. **Other sections.** `Raises:` entries written as `Raises ValueError if …`, Args entries such as `callback: Returns the new value.` (describing a callable parameter, which is correct).
4. **The Google property rule.** §3.8.3: "The docstring for a `@property` data descriptor should use the same style as the docstring for an attribute … (`"""The Bigtable path."""`, rather than `"""Returns the Bigtable path."""`)" (lines 2081–2084). Same rule or a separate one?
5. **Typed Returns lines.** Napoleon's Google style also accepts `Returns:` followed by `bool: Returns True if …` (a type prefix). The legacy linter misses these because the first word is `bool:`.
6. **Summary line.** Google explicitly allows a docstring that *starts* with `Returns …` and omits the section (line 2108–2111), so the summary line is out of scope for functions.

These are answered in [03_design.md](03_design.md).

## 5. Language scope

Python only (batch decision b). Rustdoc has no `Returns` section convention: the sections the Rust API Guidelines and Clippy know are `# Examples`, `# Errors`, `# Panics`, `# Safety`. A Rust analogue is noted as a ROADMAP suggestion in [03_design.md](03_design.md) §9.
