# Phase 5: Clean Up — Tag Taxonomy (T1)

This document records **Phase 5 (Clean up)** of the T1 exploration cycle.

**Status**: Complete.

---

## 1. Superseded Documents Removed or Folded

| Item | Action | Reason |
|---|---|---|
| `docs/dev/tag_system_analysis.md` | **Deleted** after preserving §10 ("Manual notes") verbatim in [01_understand.md §1.1](01_understand.md) | §§1–9 were early 17-rule drafts superseded by `01`–`04` and [tag_guide.md](../tag_guide.md); D11 designated only §10 as authoritative |
| [ROADMAP.md §5](../../../ROADMAP.md) | **Updated** to point to `docs/dev/rule_docs_and_tags/` and `docs/dev/tag_guide.md`, and to record the T1 `impl/` scope, T2, T3 (`explain` branch `via` provenance, D36), and deferred items (D35, D7/NG2, NG5) | Removed stale link to `tag_system_analysis.md` and outdated "17 rules / nothing decided" text |

---

## 2. Prototype Workspace (`scratch/tag_poc/`) Cleaned Up

- **Losing prototypes (`p1_flat_presets`, `p2_group_side`, `p3_label_expressions`, `p4_polyhierarchy`) removed** from the workspace after archiving their per-prototype `REPORT.md` files in `scratch/tag_poc/archive/` (and recording their full comparative metrics and lessons in [04_execution_log.md §6.2–§6.3](04_execution_log.md)).
- **Kept prototype (`p0_typed_facets`) aligned with Checkpoint 6 & Phase 7 decisions (D32–D38)**:
  - `Quality` renamed to `ImpactedQuality` (`impacted_quality`, display label `"Impacted quality"`, D38).
  - `Topic` simplified from `enum Topic` + `TopicDef` (2 spots) to a plain `struct Topic` with associated `const`s (1 spot, zero macros, `M8` cycles caught natively at compile time with `E0391`, D32/D34).
  - `MAX_DEPTH` and `topic_depth_is_at_most_three` removed (D34).
- **Verification**: `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` (25 unit/integration tests + 8 doctests including 7 `compile_fail` checks) pass in `scratch/tag_poc/`.
