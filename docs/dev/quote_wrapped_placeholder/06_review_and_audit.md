# Phase 6: Review & Audit — `quote-wrapped-placeholder` (`QuoteWrappedPlaceholderRule`)

This document records the independent pedantic code/test and user-facing text reviews conducted for `quote-wrapped-placeholder` (`QuoteWrappedPlaceholderRule`), along with the verification of all findings.

> Status: **COMPLETE and validated.**
>
> **Superseded in part (2026-10-05):** SQL strings and isolated quoted placeholders (`f"'{value}'"`) are no longer exempt; SQL injection is left to Ruff `S608`. The prose and quote heuristics moved from `ast/python/quote_wrapped.rs` into the rule file, on top of the construct-level `ast/python/format_strings.rs`. The rule's `what_it_does` is authoritative.

---

## 1. Independent Review Setup

Two independent `research` subagents performed read-only pedantic reviews of the working copy:
1. **Code & Test Reviewer** (`a037f54b-74fe-4432-a11b-4bd5222593e5`): audited [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs), [src/code_lint/rules/quote_wrapped_placeholder.rs](../../../src/code_lint/rules/quote_wrapped_placeholder.rs), test coverage, edge cases, and mutation resilience.
2. **User-Facing Text & Documentation Reviewer** (`759a5ea9-0f7d-42b8-b24e-8d40fb820093`): audited `ViolationTemplate`, `RuleDoc`, `--explain` output, taxonomy classification, and `docs/dev/quote_wrapped_placeholder/` against [docs/dev/naming_and_message_style_guide.md](../naming_and_message_style_guide.md) and [docs/dev/tag_guide.md](../tag_guide.md).

---

## 2. Findings & Resolutions

| # | Source | Severity | Finding | Resolution |
| :--- | :--- | :--- | :--- | :--- |
| **1** | Code & Test Review | Minor | In `is_prose_message_text` ([src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs)), f-string `{...}` interpolations are absent from the checked text because `append_string_literal_segments` skips `interpolation` nodes, so `f"'{value}'"` had zero prose words (`E4` isolated quoted placeholder exemption). However, for `.format()` and `%` strings (`"'{value}'".format(value=x)` and `"'%(value)s'" % {"value": x}`), the placeholder text `{value}` or `%(value)s` remained inside `literal_text`, so `"value"` was counted as an alphabetic prose word and falsely flagged. | Added `strip_non_fstring_placeholders`, which `combined_message_literal_text` applies before `is_prose_message_text` counts prose words. It strips `{...}` and `%(...)s` tokens so `.format()` and `%` strings behave identically to f-strings for isolated quoted placeholders. Added the `isolated_str_format_and_printf_ignored` case to `test_collect_quote_wrapped_placeholders_python`. |
| **2** | Code & Test Review | Minor | In `collect_quote_wrapped_placeholders` ([src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs)), when an f-string was part of an implicit string concatenation (`concatenated_string`), such as `"--output=" f"'{output_path}'"` or `` "See `" f"'{symbol}'" "` in module" ``, each `string` child was inspected in isolation without the preceding sibling literals, missing the `=` flag prefix or unclosed backtick in the preceding fragment. | Added `preceding_concatenated_literal_text` and prepended its result to the first literal fragment in `collect_fstring_quote_wrapped`, so `has_valid_left_prose_boundary` sees the `=` flag prefix or the unclosed backtick. Added the `concatenated_flag_and_backtick_ignored` case to `test_collect_quote_wrapped_placeholders_python`. |
| **3** | User-Facing Text Review | Minor | `what_it_does` in [src/code_lint/rules/quote_wrapped_placeholder.rs](../../../src/code_lint/rules/quote_wrapped_placeholder.rs) listed the unflagged constructs (`E1`–`E3`, `E5`) but omitted `E4` (isolated quoted placeholders without surrounding prose, such as `f"'{value}'"`). | Added `"isolated quoted placeholders without surrounding prose (`f\"'{value}'\"`),"` to `what_it_does`. |
| **4** | User-Facing Text Review | Minor | `Topic::LITERALS.scope_note` in [src/rule_declaration/taxonomy.rs](../../../src/rule_declaration/taxonomy.rs) and [docs/dev/tag_guide.md](../tag_guide.md) said `"Writing string and number literals (multiline strings, magic numbers). Not identifiers or formatting APIs."`, which was slightly tense now that `quote-wrapped-placeholder` is tagged `Topic::LITERALS`. | Updated `Topic::LITERALS.scope_note` in both files to `"Writing string and number literals (multiline strings, format placeholders, magic numbers). Not identifiers or logging calls (see `logging`)."` |

---

## 3. Verification Matrix

- **Rule unit tests**: `cargo test --lib code_lint::rules::quote_wrapped_placeholder` — 33 cases (`19` pass, `14` fail) + `RepeatCheck::SameCode` — **PASS**.
- **AST unit tests**: `cargo test --lib test_collect_quote_wrapped_placeholders` — **PASS**.
- **Per-exemption mutation check**: `5/5` mutations (`E1` raw and byte strings, `E2` conversion/format_spec/debug `=` check, `E3` `is_prose_message_text` SQL and non-prose strings, `E4` `has_valid_left_prose_boundary`, `E5` `has_valid_right_prose_boundary`) — **[KILLED]**.
- **Registry & architecture guardrails**: `cargo test --test registry` and `cargo test --lib architecture` — **PASS**.
