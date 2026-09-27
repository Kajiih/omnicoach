# Phase 2: Resources & References — `prefer-tuple-unpacking`

This document records **Phase 2 (Gather Resources and Reference)** for `prefer-tuple-unpacking`. It builds on the validated [01_understand.md](01_understand.md) (decisions D1–D17).

> Status: **VALIDATED** (2026-09-27). R1–R8 accepted; constraints re-checked for idiomaticity (§2.4).

Confidence markers: ✅ verified against official docs/search this session · ⚠️ from recalled docs/source, verify before citing · ❌ searched, not found.

---

## 1. External State of the Art

### 1.1 Comparison

| Tool · Rule | Trigger | Granularity | Threshold / options | Relation to our design |
| :--- | :--- | :--- | :--- | :--- |
| ESLint `prefer-destructuring` ✅ | `const x = arr[<int>]` / `x = arr[<int>]` | Per statement | `{VariableDeclarator, AssignmentExpression} × {array, object}` | Closest "enforce destructuring" rule, but per statement: flags `const x = arr[1]` → `[, x] = arr`. Docs concede large indices; array mode is the part most often disabled. Rule is frozen. |
| unicorn `no-unreadable-array-destructuring` ✅ | `[,, x] = arr` | Per pattern | `maximumIgnoredElements`, default **1**, counts **consecutive** holes | The counter-rule: the placeholder cap (D12) from the other side. |
| Clippy `index_refutable_slice` ✅ | Slice bound by a refutable pattern, then indexed by constants → slice pattern `[a, b, ..]` | **Aggregate per binding** ⚠️ | `max-suggested-slice-pattern-length`, default **3** | Best Rust precedent: aggregate, suggests a pattern, capped length. |
| IntelliJ Kotlin "Use destructuring declaration" ✅ | `val p = …; p.first; p.second` | **Aggregate per variable**, ⚠️ only if **all** uses are component reads | — | Aggregate model like ours, but "all uses" instead of "≥ 2 reads". KTIJ-40136: users asked to stop the single-component case. |
| C# IDE0042 ✅ | Local tuple only used for element access → `var (a, b) = …` | **Aggregate per local** ⚠️ (all references are element accesses) | `csharp_style_deconstructed_variable_declaration` | Same "all uses" model as IntelliJ. |
| wemake WPS460 ✅ | `a, = x` / `(a,) = x` | Per node | — | Actively prefers `x[0]` over single-element unpacking: supports D2 (N = 2). |
| wemake WPS236 ✅ · detekt `DestructuringDeclarationWithTooManyEntries` ✅ | Unpacking with too many targets | Per node | max 4 (wemake), 3 (detekt) | Upper bound on unpacking size: supports D12 and the separate named-record rule. |
| SwiftLint `large_tuple` ✅ | Tuple **types** with too many members | Per type | warning 2, error 3 | Named-record side (§6 of 01), not this rule. |
| Pylint W0632 `unbalanced-tuple-unpacking` ✅ | `a, b = (1, 2, 3)` | Per node | — | Safety net for our suggested fix's arity. |
| Ruff PLR1736 / PLR1733 ✅ | Redundant `xs[i]` / `d[k]` inside `enumerate` / `.items()` loops | Per node | autofix | Neighbor, different antipattern. |
| Ruff RUF015 ✅ | `list(x)[0]` → `next(iter(x))` | Per node | autofix | Neighbor. |
| Clippy tuple `.0` / `.1` → destructuring | ❌ no lint, no request found | — | — | **Gap**: our Rust side is novel. |
| Ruff / Pylint "unpack instead of index" | ❌ no rule, no request found | — | — | **Gap**: our Python side is novel. |

### 1.2 Key ideas

1. **Per-statement vs aggregate.** ESLint's per-statement design is why its array mode is noisy: it cannot see that only one position is read, so `const x = arr[1]` becomes `[, x] = arr`, which unicorn then flags. Every tool that works well (IntelliJ, IDE0042, Clippy `index_refutable_slice`) aggregates per binding. **Validates D2 + D4.**
2. **All-uses vs some-uses.** IntelliJ and IDE0042 fire only when *every* use of the variable is a component read, so the variable disappears after the fix (near-zero false positives, safe autofix). Our rule fires on ≥ 2 reads with other uses allowed, minus D11's collection-use exemption. Ours finds more (e.g. a tuple that is also passed along whole); theirs is safer.
3. **Placeholder caps converge on 1–4.** unicorn 1 (consecutive), Clippy pattern length 3, detekt 3, wemake 4. D12's "≤ 2 placeholders" sits in the range. unicorn counts **consecutive** holes; D12 counts **total** holes.
4. **Pattern in the message.** Clippy's `index_refutable_slice` shows the suggested pattern in its diagnostic.
5. **Rust moves.** `let (a, b) = t;` moves non-`Copy` fields out of `t`; `let (a, b) = &t;` binds references and never moves. A by-value suggestion can fail to compile when `t` is used afterwards.
6. **Style guides are silent.** PEP 8 and the Google Python Style Guide say nothing on unpacking vs indexing ✅; the Python tutorial §5.3, PEP 3132, and go/python-tips/069 are the normative sources (01 §1).

### 1.3 Evidence of pitfalls

- **Length semantics (Python)**: `a, b = t` requires `len(t) == 2`; `t[0], t[1]` works for any length ≥ 2. Covered by suggesting `*_` when the arity is unknown (01 §2.1).
- **`first, *_, last = xs`** raises on a 1-element sequence while `xs[0], xs[-1]` returns the same element twice; `*_` also copies the middle (O(n)). Accepted under D3/D11; document in the rule docs.
- **Indexable but iterates differently**: `re.Match` (D14), int-keyed dicts and pandas `df[0]` (D11 helps only if the scope also iterates / `len()`s them).
- **Rust mixed read/write scopes** (`t.0 += 1` next to `t.0`, `t.1` reads): destructuring by value cannot write back.

---

## 2. Internal Codebase

### 2.1 Reusable building blocks

| Need | Existing piece | Location |
| :--- | :--- | :--- |
| Rule skeleton, per-function aggregate, threshold | `max_test_assertions.rs` (`LanguageDefaults`, `effective_max_threshold`, `diagnostic_at_node`, per-language `suggestion`) | `src/code_lint/rules/max_test_assertions.rs` L10–71 |
| `min` threshold | `no_identical_positional_types.rs` (`effective_min_threshold`) | L14, L148 |
| Group-by-text inside the rule | `collect_duplicate_type_groups` | `no_identical_positional_types.rs` L63–84 |
| Language branching inside the rule | `no_env_in_functions.rs` (`ast::python::collect_environ_subscripts`) | L132–146 |
| Scope-bounded recursion | `count_python_assertions` (stops at `function_definition` / `class_definition`); `count_rust_assertions` (stops at `function_item`, descends into closures) | `ast/python.rs` L972; `ast/rust.rs` L492 |
| Subscript collector model | `collect_environ_subscripts` (`subscript` → `value`, `identifier` / `attribute`) | `ast/python.rs` L1019–1046 |
| Store vs read split | `traverse_python` (skips `assignment.left`, `for_statement.left`) | `ast/python.rs` L204–281 |
| Language-agnostic wrapper | `dispatch_lang!` + `collect_test_function_assertion_counts` | `ast.rs` L12–25, L309–318 |
| Tags | `Tag::Style`, `Tag::Opinionated`, `Tag::Heuristic` all exist | `core.rs` L31–74 |

### 2.2 Gaps (new helpers)

> Superseded by 03 §3.1–§3.2: one facade `collect_positional_reads` over both languages, and scopes opened by recursion with no scope key. Kept as the research record.

Rules see opaque `AstNode`s (text, span, location; no kind or navigation), so all grammar logic goes into `code_lint::ast` (design guide §7):

- **`ast::python::collect_literal_index_reads(file)`**: one group per scope (function body, module; lambdas dropped), each with reads `{node, receiver_text, index: i64}` and the set of receivers disqualified by D11 / D3. Single walk; D11 detection (`len`, iteration, mutating methods, non-literal index, slice, write target) in the same pass.
- **`ast::rust::collect_tuple_field_reads(file)`**: same shape; `field_expression` with an `integer_literal` field; receivers containing a call skipped; write targets (`assignment_expression.left`, `compound_assignment_expr.left`) handled.
- Shared return struct plus a `dispatch_lang!` wrapper in `ast.rs`. Placeholder arithmetic stays in the rule (pure policy).
- **Scopes keyed by node span, not name**: the harness repeats code with identical `def` names, and nested scopes must not leak into parents.

### 2.3 Constraints discovered

| Constraint | Source | Effect |
| :--- | :--- | :--- |
| Fail cases: exactly one diagnostic; repeated twice must give two | `test_utils.rs` L133–197 | Every fail case inside a `def` / `fn` (D17). Snippet = anchor text (`point[0]`, `span.0`). |
| `rule_test!` runs only `Config::default()` | `test_utils.rs` L111–168 | Only default thresholds testable; boundaries (`row[0], row[3]` fail / `row[0], row[4]` pass) exercised at defaults. |
| No `.sort` substring in rule files | `tests/registry.rs` L150–167 | The mutating-method list (contains `sort`) belongs in the `ast` helper, not the rule file. |
| Dogfooding: omni on `src/` and `tests/` must exit 0 | `tests/cli.rs` L305–321 | `src/command_lint/rules/jj.rs` L95 `SourceSpan::new(cmd.span.0, cmd.span.1)` is a true positive, fixed in the same change. (It turned out to sit inside `vec![]`, so dogfooding never reports it; see 04 §2.) |
| Rust macro bodies are opaque `token_tree`s | tree-sitter-rust | Reads inside `assert_eq!`, `format!`, `vec![]` are invisible: documented false negatives. |
| Python `-1` is `unary_operator(integer)`; `a[1, 2]` is a tuple key | tree-sitter-python | Parse unary minus on decimal integers; treat tuple keys as non-literal. |

### 2.4 Constraint Review: Do They Push Away From Idiomatic Code?

Each constraint was re-checked against the pit of success; none needs relaxing.

| Constraint | Verdict |
| :--- | :--- |
| Scopes keyed by node span, not name | **Correct regardless of the harness.** Real code repeats names too (same method name in two classes, nested helpers); a name key would merge unrelated scopes. (Implemented with no key at all: groups are opened by recursion, 03 §3.2.) |
| No `.sort` in rule files | **Not a real constraint.** The check matches the substring `.sort`; a method-name list holds bare strings (`"sort"`), which do not match. The list lives in the `ast` helper anyway because D11 detection happens in the same walk. |
| Only default thresholds testable in `rule_test!` | **Idiomatic.** Design guide §6.3: threshold plumbing is tested centrally in `src/core.rs`; rule tests cover the default boundaries (`row[0], row[3]` fail / `row[0], row[4]` pass). |
| Dogfooding flags `jj.rs` L95 | **Assumption was wrong.** It is a true positive, but it sits inside `vec![]`, whose arguments the rule does not inspect (macro gap), so dogfooding never flagged it. Fixed by hand (`let (start, end) = cmd.span;`, 04 §2). |

---

## 3. Our Design vs References

| Aspect | Ours (01) | References | Verdict |
| :--- | :--- | :--- | :--- |
| Aggregation | Per `(scope, receiver)` | IntelliJ, IDE0042, Clippy aggregate; ESLint per statement (noisy) | Keep |
| Trigger | ≥ 2 distinct positions, other uses allowed | IntelliJ / IDE0042: all uses must be component reads | Keep (finds more); D11 covers the collection side |
| Single read | Not flagged (D2) | wemake WPS460, KTIJ-40136 agree | Keep |
| Placeholder cap | ≤ 2 total (D12) | 1–4; unicorn counts consecutive | Keep total (R1) |
| Named-record split | Separate rule | SwiftLint, detekt, WPS236 are all separate | Keep |
| Rust fix wording | `let (a, b) = t;` | Clippy-style pattern; move semantics | Adjust (R3) |
| Novelty | — | No Ruff / Pylint / Clippy equivalent | Our rule fills a real gap |

---

## 4. Proposed Adjustments (for Validation)

| ID | Proposal | Reason |
| :--- | :--- | :--- |
| R1 | D12 counts **total** placeholders; `*_` is free. | Simpler to explain than unicorn's consecutive count, and stricter where it matters (`a, _, c, _, e` reads worse than one run of two). |
| R2 | No "all uses" mode (IntelliJ / IDE0042 style). | D11 already removes the collection-use false positives; a second mode is configuration without a requested need (simplicity first). |
| R3 | Rust suggestion: `let (start, end) = span;`, adding "destructure `&span` when fields are not `Copy`" (Python: `start, end = span`, with `*_` when arity is unknown). | By value is cleanest for `Copy` fields (the common case, e.g. `jj.rs` spans: no `*start` at use sites) but moves non-`Copy` fields; `&span` never moves but forces derefs. The rule cannot see types, so the suggestion names both in one sentence. |
| R4 | Summary names the receiver and positions (`` `span` is read by index at positions 0, 1 ``); suggestion stays a per-language canonical form, not a computed pattern. | Positions make the finding self-explanatory; a computed pattern needs invented names (`a, _, c`), so the generic canonical form is clearer and simpler. |
| R5 | Rust analog of D11's write part: skip a receiver when the same scope writes any of its fields (`t.0 = x`, `t.0 += 1`, `&mut t.0`). | Destructuring cannot write back; mixed scopes would get an unhelpful suggestion. Coherent with Python D3/D11. |
| R6 | Python class bodies and Rust `impl` / `const` initializers: no own group, reads there are ignored. | Rare; D5 names only functions and module top level. Keeps scope rules minimal. |
| R7 | Index literals: decimal integers and unary minus on them; anything else (`0x1`, `1_0`, `+1`, tuple keys `a[1, 2]`) is a non-literal index (D11 disqualifier in Python). | Minimal parsing; exotic forms are rare and conservatively exempt. |
| R8 | Fix `src/command_lint/rules/jj.rs` L95 as part of the change; document Rust macro opacity as a known false negative (pass test). | Genuine instance of the pattern; honest limitations. (Assumed to break dogfooding; it does not, since L95 sits inside `vec![]`, see 04 §2.) |

### Out-of-scope defects surfaced

- `rule_design_guide.md` §6 named `ThresholdDefaults`; the type is `LanguageDefaults<usize>`. **Fixed** in a separate commit.
- `ROADMAP.md` Candidate Rules entry was outdated against D3 / D13. **Synced** with the design docs.
- Stray untracked `.pending-snap` files in `src/code_lint/rules/` (leftovers from before `rule_test!` banned snapshots). **Deleted** locally.

---

## 5. Sources

- ESLint `prefer-destructuring`: https://eslint.org/docs/latest/rules/prefer-destructuring
- typescript-eslint `prefer-destructuring`: https://typescript-eslint.io/rules/prefer-destructuring
- unicorn rules: https://github.com/sindresorhus/eslint-plugin-unicorn/tree/main/docs/rules
- Clippy lints: https://rust-lang.github.io/rust-clippy/master/
- Rust API Guidelines (C-NEWTYPE): https://rust-lang.github.io/api-guidelines/type-safety.html
- Kotlin destructuring: https://kotlinlang.org/docs/destructuring-declarations.html
- C# IDE0042: https://learn.microsoft.com/dotnet/fundamentals/code-analysis/style-rules/ide0042
- wemake-python-styleguide: https://wemake-python-styleguide.readthedocs.io
- Pylint W0632: https://pylint.readthedocs.io/en/stable/user_guide/messages/warning/unbalanced-tuple-unpacking.html
- Ruff rules: https://docs.astral.sh/ruff/rules/
- PEP 3132: https://peps.python.org/pep-3132/ · PEP 3113: https://peps.python.org/pep-3113/
- Google Python Style Guide: https://google.github.io/styleguide/pyguide.html
