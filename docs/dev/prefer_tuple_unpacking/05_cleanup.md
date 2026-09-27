# Phase 5: Clean Up — `prefer-tuple-unpacking`

Records **Phase 5 (Clean up)**: a pass over every file the change touched (`jj show --stat wnnynxwm`) for outdated, dead, or legacy code, docs, and artifacts.

---

## 1. Checklist

- **Dead code / unused imports**: none. `cargo clippy --all-targets -- -D warnings` clean; the T1 stubs were fully replaced; no `examples/cst_probe.rs` or `.pending-snap` leftovers; working copy clean.
- **Backward compatibility**: none added (new rule, new facade, no renamed APIs).
- **Template `base` texts**: unreachable (both supported languages override), but kept: the `violation_template!` macro requires `base` when overrides exist, and every other rule follows the same shape.
- **Other catalogs**: only `README.md` and `rules.rs` list rules; both updated.
- **Formatting / dogfooding**: `cargo fmt --check`, `test_self_dogfooding_code_lint` pass.

## 2. Fixed

- `02_resources.md` R8 rationale claimed the dogfooding test would catch `jj.rs` L95; corrected with a pointer to 04 §2.

## 3. Left for Phase 6 (review, not cleanup)

- `placeholder_count` goes through `u64` and `usize::try_from`; correct, but a reviewer may prefer plain `i64` / `usize` arithmetic.
- `group_record_reads` naming ("record" = the tuple as a record) may read ambiguously.
