# T1 `impl/` — Plan

Ports the validated P0 design ([ADR 007](../../../../../decisions/007_rule_taxonomy_and_selection.md), [tag_guide.md](../../../tag_guide.md)) from `scratch/tag_poc/p0_typed_facets` into `src/`. Short cycle: plan → execute → review. D35 (shadowed-selector warning) stays on the roadmap.

## 1. Changes (one `jj` change each)

| # | Change | Verify |
|---|---|---|
| C1 | **Separate suppression audits.** The four audits implement only `Rule` and move to `SUPPRESSION_AUDITS` in `code_lint::suppression`. `CODE_RULES` holds AST rules only, so the runner never branches on `Tag::Suppression`. | Existing runner and suppression tests pass unchanged in intent. |
| C2 | **Add `RuleTaxonomy` and classify every rule.** New `src/rule_taxonomy.rs` (`Topic`, `Precision`, `Consensus`, `ImpactedQuality`, `Classification`, `ClassifiedRule`). Each rule declares an inherent `CLASSIFICATION` const. `CODE_RULES`, `SUPPRESSION_AUDITS` and `COMMAND_RULES` become `&[ClassifiedRule<dyn …>]`, so an unclassified rule cannot be registered (R3). The 26 classifications are copied from P0 unchanged. | `compile_fail` doctests (M1, M2, M5–M8, M11); all tests pass. |
| C3 | **Add `RuleSelection`; remove `Tag`.** New `src/rule_selection.rs` (+ private child `taxonomy`): derived facets, labels and synonyms, precedence model B, config adapter with loud errors. It resolves every selector at load into tag-free `RuleName` sets on `core::Config` (`disabled_rules`, `per_file_ignores`). `Tag`, `Selector`, `Rule::tags`/`has_tag` and `Config::load` leave `core`; binaries call `rule_selection::load_config`. | Taxonomy invariant tests, model B cases from the CUJ harness, error-message tests, `architecture_conformance` (new graph edges + runners must not reference `rule_taxonomy`). |
| C4 | **Docs.** README config example, tag guide (§2 example, §6 test locations), ADR 007 status and verification paths, ROADMAP. | Links and examples match the code. |

## 2. Decisions taken in this cycle

- **I1 — Registration as a pair struct, classification as an inherent const.** `ClassifiedRule { rule, classification }` is plain Rust and gives one registration list per domain (no macro). The classification is `impl NoSleepInTests { pub(crate) const CLASSIFICATION … }`, so it is colocated with the rule but not reachable through `dyn Rule`.
- **I2 — Runners cannot name the taxonomy.** The DAG lets runners reach `RuleTaxonomy` transitively (through the rule registries), so the conformance test adds an explicit rule: `code_lint::runner` and `command_lint::runner` must not reference `rule_taxonomy` or `rule_selection`.
- **I3 — I1–I3 facades wait for T3.** `describe_rule`, `describe_tag` and `explain` provenance have no consumer before T3. The planner computes only the outcome; the P0 prototype keeps the provenance reference implementation.
- **I4 — Topics are `pub(crate)` consts** so that an unused topic fails `dead_code` (M10), as the ADR states.
