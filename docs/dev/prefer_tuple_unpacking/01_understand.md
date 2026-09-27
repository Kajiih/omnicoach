# Phase 1: Understand — `prefer-tuple-unpacking`

This document records **Phase 1 (Understand)** for the candidate rule `prefer-tuple-unpacking` (`ROADMAP.md` → Candidate Rules).

> Status: **VALIDATED** (2026-09-27). All decisions D1–D17 validated; no open questions.

---

## 1. Problem Statement

Reading a tuple's elements through literal indices (`point[0]`, `point[1]`) hides the meaning of each position. The reader must remember the internal layout of the value, and a layout change silently breaks every index site.

Sources:

- **Python Tip of the Week #069 `#unpack`** (Python tips #069, copied from Python readability advice#unpack): unpack into named variables (`first_x, first_y = first_point`, `for x, y in points:`); dataclasses or lightweight types are often even better.
- **Python tutorial, Data Structures §5.3**: "Tuples are immutable, and usually contain a heterogeneous sequence of elements that are accessed via unpacking or indexing (or even by attribute in the case of namedtuples). Lists are mutable, and their elements are usually homogeneous and are accessed by iterating over the list."
- **PEP 3132 (Extended Iterable Unpacking)**: `first, *rest = seq` and `a, *b, c = range(5)` make unpacking usable on sequences of unknown length.
- **The legacy linter `IndexingInsteadOfUnpackingRule`** (`scratch/legacy_reference/legacy_lints.py` L1397–1442): warning-level; flags any bare name read with ≥1 integer literal index per scope; one report per name at its first subscript.
- **Existing linters** (preliminary, full survey in Phase 2): no Ruff, Pylint, or Clippy rule enforces unpacking over literal indexing. Pylint `unbalanced-tuple-unpacking` (W0632) only guards unpacking arity.

---

## 2. Rule Principle (What Is Enforced)

> **A value used as a fixed-shape record must have its positions named once, at a single decoding site, instead of being decoded by literal index at each use.**

A read pattern is a violation when all three hold:

| # | Condition | Why it is required |
| :--- | :--- | :--- |
| C1 | **Record decoding**: the same receiver is read at **≥ N distinct literal positions** (default N = 2). | Several positions read by index means the code relies on the value's layout. |
| C2 | **Same value**: every read targets the same value (`pair`, `self.pair`, `rows[i]`; never a call such as `f()`). | Merging the reads into one unpacking is only equivalent if each read would have produced the same value. |
| C3 | **Equivalent, idiomatic fix**: an unpacking pattern exists that reads the same elements and is at least as readable (§2.1). | The rule must never push toward code that breaks or reads worse (design guide §2, §3). |

### 2.1 Idiomatic Boundary: How Far Unpacking Goes

**Correction to round 2.** Round 2 claimed `sys.argv[1]` / `sys.argv[2]` has no valid unpacking. That was only true for exact-arity unpacking (`_, src, dst = sys.argv`). With PEP 3132, `_, src, dst, *_ = sys.argv` reads the same elements and, like indexing, fails when fewer than 3 are present. Python star unpacking works on **any ordered iterable**, so validity is much less restrictive than stated.

| Read pattern | Unpacking | Idiomatic? |
| :--- | :--- | :--- |
| `p[0]`, `p[1]`, exact arity known | `x, y = p` | **Yes, best.** Also validates arity. |
| `argv[1]`, `argv[2]` (prefix of a longer sequence) | `_, src, dst, *_ = argv` | **Yes** (PEP 3132). For `sys.argv` specifically, `argparse` is the canonical tool, but unpacking already improves on indexing. |
| `xs[0]`, `xs[-1]` (head and tail) | `first, *_, last = xs` | **Yes** (PEP 3132 example form). |
| `row[0]`, `row[2]` (one gap) | `a, _, c, *_ = row` | **Yes**: one placeholder. |
| `row[0]`, `row[7]` (many gaps) | `a, _, _, _, _, _, _, h, *_ = row` | **No**: placeholders hide positions worse than indices do. The real fix is a named type (separate rule, D12). |
| `xs[0]` alone | `first, *_ = xs` | **No**: nothing to decode; `xs[0]` is the idiom for "first element". |

Where unpacking is **not equivalent** (Python):

| Case | Why | Handling |
| :--- | :--- | :--- |
| Mapping with int keys (`counts[0]`, `counts[1]`) | Unpacking iterates **keys**, silently changing the result. | D11 |
| Indexable but not iterable (`re.Match`: `m[1]`, `m[2]`) | `a, b = m` raises `TypeError`; the equivalent is `a, b = m.groups()`. | D14 |
| Large collections (`data[0]`, `data[1]` on a million-row list) | `*_` copies the tail: O(n) instead of O(1). | D11 |
| Reads guarded by `except IndexError` | Unpacking raises `ValueError` instead. | NG8 (rare; suppress) |

**Rust** has no star-unpacking on tuples, but needs none: `.0` / `.1` only exist on tuples and tuple structs, whose arity is fixed by the type, so `let (a, b, ..) = t;` is always equivalent.

### Canonical example

```python
# Flagged
for point in points:
    plot(point[0], point[1])

# Fix
for x, y in points:
    plot(x, y)
```

```rust
// Flagged
let span = find_span(node);
emit(span.0, span.1);

// Fix
let (start, end) = find_span(node);
emit(start, end);
```

---

## 3. Goals

| ID | Goal | Reason |
| :--- | :--- | :--- |
| G1 | Flag receivers matching C1–C3 in Python and Rust. | §2 principle; language-agnostic antipattern, language-specific fix (design guide §4). |
| G2 | Suggest one canonical fix per language: unpacking / destructuring, with a named type (dataclass / `NamedTuple` / struct) as the alternative when the tuple crosses a function boundary. | Design guide §2; tip #069 ("dataclasses … often even better"). |
| G3 | Keep false positives low enough to run in CI without mass suppressions. | A noisy rule gets disabled; the legacy linter's `_MIN_INDICES = 1` fires on any `xs[0]`. |
| G4 | Fit the `rule_test!` harness without an opt-out. | Per-scope reporting reports both copies when fail cases are function-scoped. |
| G5 | Configurable thresholds. | Design guide §5. |

## 4. Non-Goals

| ID | Non-goal | Reason |
| :--- | :--- | :--- |
| NG1 | Type inference to prove a Python value is a tuple. | No type information in the pipeline; disproportionate for one rule. |
| NG2 | Autofix. | No autofix engine yet; position names need human choice. |
| NG3 | Functions returning large tuples. | Different antipattern and fix site (design guide §1 split test); §6. |
| NG4 | Multi-field tuple struct definitions (Rust). | Different fix site (the type definition); §6. |
| NG5 | Non-literal indices (`p[i]`) and slices (`p[1:]`). | Not expressible as unpacking. |
| NG6 | Cross-function tracking. | Each scope is judged on its own reads. |
| NG7 | Detecting reassignment of the receiver between reads. | Rare; keeps analysis syntactic. Suppress if it occurs. |
| NG8 | Detecting reads guarded by `except IndexError`. | Rare; suppress if it occurs. |

---

## 5. Decisions

All decisions are **validated**. The last column keeps the review trail.

| ID | Decision | Rationale / review trail |
| :--- | :--- | :--- |
| D1 | Name `prefer-tuple-unpacking` (`prefer-*` family). | — |
| D2 | Trigger C1 at N = 2 distinct positions by default, configurable. | A single read has nothing to decode: `first, *_ = xs` is worse than `xs[0]`. `first, _ = xs` is good only when the arity is known to be exactly two, which the rule cannot know. Teams wanting the legacy linter's behavior can set N = 1. |
| D3 | Exempt writes (`p[0] = x`, `p[0] += 1`, `del p[0]`) and slices. Negative indices are supported via `*_` (`first, *_, last = xs`). | Semantic, not simplification: unpacking only creates new names and cannot write back; slices keep the container type (`t[1:]` is a tuple, `s[1:]` a `str`) while `*rest` is always a `list`. |
| D4 | One diagnostic per `(scope, receiver)`, anchored on the first read in source order. | Coherent with `max-test-assertions` (one diagnostic per function, anchored on one node); no new reporting mechanism. |
| D5 | Reads are grouped per function / method body; module / file top level is one more group. Python `lambda` bodies are skipped. Rust closures group with their enclosing function. | Per-file grouping would combine unrelated same-named receivers (`p[0]` in `f`, `p[1]` in `g`). Python 3 removed parameter unpacking (PEP 3113), so `lambda p: p[0] + p[1]` has no unpacking fix; Rust closures can destructure (`\|(a, b)\|`). |
| D6 | Supersedes the legacy linter `IndexingInsteadOfUnpackingRule`; `_MIN_INDICES = 1` is not reproduced. | — |
| D7 | Languages: Python and Rust. | — |
| D8 | Runs on all files (source and tests), including module / top-level scope. | — |
| D9 | Tags `Style` + `Opinionated` + `Heuristic`. Suggestion: unpack / destructure; named type as the alternative when the tuple crosses a function boundary. | `Heuristic` ("may trigger edge-case false positives", as on `max-test-assertions`): the Python side cannot see types (NG1) and relies on D11. |
| D10 | `self.0` / `self.1` are treated like any receiver. | "Multi-field tuple struct → named fields" is a separate rule (NG4, §6). |
| D11 | **Collection-use exemption (Python), fixed, not configurable.** A receiver is skipped when the same scope also iterates it, passes it to `len()`, indexes it with a non-literal, slices it, writes to it, or calls a mutating method on it. | These uses mark a variable-length or large container (where `*_` copies O(n)) or a mapping (where unpacking iterates keys), not a record. Not configurable: rule configs are numeric thresholds and name lists, so a toggle would add a config type for one rule and double the test matrix; it only removes diagnostics, and suppression covers the rare record that is also iterated. Revisit if real-world runs show it hides true positives. |
| D12 | **Placeholder limit**: this rule flags only when the unpacking pattern needs ≤ 2 `_` placeholders (default, configurable). `row[0]`, `row[3]` is flagged; `row[0]`, `row[4]` is left to the separate named-record rule (§6). | Beyond a few placeholders the fix is no longer unpacking but a named type: a different fix, so a different rule (design guide §1 split test). The limit is the boundary between the two rules. |
| D13 | Two reads hit the same value (C2) when their receiver source text is identical. Receivers are names, attribute / field access, and subscripts (`pair`, `self.pair`, `sys.argv`, `rows[i]`); receivers containing a call are skipped. | `get_pair()[0]` / `get_pair()[1]` are two calls that may return different values; `a, b = get_pair()` would change behavior by calling once. |
| D14 | `re.Match` group indexing (`m[1]`, `m[2]`) is not special-cased; roadmap. | Detecting `m = re.match(...)` needs binding tracking this rule does not otherwise need. Suppress meanwhile. |
| D15 | Rust `[]` indexing (`arr[0]`, `arr[1]`) is excluded; roadmap. | Same syntax for arrays, `Vec`, slices, and any `Index` impl; only arrays get the clean `let [a, b] = arr;`. |
| D16 | Configuration reuses `ThresholdConfig` (`src/core.rs`): `min` = distinct positions (D2, default 2), `max` = placeholders (D12, default 2). | No new config type; same TOML shape as `max-test-assertions` and `no-identical-positional-types`. The rule docs must state what each bound measures. |
| D17 | No harness opt-out: fail cases are function-scoped, so the repeated-occurrence check sees two scopes. | G4. |

---

## 6. Related Ideas (Out of Scope, for Roadmap)

- **Named record for wide / sparse positional access** (D12): reads needing more placeholders than D12 allows (`row[0]`, `row[7]`) → `NamedTuple` / dataclass / struct (or `csv.DictReader` for CSV rows).
- **Tuple-returning functions** (NG3): functions returning tuples of ≥3 elements → `NamedTuple` / dataclass / struct.
- **Multi-field tuple structs** (NG4): tuple structs with ≥2 fields → named-field struct (tuple structs reserved for newtypes).
- **`re.Match` group indexing** (D14): `m[1]`, `m[2]` → `a, b = m.groups()`.
- **Rust slice patterns** (D15): `arr[0]`, `arr[1]` on fixed-size arrays → `let [a, b] = arr;`.
- **`rule_design_guide.md` §6 line 62**: once this rule lands, the note naming the legacy linter `IndexingInsteadOfUnpackingRule` as needing a harness opt-out becomes obsolete.
