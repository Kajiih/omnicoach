# T2/T3 `impl/` — Production Implementation Plan

This plan operationalizes the validated design from [ADR 008](../../../../../decisions/008_rule_documentation_and_discovery.md) into production `src/`. Execution proceeds in 4 atomic, verifiable changes (one `jj` commit each).

---

## 1. Implementation Steps (One `jj` Change Each)

| Step | Change | Deliverables | Verification |
| :--- | :--- | :--- | :--- |
| **C1** | **Add `RuleDocumentation` and wire into `RuleTaxonomy` / `RuleEntry`** | • `src/rule_documentation.rs` (`RuleDoc`, `Reference`, `ConfigShape`, `RuleDoc::TODO`)<br>• `src/architecture.rs` (`RuleDocumentation => [FoundationPrimitives]`, `RuleTaxonomy => [FoundationPrimitives, RuleDocumentation]`)<br>• `RuleEntry<R>` in `src/rule_taxonomy.rs` includes `pub doc: RuleDoc`<br>• Colocate `DOC = RuleDoc::TODO` on all 26 rules across registries | `cargo check`<br>`cargo test --test architecture_conformance`<br>`cargo clippy --all-targets -- -D warnings` |
| **C2** | **Add `RuleCatalog` and status resolution in `RuleSelection`** | • `src/rule_selection.rs`: retain decision provenance (`RuleStatus`, `load_rule_status`, `rules_tagged`, `find_rule`)<br>• `src/rule_catalog.rs`: Markdown renderer (`list_rules`, `explain`, `shape_keys`, `explain_footer`)<br>• `src/architecture.rs`: `RuleCatalog => [RuleSelection, RuleDocumentation, CoreVocabulary]`<br>• Tests for `doc_problems`, `shape_keys`, and status provenance | `cargo test`<br>`cargo test --test architecture_conformance`<br>`cargo clippy --all-targets -- -D warnings` |
| **C3** | **Wire discovery surfaces into CLI binaries and upgrade `OutputFormat`** | • `src/diagnostic.rs`: `OutputFormat` ValueEnum (`Plain`, `Json`)<br>• `src/bin/*.rs`: add `#[command(flatten)] discovery: DiscoveryArgs`, typed `format`, plain footer<br>• `tests/cli.rs`: snapshots for `--list-rules`, `--tag`, `--explain`, typo suggestions, missing cmd, help flags | `cargo test --test cli`<br>`cargo test` (all 600+ tests)<br>Self-dogfooding passes |
| **C4** | **Initial rule documentation content & README update** | • Full `RuleDoc` for 4 representative rules (`no-sleep-in-tests`, `max-test-assertions`, `missing-suppression-reason`, `no-edits-on-described-commits`)<br>• Invariant test `documented_rules_follow_the_doc_style`<br>• Remove hand-maintained catalog from `README.md` and direct users to `--list-rules` / `--explain` | `cargo test`<br>`cargo clippy --all-targets -- -D warnings`<br>`cargo fmt --check` |

---

## 2. Invariants & Guardrails

- **Zero Macro Expansion for Regular Code**: Plain Rust structs and consts; no macro magic.
- **Strict Architecture DAG Compliance**: `RuleDocumentation` stays in `FoundationPrimitives` without depending on `CoreVocabulary`. Key derivation is isolated in `RuleCatalog`.
- **Compile-Time Completeness**: Omitting a documentation section or config shape from a rule fails compilation (`E0063`).
- **No Orphan Warnings**: All clippy nursery and pedantic lints pass with `-D warnings`.
