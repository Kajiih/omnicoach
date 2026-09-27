# Phase 6: Review and Audit — `prefer-tuple-unpacking`

Three independent read-only reviewers (none wrote the code), each given the project rules, the rule design guide, `ROADMAP.md`, `decisions/006`, and docs 01–05 up front:

| Reviewer | Area | Transcript |
| :--- | :--- | :--- |
| PY | Python collector (`ast/python.rs`) | `d0330273-…` |
| RS | Rust collector, facade types, rule logic, `jj.rs` fix | `97aa9977-…` |
| TD | `rule_test!` suite, collector unit test, docs coherence | `0d6e4776-…` |

**Status: decided.** The user accepted every proposed verdict; outcomes in §3.

---

## 1. Triage

### Behavior

| ID | Sev | Finding | Proposed | Reason |
| :--- | :--- | :--- | :--- | :--- |
| PY-1 | Med | `for i, x in enumerate(xs)` / `zip` / `sorted` / `reversed` do not mark `xs` as iterated, so `xs[0], xs[-1]` is flagged. Likely the main false-positive source. | **Fix** | D11 says "iterated"; wrapping builtins are the common form. Small constant list, like `MUTATING_METHODS`. |
| PY-2 | Low | Default values / return annotation are collected in the function's scope, though Python evaluates them in the enclosing one. | **Fix** | Open the function scope on the `body` only; one-line change, correct semantics. |
| RS-1a | Med | Rust destructuring assignment `(t.0, t.1) = (b, a);` (a swap) is read as two reads. | **Fix** | Python already climbs target containers (`p[0], p[1] = …`); climb `tuple_expression` / `parenthesized_expression` for coherence. |
| RS-1b | — | Nested writes `t.0.x = 1`, `t.0[i] = v` count as reads. | **Reject** | Coherent with Python (`p[0].x = 1` is a read there too), and the suggestion works: `let (a, b) = &mut t; a.x = 1;`. |
| RS-2a | — | Method autoref `t.0.push(x); t.1.len()` is flagged. | **Reject** | Not a false positive: `let (a, b) = &mut t;` is the fix. |
| RS-2b | Med | Rebinding the whole receiver between reads (`a = p[0]; p = nxt(); b = p[1]`) is flagged in both languages. | **Defer** | Both languages; needs binding awareness. ROADMAP entry. |
| RS-3 | Med | Grouping by text ignores shadowing: Rust closure param `\|t\| t.1` next to outer `t.0`; same for a Python comprehension variable. | **Defer** | Heuristic limitation in both languages; ROADMAP entry with the example. |
| RS-4 | Med | `min` counts positions, `max` counts placeholders (two units on one config type); `min = 1` flags every `id.0`. | **Reject** | Both are documented in README and the constants; `min = 1` is the legacy linter's legitimate behavior (D6), so no clamp. D16 was validated. |
| RS-5 / PY-6 | Low | Receivers match by exact text: `(t).0`, `len((xs))`, multi-line chains don't match. | **Defer** | Both languages; one normalization step. ROADMAP entry. |
| RS-7 | Low | Rust `v[i].0` is not a valid receiver while Python `rows[i][0]` is; undocumented. | **Fix (doc)** | One sentence in 03 §3.2 (Rust `Index` is a call; D13). |
| PY-3 | Low | `with cm as p[0]:` is not a write. | **Reject** | Very rare. |
| PY-5 | Low | Matrices (`grid[0][0], grid[1][1]`), `Literal[0]`, `df.iloc[0]` are treated as records. | **Reject** | Consistent with D13 / NG1 (no type inference); rule is tagged `Heuristic`. |
| PY-7 | Nit | `p[1,]` counts as a read at 1. | **Reject** | Very rare. |

### Tests

| ID | Sev | Finding | Proposed | Reason |
| :--- | :--- | :--- | :--- | :--- |
| TD-1 | **High** | 5 write pass cases (`p[0] = p[1]`, `+=`, `del`, Rust `+=`, `&mut`) leave a single read, so they pass by D2 even without the exemption. | **Fix** | Give each two surviving reads (`p[0] = p[1] + p[2]`, …). |
| TD-2 | Med | The trailing (negative) branch of `placeholder_count` is untested beyond `{0,-1}`. | **Fix** | Add `xs[0], xs[-3]` (fail) and `xs[0], xs[-4]` (pass). |
| TD-5 / PY-4 | Low | Missing named cases: tuple target `p[0], q = …`, `for p[0] in xs`, `a[p[0]] = p[1]` (still flags), Rust nested `fn` scope, Rust top-level `const` ignored (R6). | **Fix** | 03 §1.2 promises one named case per exemption / scope rule. |
| TD-8 | Nit | Mixed names `*_exempts_receiver` vs `*_receiver_exempt`; `comprehension_reads` vs `closure_groups_with_function`. | **Fix** | Rename to `*_exempts_receiver` / `comprehension_groups_with_function`. |
| PY-9 / TD-11 | Nit | Collector unit test asserts receivers but not positions. | **Fix** | Assert `(receiver, position)` pairs. |
| TD-10 | Nit | Rust `three_placeholders_over_limit` repeats language-agnostic policy. | **Reject** | Harmless smoke test of the Rust path. |

### Code quality

| ID | Sev | Finding | Proposed | Reason |
| :--- | :--- | :--- | :--- | :--- |
| RS-6 | Low | `collection_receivers` only ever holds mutated receivers in Rust; doc lists Python-only uses. `group_record_reads` is vague. | **Fix** | Rename to `exempt_receivers` (doc per language) and `group_reads_by_receiver`. |
| PY-8 | Nit | `.contains(..).then(..).flatten()` is harder to read than `if/else`. | **Fix** | Plain `if`. |
| RS-9 | Nit | `as u64` casts in `placeholder_count`. | **Reject** | Clippy (project config) is clean; values are provably in range. |
| RS-8 | Nit | Rust suggestion could mention `..`. | **Reject** | P3 fixed the suggestion's length. |
| TD-9 | Nit | Rust suggestion fragment "or `&span` when…"; rationale "silently breaks" overstates Rust (removal is a compile error). | **Fix** | "or `let (start, end) = &span;` when…"; "misreads or breaks". |

### Docs

| ID | Sev | Finding | Proposed | Reason |
| :--- | :--- | :--- | :--- | :--- |
| TD-3 | Med | 03 CUJ2, §1.2 metrics, T6 RED, and 02 §2.3 still claim dogfooding catches `jj.rs` L95. | **Fix** | Reword with pointer to 04 §2. |
| TD-4 | Med | README says "in one function" (misses Python module level) and reads as if only the write/slice access is skipped rather than the whole receiver. | **Fix** | Reword. |
| TD-6 | Low | 02 §2.2 / §2.4 name old collector functions and "scopes keyed by node span". | **Fix** | "Superseded by 03 §3.2" note. |
| TD-7 | Nit | 03 §3.2 says groups are "pushed when entered"; they are opened on entry, pushed on exit. | **Fix** | Reword. |

## 2. Out-of-Scope Defects (independent roadmap)

| ID | Finding | Proposed |
| :--- | :--- | :--- |
| OOS-1 | `ThresholdConfig` accepts the `min_args` alias (from `no-identical-positional-types`) for every rule, so `min_args = 3` silently works for `prefer-tuple-unpacking` too. A rule-specific alias leaks into a shared type. | ROADMAP entry. |

## 3. Outcome

* **Behavior fixes, test first.** New cases `enumerate_exempts_receiver`, `default_arguments_in_enclosing_scope`, `swap_assignment_exempts_receiver` were RED (exactly these 3), then GREEN after:
  * PY-1: `COLLECTION_BUILTINS` (`len`, `enumerate`, `zip`, `reversed`, `sorted`) exempt their positional arguments.
  * PY-2: a `function_definition` opens its scope for the `body` field only.
  * RS-1a: `is_mutated` climbs `tuple_expression` / `parenthesized_expression` to an assignment target.
* **Tests.** TD-1 write / `&mut` cases now keep two surviving reads; TD-2 tail placeholder boundary (`tail_placeholders_at_limit` / `_over_limit`); TD-5 named cases (tuple target, `for` target, write-target index, Rust nested `fn`, Rust items outside functions); TD-8 renames; PY-9 collector test asserts `(receiver, position)`.
* **Mutation check.** With the exempt-receiver skip disabled, all 17 exemption pass cases fail, so each one proves its exemption.
* **Code / docs.** RS-6 renames, PY-8, TD-9, RS-7, TD-3, TD-4, TD-6, TD-7 applied as proposed.
* **Deferred.** RS-2b, RS-3, RS-5 / PY-6 and OOS-1 recorded in `ROADMAP.md`.
* **Verification.** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` pass.
