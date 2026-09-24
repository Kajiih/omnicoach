# Phase 1 — Understand

## 1. Goal

Make the crate's abstraction levels **explicit and machine-enforced**, and make rule files the
highest-level, purely declarative layer of `code_lint`.

## 2. Scope (validated answers)

| Question | Decision |
|---|---|
| Work directory | `docs/dev/projects/rule_layering/` (versioned) |
| Rule cleanup (#1) | **All** rule files, including a consistency pass on already-clean ones |
| Layering coverage (#2) | **Whole crate**: `code_lint`, `command_lint`, `core`, `diagnostic`, `rules.rs`, `diff.rs`, `test_utils.rs`, `bin/` — one declared graph |
| Pulled-in roadmap items | Universal Punctuation & Trivia Token Handling; Standardizing Per-Language Dispatch; fix stale ROADMAP §4 multi-violation item |
| Public API stability | None — internal crate, break paths freely, no compatibility shims |
| Enforcement mechanism | Decided in Phase 2 after comparing references |

### Out of scope

- Unified single-pass AST visitor dispatch, performance work, tag system, autofix.
- Behavior changes to any rule (diagnostics must be byte-identical before/after; CLI snapshots are the oracle).

## 3. Preliminary findings (current state, 2026-09-24)

Measured with `rg` over `src/`; to be re-verified with a proper tool in Phase 2.

### 3.1 Dependency inversions and cycles beyond the roadmap

The roadmap lists two inversions. The crate graph shows more:

| # | Edge | Why it violates the intended layering |
|---|---|---|
| A | `ast_python.rs`, `ast_rust.rs` → `crate::code_lint::AstNode` | Upward import of an L1 symbol through the L4/L6 parent (roadmap) |
| B | `code_lint.rs` is both the `CodeRule` contract (L4) and the runner (L6) | `rules/` sandwiched inside a parent that is above and below it (roadmap) |
| C | `core.rs` ↔ `rules.rs` | `core` (L1) imports `crate::rules` (registry, which imports every rule) — **cycle** through the top of the graph |
| D | `core.rs` ↔ `diagnostic.rs` | Mutual imports inside L1 — **cycle** |
| E | `bindings.rs` (L3) → `CodeRule` (L4), `Diagnostic` | Shared engine depends on the rule contract |
| F | `suppression.rs` (L3) → `CodeRule`, `Rule`, `Tag`, `lint_file` (L6) | Engine module also hosts rule/registry concerns; `lint_file` usage to be checked (tests only?) |
| G | `code_lint.rs` → `crate::rules` (registry) | Contract module depends on the registry of its implementors |

### 3.2 Grammar leaks outside `ast_*.rs`

| File | `.kind()` checks | Tree navigation (`children`/`field`/`ancestors`/`dfs`/…) | Kind literals |
|---|---|---|---|
| `rules/no_env_in_functions.rs` | 5 | 7 | 2 |
| `rules/no_assertion_packing.rs` | 4 | 6 | 1 |
| `rules/max_test_assertions.rs` | 2 | 4 | 3 |
| `rules/no_identical_positional_types.rs` | 1 | 3 | 2 |
| `rules/prefer_dedent_for_multiline_strings.rs` | – | 1 | – |
| `rules/flat_scope_enforced.rs` | – | 1 | – |
| `statements.rs` (L3) | – | – | 10 |
| `comments.rs` (L3) | – | – | 1 |
| `calls.rs` (L3) | – | – | punctuation `"(" ")" ","` |

Note: the three rules the roadmap names (`no_uncommented_suppress`, `no_typing_cast`,
`no_mock_assertions`) are **already clean** by these metrics; the real offenders are the four above.

### 3.3 Duplicated per-language dispatch

Nine `match lang { Python => ast_python::f, Rust => ast_rust::f, _ => … }` sites across `bindings.rs`,
`calls.rs`, `comments.rs`, `statements.rs`, `code_lint.rs` (count from ROADMAP, to re-verify).

### 3.4 Stale roadmap item

ROADMAP §4 "Multi-Violation AST Node Aggregation" says `no-identical-positional-types` emits several
diagnostics on one node. The code already consolidates all duplicate groups into **one** diagnostic
per function (`check_function_definition`). The item is outdated.

## 4. Assumptions to verify

1. CLI snapshot tests (`tests/cli.rs`) plus `rule_test!` suites are a sufficient behavioral oracle for a
   pure refactor.
2. `suppression.rs`'s use of `lint_file` is test-only (if so, test modules need a policy, see Q2).
3. No external consumer depends on `pub` paths (confirmed by user).

## 5. Resolved questions

| # | Question | Decision |
|---|---|---|
| Q1 | Are ast-grep pattern strings (`find_all("def $NAME($$$ARGS): $$$BODY")`) a grammar leak? | **Target: no grammar text in rule files**, *if* named helpers keep readability and precision. Phase 2 must show realistic evidence (references + a prototype on one rule); fall back to allowing patterns if helpers hurt readability. |
| Q2 | Do layering rules apply to `#[cfg(test)]` code? | **Yes.** Tests follow the same levels; tests needing higher layers move to the right module or to integration tests. Re-evaluate after practice. |
| Q3 | Are Tree-sitter field names (`.field("name")`) grammar vocabulary? | **Yes**, banned outside `ast_*.rs` like kind literals. |
| Q4 | Placement of `test_utils.rs` and crate-root macros | **Place by dependency direction, no special cases** — see analysis below; final level map decided in Phase 3. |

### Q4 analysis (dependency-driven placement)

- `violation_template!` expands to `ViolationTemplate` (`diagnostic.rs`) → it sits at the level of `diagnostic.rs`.
- `rule_test!` / `test_utils.rs` depend on the rule contract (`CodeRule`), `comments::CommentIndex`,
  `command_lint` types, `core`, `diagnostic`. Rule files' test modules depend on it. With Q2 (tests obey
  levels), it must sit **above the rule contract and below rule files**: a *test harness* level.
- Finding: `test_utils::run_code_rule` **re-implements** the runner's `RequireExplanation` filtering
  because it cannot call the runner (L6) from below. That duplication is a symptom of a missing seam:
  the filtering belongs in a level both the runner and the harness can call (candidate for Phase 3).


## 6. Success criteria

1. A single declared level map covering every module, stated in code (module headers and/or manifest).
2. CI tests fail on: upward/same-level imports, cycles, `ast_python` ↔ `ast_rust` coupling, grammar
   kind literals outside `ast_*.rs` (definition per Q1/Q3).
3. All current violations (§3.1, §3.2) fixed; zero allowlist entries unless explicitly justified.
4. Per-language dispatch standardized (mechanism decided in Phase 3).
5. All tests and CLI snapshots unchanged; clippy clean.
6. `rule_design_guide.md` and ROADMAP updated; stale §4 item fixed.

## 7. Adjacent ideas (not in scope unless you pull them in)

- **Dogfooding**: express some architecture checks as Omni rules run on Omni itself.
- **Suppression as a first-class L3 engine vs. a rule**: §3.1-F suggests `suppression.rs` mixes an
  engine with a rule (`no-uncommented-suppress`-like meta rule); separating them may simplify the graph.
