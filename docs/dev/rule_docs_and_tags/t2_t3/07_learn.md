# Phase 7 — Learnings & Next Steps (T2/T3)

## 1. Key Retrospective Learnings

The T2/T3 exploration cycle evaluated the in-tree rule documentation model and discovery CLI surfaces through research, design planning, and two fully functional competing prototypes (PA — Minimal, PB — Derived).

### 1. Architectural Boundaries Must Be Validated Concretely
- In PB, deriving configuration keys from config shapes initially tempted placing `keys()` on `ConfigShape` in `RuleDocumentation`.
- The architecture conformance test immediately flagged that `RuleDocumentation` (`FoundationPrimitives`) was importing `core::*Config` (`CoreVocabulary`), violating the clean acyclic dependency model.
- The solution was simple and clean: `RuleDocumentation` remains a pure foundational primitive with the `ConfigShape` enum, while key derivation is performed in `RuleCatalog` (which depends on both `RuleDocumentation` and `CoreVocabulary`).
- *Takeaway*: Running actual architecture conformance tests against competing prototypes catches DAG violations early before they reach production.

### 2. High-Signal CLI UX Requires Two-Tier Summaries
- PA attempted to use `what_it_does` as a single sentence for both `--list-rules` and the `## What it does` section in `--explain`.
- In practice, this forced rules into a poor compromise: either the rule list became verbose and unreadable, or `--explain` lost critical operational details (e.g., standard library functions covered, zero-duration sleep exceptions, injected object exemptions).
- PB demonstrated that having a strict 1-sentence `summary` alongside a multi-sentence `what_it_does` produces both a scannable catalog list and comprehensive, high-signal rule documentation.

### 3. Explaining Active Rule Status Solves the Primary Linter Friction
- Users configuring linters frequently ask: *"Why did this rule flag?"* or *"Why is this rule not running?"*
- Implementing `Status: enabled (default)` or `Status: enabled by select = ["testing"] (via testing > test-timing)` inside `--explain <rule>` required only ~230 lines of code in `rule_selection.rs`, directly reusing the existing `Plan` resolution engine.
- Integrating status directly into `--explain` provides an instant answer without requiring separate commands or verbose configuration debugging flags.

### 4. Simplicity First: Plain Markdown Over ANSI Escapes
- Directly outputting clean Markdown without ANSI escape sequences proved robust across standard terminals, pagers (`less -R`), CI logs, and programmatic callers.
- Avoiding bespoke terminal styling libraries kept binary footprints minimal and eliminated terminal capability bugs.

### 5. Incremental Adoption via Placeholders
- Introducing `RuleDoc::TODO` decouples the documentation architecture from writing 26 comprehensive rationale texts in one go.
- Strict style lint tests validate all documented rules without breaking builds for placeholder rules, enabling clean, atomic progress.

---

## 2. Next Steps

With ADR 008 formalized and the exploration cycle complete, the transition to production code begins:
1. **Implementation Plan (`impl/01_plan.md`)**:
   - Establish an atomic, staged execution plan for rolling out ADR 008 into `src/`.
2. **Phase 1 Implementation (Foundation & Models)**:
   - Introduce `RuleDoc`, `ConfigShape`, `Reference` in `src/rule_documentation.rs`.
   - Update `RuleEntry` and rule registries with `doc: RuleDoc`.
3. **Phase 2 Implementation (Catalog & Discovery Engine)**:
   - Implement `src/rule_catalog.rs` and status resolution in `src/rule_selection.rs`.
   - Wire discovery flags and diagnostics footer into `src/bin/*.rs`.
   - Upgrade `--format` to typed `OutputFormat`.
4. **Phase 3 Implementation (Initial Docs & Testing)**:
   - Document the 4 initial representative rules (`no-sleep-in-tests`, `max-test-assertions`, `missing-suppression-reason`, `no-edits-on-described-commits`).
   - Add registry doc style tests, architecture DAG updates, and CLI snapshot tests.
   - Remove hand-maintained catalog from `README.md`.
