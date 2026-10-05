# Phase 7: Learn — `quote-wrapped-placeholder` (`QuoteWrappedPlaceholderRule`)

This document captures the reusable lessons from designing, implementing, and auditing `quote-wrapped-placeholder` (`QuoteWrappedPlaceholderRule`).

> Status: **COMPLETE.**
>
> **Superseded in part (2026-10-05):** SQL strings and isolated quoted placeholders (`f"'{value}'"`) are no longer exempt; SQL injection is left to Ruff `S608`. The prose and quote heuristics moved from `ast/python/quote_wrapped.rs` into the rule file, on top of the construct-level `ast/python/format_strings.rs`. The rule's `what_it_does` is authoritative.

---

## 1. Key Takeaways

1. **Normalize AST Representation Differences Between f-strings and `.format()` / `%` Strings**:
   - In `tree-sitter-python`, an f-string splits `string_content` and `interpolation` into sibling AST nodes, whereas `.format()` and `%` format strings are single `string_content` nodes containing literal `{name}` or `%(name)s` substrings. Any heuristic that inspects the combined literal text (such as `is_prose_message_text`) must strip `{...}` and `%(...)s` placeholders from non-f-string literal fragments first so that `f"'{value}'"` and `"'{value}'".format(value=x)` are treated identically.
2. **Inspect Preceding Sibling Fragments in `concatenated_string` Nodes**:
   - Python's implicit string literal concatenation (`"--output=" f"'{output_path}'"`) places the `=` prefix in an earlier `string` child of `concatenated_string`. Collecting the literal text of preceding `string` siblings in `preceding_concatenated_literal_text` ensures prefix exemptions (`key="value"`, unclosed backticks, SQL/HTML prefixes) work for f-string parts of implicitly concatenated strings. `.format()` and `%` parts still run the prefix checks on their own text only.
3. **Require Both a Formatting Context Gate and a Prose Gate**:
   - Combining an explicit formatting-context check (`E1`: f-string, `.format`/`.format_map`, `%` binary operator, or logger call with format arguments) with a prose-word check (`E4`: at least one ≥2-letter alphabetic word outside the quotes) and structured-syntax exemptions (`E3`/`E5`: HTML, SQL, JSON/TOML, `key='...'`, backticks) eliminates false positives on code-generation templates, CLI flag builders, and SQL queries while catching human-facing log and exception messages.
