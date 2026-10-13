# Return contracts: 01 Understand

> **Status: VALIDATED** (lead & user, 2026-10-13). Scope: three rules designed together as one batch: `positional-tuple-return`, `non-boolean-predicate` and `unexplained-return`. They replace `undocumented-tuple-return`.

Markers: **[V]** verified in code, corpus or a primary source; **[U]** not verified.

## 1. Problem

A function's return type is the part of its contract a caller reads first. Three ways it misleads:

1. **Positional tuples.** `tuple[str, int]` / `(PathBuf, PathBuf)` states element types by position, not what each position means. Two same-typed positions swap silently. `undocumented-tuple-return` asked for a `Returns:` section, which documents the problem instead of removing it: a `NamedTuple`, a dataclass or a struct puts the field names in the type, where callers and type checkers see them.
2. **Predicate names that do not return a boolean.** `is_active() -> Option<ContextAge>`, `has_report() -> Option<u32>`, `is_implicit_override() -> Option<bool>`. Callers write `if is_x(...)`; in Python a `0` id or a `None` third state reads as `False`.
3. **Non-obvious results of nontrivial functions.** A nullable, boolean, union or unannotated result does not say what each outcome means (when `None` is returned, what `True` means for an action). `03c_section_requirements.md` (buckets G and H) found these were the returns that needed a description; a self-describing domain type did not.

## 2. Why one batch

The three rules read the same structural fact, a function's return contract, and must partition it:

- A positional tuple is `positional-tuple-return`'s finding only: `unexplained-return` never asks to document a tuple (that was `undocumented-tuple-return`'s band-aid).
- A `bool` result of a predicate-named function is obvious for `unexplained-return`, and the same predicate-name test decides `non-boolean-predicate`.
- `Option` / `| None` around a tuple is both a tuple (structural) and a nullable result (when `None` is returned); the two findings ask for compatible fixes.

## 3. Evidence

- **Tuple returns** [V]: `src/` had 26 Rust functions returning 2-tuples and 3 returning 3-tuples; `cargo/src/` returns `(PackageId, PackageId)`, `(PathBuf, PathBuf)`, `(Vec<Dependency>, Vec<Dependency>)`, `(String, String)`. Effective Python (Slatkin) Item 19 recommends a dataclass beyond three unpacked values; same-typed pairs are the swap hazard at any arity.
- **Predicates** [V]: in `src/`, every `fn (is|has|can|should)_*` returns `bool`. In `cargo/src/`, 6 do not: 4 return `CargoResult<bool>` / `Option<bool>` and 2 return a value (`has_report -> Option<u32>`, `is_active -> Option<ContextAge>`, plus `is_conflicting -> Option<usize>`). In `cpython/Lib/`, every annotated predicate returns `bool` except `TypeGuard[...]` / `TypeIs[...]` (documentation examples in `typing.py`). The 12 prefixes (§3 of `03_design.md`) matched no false positive in the three corpora.
- **`Result<bool, E>`** [V]: `std::fs::exists -> io::Result<bool>` exists because `Path::exists -> bool` turns permission errors into `false`. A fallible predicate is idiomatic Rust; an optional one (`Option<bool>`) is a three-state value.
- **Returns needing a description** [V]: `03c_section_requirements.md` F9 (buckets G, H): warranted cases were values-or-`None`, path-or-message unions, ambiguous booleans and unannotated values; one-line action summaries over a domain type were not.
