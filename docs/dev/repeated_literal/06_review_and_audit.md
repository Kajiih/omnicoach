# Phase 6: Review and Audit — `repeated-literal`

Three independent read-only reviewers who did not write the code. Each was given the project rules, docs 01–05, the style and tag guides and `ROADMAP.md`:

| Reviewer | Area | Transcript |
| :--- | :--- | :--- |
| CO | Collectors (`ast.rs`, `ast/python.rs`, `ast/rust.rs`) | `ed680853-…` |
| RH | Rule, harness (`test_utils.rs`), dogfood edits | `b1e89220-…` |
| TD | Test quality, doc coherence | `92635708-…` |

The orchestrator re-checked the findings that change behavior or numbers:
- CO-1 is confirmed by reading the code: raw and plain content are both stored undecoded.
- `0x1f32` stays an integer, as CO also found.
- The named-case count is 43 (TD-9 is right; RH counted the 2 generated tests).
- CO-6 still needs a grammar probe, to be done during the fix.

**Status: decided and applied** (2026-10-03). The user delegated the triage: fix what is RICR and not speculative, defer or drop the rest. Outcome in §4. IDs merge duplicates; source IDs are in brackets.

---

## 1. Triage

### Behavior

| ID | Sev | Finding | Proposed | Reason |
| :--- | :--- | :--- | :--- | :--- |
| B1 | Med | Raw and plain strings with the same characters merge, even when their values differ: `r"a\nb"` and `"a\nb"` form one group. Following the suggestion would change behavior. [CO-1] | **Fix** | Normalize raw content to its plain spelling by doubling each `\`. Then `r"\d"` equals `"\\d"`, which is correct. Add a pass case per language. |
| B2 | Low | `is_trivial` treats every escape as `\` plus one character. So `"\x00"` counts as 3 units, 2 of them alphanumeric. Raw backslashes are also read as escapes, so `r"\s+"` becomes trivial. [CO-2] | **Fix (doc)** | B1 fixes the raw half. For the rest, reword the doc to "`\` and the next character count as one unit". Fully parsing escapes is not worth the code. |
| B3 | Med | Python constants inside module-level `if` / `try` / `with` bodies count as `Inline`. Platform branches and `ImportError` fallbacks are idiomatic, so this breaks CUJ 3. [CO-3] | **Fix** | Climb through blocks owned by `if` / `elif` / `else` / `try` / `except` / `finally` / `with` up to `module` or `class_definition`. Stop at function and lambda scopes. |
| B4 | Low | A parenthesized scalar constant (`MSG = ("…")`, common for long strings) counts as composite, so it never pairs with its inline copies. [CO-4] | **Fix** | Unwrap `parenthesized_expression` in both `literal_value`s. |
| B5 | Low | Tuple unpacking (`X_MIN, X_MAX = 10, 90`) is collected as inline. A chained `A = B = 30` is dropped entirely. [CO-5] | **Defer** | Rare shapes. Add a ROADMAP follow-up. |
| B6 | Low | `-N` in Python `dict_pattern` keys and `keyword_pattern` is also collected as `N` (pending probe). [CO-6] | **Fix** | In any pattern node, read a `-` token followed by a number sibling as the negative number. This also removes the accepted `union_pattern` imprecision, leaving only the Rust token-tree one. |
| B7 | Low | `Final` is detected by stripping a `typing.` text prefix, while `Literal` resolves the path. So `t.Final` and `typing_extensions.Final` are missed. [CO-7] | **Fix** | Resolve the terminal name the way `Literal` does. |
| B8 | Low | `static mut` counts as a constant definition. [CO-8] | **Fix** | A `static_item` with `mutable_specifier` falls through to normal collection. |
| B9 | Low | A tuple index inside a non-exempt macro (`vec![p.3, q.3]`) is counted. [CO-9] | **Defer** | Same token-tree family as the accepted `-N` gap. Widen that ROADMAP entry. |
| B10 | Low | When several constants share a value, the threshold (`1 + inline`) and `{count}` (`definitions + inline`) disagree. [RH-8] | **Fix** | Use `definitions + inline` in both. This matches the option doc ("its constant definition included"). |
| B11 | Low | `.max(1)` is dead, and `min-occurrences` values 0, 1 and 2 behave the same. [RH-7] | **Fix** | Remove it. The option doc says values below 2 act as 2. |
| B12 | Nit | `2` is trivial but `2.0` is not. [RH-16] | **Ask** | D5 was validated as-is. Adding `2.0` would be more consistent. |
| B13 | Nit | `cast` is matched by terminal name, so `.cast("x")` arguments are pruned. A docstring written as an implicit concatenation is collected. [CO-13, CO-14] | **Reject** | The first only causes missed findings. The second is rare. |

### Rule text and metadata

| ID | Sev | Finding | Proposed | Reason |
| :--- | :--- | :--- | :--- | :--- |
| M1 | Low | The Python suggestion always adds the `case` advice, which is noise for most findings. The doc already explains it, and §2.4 says suggestions must not re-explain. [RH-5] | **Fix** | Drop the Python override. Keep the advice in `why_is_this_bad`. |
| M2 | Low | `{expression}` is reused with a different meaning, which style guide §3 forbids. [RH-11] | **Fix** | Add `{literal}` to the vocabulary (guide §3 and the registry test). |
| M3 | Low | "appears {count} times" leaves out exempt copies (tests, macros, composite constants). [RH-9] | **Fix (doc)** | Keep the wording. `what_it_does` states what the count covers. |
| M4 | Low | `what_it_does` has several inaccuracies: [RH-10, TD-19, RH out-of-scope] <br>• It omits the `#[test]` / `#[rstest]` function exclusion. <br>• It files tuple positions under "required" instead of "not values". <br>• Its example `'x'` is trivial, and is a char in Rust. <br>• It doesn't say that ints and floats are distinct. <br>• It says "composite" constants are not collected, but in Rust any non-literal initializer is skipped. | **Fix** | Reword. |
| M5 | Med | Several dogfood constants just repeat their own key (`CALLEE = "callee"`, `RULE_PLACEHOLDER = "rule"`). The key is already a name, and the template still spells `{callee}` by hand, so nothing is single-sourced. `CALLEE` also exists twice. [RH-4] | **Ask** | See §3, Q1. |

### Tests

| ID | Sev | Finding | Proposed | Reason |
| :--- | :--- | :--- | :--- | :--- |
| T1 | **High** | Much of the rule's own logic is untested: <br>• 3+ copies (`inline_uses[1..2]` would pass every test). <br>• A constant plus 2 uses. <br>• `{count}`. <br>• `min-occurrences = 3` (CUJ 5). <br>`rule_test!` allows one diagnostic per case and has no options slot. [TD-1, TD-8, RH-1] | **Fix** | One `tests/cli.rs` snapshot on a fixture with a `.omnilint.toml` setting `min-occurrences = 3`. It covers all four. |
| T2 | Med | `DistinctLiterals` has no guardrail test showing it still catches a rule that stops early. [RH-2] | **Fix** | Meta-test: a fake rule that flags only the first group, run under `#[should_panic]`. |
| T3 | Med | Several exemption cases are masked or bundled: <br>• The two macro copies sit in different macros, so removing one exemption still passes. <br>• `Literal[...]` sits inside an annotation, which is already exempt. <br>• `Final` is masked by an UPPER_SNAKE name. <br>• Trivial strings and numbers are bundled. [TD-2, TD-3, TD-6, TD-10, RH-3] | **Fix** | One behavior per case, with both copies in the same exempt position. |
| T4 | Med | `test_module_literals_are_ignored` puts the test code second, so the runner would already drop it. The rule's filter exists for the reverse order. [TD-4] | **Fix** | Put the test module first. |
| T5 | Med | 03 promised a mutation check per exemption. Only 2 were recorded. [TD-5] | **Fix** | Run it for every exemption after T3. Record the results in this doc. |
| T6 | Low | Missing cases for claimed behavior: <br>• `NewType` / `ParamSpec` / `TypeVarTuple` / `NamedTuple` / `TypedDict`. <br>• Each macro family. <br>• Base prefixes and raw prefixes at rule level. <br>• Rust `static` / enum, and a constant defined after its use. <br>• `0x1f32`, `#![…]`, `char`. <br>• `is_trivial` boundaries. <br>• Python prefixes and float forms. [TD-7, TD-11, TD-14, CO-10] | **Fix** | Add rows to the existing rstest tables, plus a few rule cases. |
| T7 | Low | The Rust token-tree `-N` imprecision is not pinned. [TD-12] | **Fix** | Add a unit row asserting today's value, with a ROADMAP reference. |
| T8 | Low | Harness tests and docs have gaps: <br>• The "no longer parses" error path is untested. <br>• Rejections assert only `is_err()`. <br>• The hex swap has no row. <br>• `find(content)` can hit the prefix in `rb'rb'`. <br>• The doc wording is inaccurate in places. <br>• `_ => digit` is unreachable. <br>• Unused `PartialEq` / `Eq` derives. <br>• The `name: RULE, repeat:` form is undocumented. [TD-13, RH-12, RH-13, RH-14, RH-15] | **Fix** | Assert error substrings; add the rows; locate the content from the end (`rfind`); use `unreachable!`; drop the derives; fix the wording. Binary/octal rewrites are rejected loudly → document, don't handle. |

### Docs

| ID | Sev | Finding | Proposed |
| :--- | :--- | :--- | :--- |
| D1 | Low | 04 says "45 cases", but there are 43 named cases. [TD-9] | **Fix** |
| D2 | Low | 03 §3.5 describes the rewrite inaccurately (toggling only). [TD-15] | **Fix**: record it in 04 §2. |
| D3 | Low | Historical docs are stale: <br>• 02 has line anchors and `statements.rs`. <br>• 02 still says `case` is exempt. <br>• 01 still has D2/D4/G1. <br>• 01/03 still say "Next: Phase N". <br>• 05:15 says the old name is the only stale reference. [TD-16, TD-18] | **Fix**: add "superseded" notes, drop the anchors, update the banners. |

## 2. Out of Scope (pre-existing or other workstreams)

- `tests/snapshots/*.snap.new` and the modified snapshots in `@` belong to the in-progress explanation-hint work, not to this change.
- `is_docstring_raw` accepts any bare string statement (already in ROADMAP).
- Exempt macros are matched by last path segment, so user macros named `error!` / `write!` are exempt too.
- `rule_test!` bypasses the runner (`SourceOnly`, test files), and it has no options slot. This is the root of T1.

## 3. Open Questions

- **Q1 (M5): placeholder-key constants.**
  - (a) Keep the file-local constants and add a ROADMAP item for typed template placeholders, which would remove the strings entirely.
  - (b) Revert to inline keys and suppress per file.
  - (c) Share one set of key constants in `diagnostic`.
  - Recommended: **(a)**. (b) adds suppressions. (c) is half of the typed-placeholder design without its checking.
- **Q2 (B12):** add `2.0` to the trivial set?

Both were delegated: Q1 → (a), Q2 → yes.

## 4. Outcome

### 4.1 Applied as proposed

B1, B2 (doc), B3, B4, B6, B7, B8, B10, B11, B12, M1, M3, M4, T3, T4, T6, T7, T8, D1, D2, D3. Notes:
- **B7** exposed a real bug: `Final[int]` parses as `generic_type`, not `subscript`. The old text check only seemed to work because the `LIMIT` case used an UPPER_SNAKE name. The unmasked case (`class_final_lowercase_defines`) caught it.
- **B6** turns the `-N` miscount into a skip: a number after a bare `-` token in a Python `*_pattern` or a Rust `token_tree` is not collected. Pinned by `known_gap_negative_numbers_*` pass cases.
- **T8**: the rewrite now locates the first unescaped ASCII letter or digit after the opening quote, instead of `find(content)`, which failed on normalized raw strings. The "no longer parses" path was untested and broken: `0b11` → `0b31` re-parses as `0` plus an error node, with the same literal count. Added `ParsedFile::has_syntax_error` so such rewrites are rejected.
- **Role cases** (constant vs inline): with the constant first, a lost constant role still flags the same text. The discriminating cases now put the use first and spell it differently (`'fast'` vs `"fast"`, `r"jj"`, `404u16`), so only the correct role yields the expected span.

### 4.2 Changed verdicts

| ID | Proposed | Outcome | Why |
| :--- | :--- | :--- | :--- |
| T1 | CLI snapshot with `min-occurrences = 3` | **Mostly dropped** | By design, no rule tests message params or option thresholds (`rule_declaration` tests options centrally; templates are checked by `tests/registry.rs`). The real gap, 3+ copies and constant + 2 uses, needs multi-diagnostic `fail` cases → ROADMAP. Covered meanwhile by the T2 mutation below and collector unit tests. |
| T2 | Meta-test with a fake rule | **One-time mutation check** | `test_utils` cannot import rules (`src/architecture.rs`), and a hand-written fake rule would test itself. |
| M2 | Add `{literal}` | **Widened `{expression}`** | Guide row now reads "the flagged expression (an access, a literal), as written in the source". No vocabulary change in `tests/registry.rs`. |
| T6 | Base prefixes at rule level | **Unit rows only** | `DistinctLiterals` cannot rewrite `0xFF` and `255` consistently; it panics loudly. Documented in the `rule_test!` Rustdoc. |

### 4.3 Deferred to `ROADMAP.md`

B5 (unpacked and chained constants), B9 (tuple indices in custom macros), M5/Q1 (typed template placeholders), T1 remainder (multi-diagnostic `rule_test!` cases, shared with `nested-function`), and the reworded negative-number gap.

### 4.4 Rejected

B13: `.cast(...)` pruning only causes missed findings; implicitly concatenated docstrings are rare.

### 4.5 Mutation check (T5, T2)

Each behavior was disabled once and `cargo test --lib literal` re-run; every mutation failed at least one test.

| Disabled | Failing tests (rule case or unit row) |
| :--- | :--- |
| Python: `type` prune / docstring / `Literal[...]` / type-name call | 2 / 2 / 2 / 8 (rule case + unit rows each) |
| Python: composite constant / interpolation / pattern sign skip | 2 / 3 / 2 |
| Python: transparent statements / parentheses / `Final` / raw spelling / constant role | 3 / 2 / 2 / 2 / 12 |
| Rust: attribute / inner attribute / extern | 2 / 1 (unit) / 1 (unit) |
| Rust: `const` / `static` / enum role / `static mut` guard | 5 / 2 / 2 / 2 |
| Rust: exempt `macro_invocation` / exempt `token_tree` | 1 (unit) / 1 (unit): each alone is backed up by the other at rule level |
| Rust: `field_expression` / token-tree sign skip / raw spelling | 2 / 2 / 3 |
| Rule: Rust test ranges | `test_module_literals_are_ignored` |
| Rule: report only the first group (T2) | 27 `fail` cases + `documented_examples` ("did not report every occurrence") |
