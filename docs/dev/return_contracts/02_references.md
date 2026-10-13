# Return contracts: 02 References

> **Status: VALIDATED** (lead & user, 2026-10-13).

Markers: **[V]** verified in a primary source; **[U]** not verified.

| Source | What it says or checks | Gap these rules fill |
| :--- | :--- | :--- |
| Effective Python (Slatkin), Item 19 "Never Unpack More Than Three Variables When Functions Return Multiple Values" [V, title and advice] | Return a lightweight class (`@dataclass`) instead of a long tuple; same-typed values swap silently. | No linter enforces it; `positional-tuple-return` applies the same reasoning from two elements, where same-typed pairs already swap. |
| Rust `std::fs::exists` [V] | Returns `io::Result<bool>`: `Ok(true)` / `Ok(false)` only when existence is verified, `Err` otherwise, unlike `Path::exists`. | Basis for accepting `Result<bool, E>` as a boolean return in `non-boolean-predicate`. |
| PEP 647 `TypeGuard`, PEP 742 `TypeIs` [V, typing docs] | User-defined type predicates return `TypeGuard[T]` / `TypeIs[T]`, which are booleans to callers. | Accepted as boolean returns in `non-boolean-predicate`. |
| Rust API Guidelines, Naming [V] | Covers `as_` / `to_` / `into_` conversions and getters; says nothing about `is_` / `has_` predicates. | No Rust guideline or Clippy lint checks predicate return types [U for Clippy: `wrong_self_convention` checks the receiver of `is_*` methods, not the return type]. |
| Ruff / Pylint [U] | No rule for tuple returns or predicate return types. Ruff `DOC201` (preview) requires `Returns:` in docstrings that already have sections. | `unexplained-return` asks for a description only when the result is non-obvious, including one-line docstrings that `DOC201` skips. |
| `docs/dev/complex_function_docstring/03c_section_requirements.md` [V] | Corpus study of when `Returns:` is warranted (F9, P5, P7). | Supplies `unexplained-return`'s triggers; tuple results move to `positional-tuple-return`. |
| Google Python Style Guide §3.8.3 [V] | `Returns:` may be omitted when the docstring starts with "Returns" or "Yields" and the sentence suffices. | Kept as `unexplained-return`'s summary exemption. |
