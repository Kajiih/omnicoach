# Phase 5: Clean Up — `repeated-literal`

Records **Phase 5 (Clean up)**: a pass over every file the change touched (`jj show --stat lrvsuokl`), looking for outdated, dead or legacy code, docs and artifacts.

---

## 1. Checklist

- **Dead code / unused imports**: none. `cargo clippy --all-targets -- -D warnings` is clean.
- **Leftovers**: none.
  - The temporary `examples/probe_literals.rs` is deleted, and so is the now-empty `examples/` directory.
  - The scratch fixture is deleted.
  - There are no `.snap.new` / `.pending-snap` files.
- **Backward compatibility**: none added. `rule_test!` keeps its existing arms. `repeat:` is optional and defaults to `SameCode`, so every other rule is unchanged.
- **Stale references to the old name `no-repeated-literals`**: they remain only in historical phase docs (`01_understand.md`, `prefer_tuple_unpacking/03_design_plan.md` P2). Those documents record past decisions, so they are kept as is; 01–03 carry a banner pointing to the phases that supersede them.
- **Rule catalog**: `--list-rules` builds it from `CODE_RULES`, and `cli__list_rules.snap` is updated. No hand-maintained list needs an entry.
- **Formatting / dogfooding**: `cargo fmt --check` and `test_self_dogfooding_code_lint` pass.

## 2. Fixed

- **`ROADMAP.md`, "Validated Tree-sitter node kinds"**: it named `statements.rs` as one of the opted-out files. It now names `ast.rs`, `ast/python.rs` and `ast/rust.rs`, the files that actually carry `omni:disable-file` (see 04 §2).
- **Doc comments on the T5 constants**: `CALLEE` in `sleep_in_tests.rs` and `JJ_BINARY` / `GIT_BINARY` in `diff.rs` had none. The four planned dogfood files document every extracted constant, so these now do too.

## 3. Left for Phase 6 (review, not cleanup)

- **The Python suggestion** always appends the `case`-pattern advice, even for a literal that is not in a `case` (raised at the Phase 4 gate).
- **`changed_digit` in `test_utils.rs`** has a `_ => digit` arm that the callers never reach. If it were reached, the "nothing changed" validation would catch it.
- **`LiteralGroup::flagged_uses` clamps with `min_occurrences.max(1)`**: with `min-occurrences = 0` or `1` and no constant, every repeat after the first is flagged, never the first itself. This is consistent with D7, but worth a reviewer's look.
