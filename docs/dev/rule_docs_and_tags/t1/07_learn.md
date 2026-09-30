# Phase 7: Learn — Tag Taxonomy & Selection (T1)

Process and principles only; [ADR 007](../../../../decisions/007_rule_taxonomy_and_selection.md), [tag_guide.md](../../tag_guide.md), the P0 reference prototype (`scratch/tag_poc/p0_typed_facets`), and docs `01`–`06` hold the technical details.

**Status**: Validated by User.

---

## 1. Taxonomy & Vocabulary Design

1. **Define a concept by a repeatable yes/no test before naming it.**
   - Early iterations debated names (`disposition`, `basis`, `opinion`, `confidence`, `quality`) while the underlying boundary was still fuzzy. In every case, reviewer disagreement came from an underspecified boundary, not the label. Once a facet had a one-sentence operational test and explicit exclusions (e.g. the counterexample test for `opinionated`, design proxy vs implementation bug for `heuristic`, and mechanism-for-wrong-behaviour for `reliability`), both classification agreement and the right display name followed naturally.
2. **Test candidate definitions blind on the real corpus before choosing.**
   - Running two independent blind reviewers across the candidate definitions on all 26 rules (E1) immediately falsified definitions that looked plausible in the abstract (such as "beyond an external baseline", which classified 26/26 rules as opinionated) and exposed exact boundary ambiguities where reviewers split. Reading every rule against a strict test also surfaced seven pre-existing rule/metadata bugs (`S1`–`S7`) as a by-product.
3. **Decouple display labels from config selectors.**
   - Forcing one word to serve as both a terse config token and a self-explanatory documentation heading created unnecessary tension. Making facet names display-only labels (`Impacted quality`, `Analyzed input`, `File scope`) while keeping facet *values* as the only selectors let documentation use explicit multi-word phrases and kept the selector vocabulary flat and unambiguous.

## 2. Prototyping & Architecture

4. **Measure competing designs against one intent-fixed harness, then harvest across prototypes.**
   - Writing the representation-agnostic CUJ harness first (`03_design_plan.md` → `scratch/tag_poc/harness`) and building five competing prototypes (`P0`–`P4`) in parallel turned abstract design debates into concrete evidence. Even rejected prototypes contributed directly to the final design: `P4` motivated compile-time tree cycle checking (`D34`), and `P3` demonstrated by construction why expression-based selection breaks rule-name overrides (`D15`).
5. **Turn "do not read X" conventions into structural boundaries.**
   - Leaving `M13` ("rule and runner behaviour must not depend on tags") as a code-review convention left one mis-tag class uncaught. Moving classifications out of the `Rule` trait and having `RuleSelection` resolve selectors into tag-free `RuleName` sets on `core::Config` made the wrong dependency unrepresentable in rule/runner code and enforceable by `tests/architecture_conformance.rs`.
6. **Prefer plain `const` struct literals over `enum` + `match` tables or custom macros when exhaustive branching is unnecessary.**
   - Modeling open-ended metadata registries (such as `Topic`) as an `enum` forced splitting each entry across variant declarations and a `match` table, or hiding that split behind a `macro_rules!`. Because nothing in the system branches on individual topics (`D37`), a plain `struct Topic` with associated `const` literals keeps every field (`label`, `parent`, `description`, `scope_note`, `synonyms`) in one named-field literal, eliminates custom macros, and gets cycle prevention (`E0391`) and unused-item checks (`dead_code`) from `rustc` for free (`D32`, `D34`).

## 3. Review & Maintenance

7. **Sweep prior phase docs when later checkpoints refine a decision, and filter review findings against speculative complexity.**
   - Multi-checkpoint exploration cycles naturally evolve terminology (`detection` → `precision`, `Quality` → `Impacted quality`, `M12` dropped). Three parallel reviewers with disjoint scopes (architecture/ADR, tag guide/classifications, prototype code) caught both real boundary gaps (`R1`–`R5`) and residual terminology drift across earlier docs (`R6`, `R9`), while the anti-speculation filter rejected adding test-only public API helpers that were already enforced at compile time.
