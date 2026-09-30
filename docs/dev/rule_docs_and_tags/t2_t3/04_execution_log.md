# Phase 4 — Execution Log & Comparative Evaluation (T2/T3)

## 1. Prototype Construction & Verification

Two isolated, functional prototypes were built in dedicated jujutsu workspaces branched from commit `wrsupvvo`:
- **PA (Minimal)** (`scratch/poc_pa`, commit `yqtummxy`): Single-sentence `what_it_does`, hand-listed configuration string slices, docs-only `--explain`.
- **PB (Derived)** (`scratch/poc_pb`, commit `nnptxvms`): Dedicated single-sentence `summary` + multi-sentence `what_it_does`, `ConfigShape` enum with derived keys, status reporting in `--explain`.

Both prototypes implemented the shared T2/T3 baseline:
- CLI discovery surface (`--list-rules [--tag <label>]`, `--explain <rule>`) on both `omni-code-lint` and `omni-command-lint` (DI8, DI9).
- Shared `OutputFormat` enum (`Plain`, `Json`) replacing stringly `--format` (D51).
- One-line plain diagnostics footer (`For details on a rule, run: <binary> --explain <rule>`) (D48, DI14).
- Terminal Markdown rendering without ANSI escapes (A8, D50).
- 4 representative rules documented (`no-sleep-in-tests`, `max-test-assertions`, `missing-suppression-reason`, `no-edits-on-described-commits`) and 22 rules with `RuleDoc::TODO` placeholder.

### Verification Results

| Metric | PA (Minimal) | PB (Derived) | Notes |
| :--- | :--- | :--- | :--- |
| **Unit tests** | 603 passed, 0 failed | 611 passed, 0 failed | PB adds unit tests for `RuleStatus` and `ConfigShape` |
| **Architecture tests** | 15 passed, 0 failed | 13 passed, 0 failed | Both enforce full DAG boundaries |
| **CLI integration tests** | 26 passed, 0 failed | 26 passed, 0 failed | 100% snapshot coverage of discovery surfaces |
| **Doctests** | 8 passed (incl. 7 compile_fail) | 8 passed (incl. 7 compile_fail) | Verifies taxonomy compile-time invariants |
| **Self-dogfooding** | Clean | Clean | Caught `err` abbreviation during development |
| **Clippy & rustfmt** | Zero warnings (`-D warnings`) | Zero warnings (`-D warnings`) | Clean nursery & pedantic checks |
| **Code delta (lines)** | +1020 / -101 (36 files) | +1287 / -109 (53 files) | Diff against parent `wrsupvvo` |

---

## 2. Comparative Evaluation of Decision Items

### DI10: Summary Line (`what_it_does` vs separate `summary`)
- **PA Implementation:** Uses `what_it_does: &'static str` as a single sentence ending with a period. Used for both `--list-rules` bullets and the `## What it does` section in `--explain`.
- **PB Implementation:** Adds `summary: &'static str` (single sentence for `--list-rules`) and allows `what_it_does: &'static str` to be a detailed multi-sentence explanation.
- **Evaluation:**
  - In PA, rule authors are forced into a harsh tradeoff: either `--list-rules` becomes an unwieldy wall of text, or the `## What it does` section in `--explain` lacks essential scope details (such as which specific functions are flagged, what exceptions exist, or how zero-duration sleeps are handled).
  - In PB, `--explain no-sleep-in-tests` clearly lists the covered standard library functions (`time.sleep`, `tokio::time::sleep`, etc.) and explicitly notes that injected clock objects (`clock.sleep`) are permitted. Meanwhile, `--list-rules` stays crisp and scannable.
  - **Decision: PB wins.** A separate `summary` field is essential for clear, high-signal documentation without compromising list readability.

### DI11: Configuration Keys (`&[&str]` vs `ConfigShape`)
- **PA Implementation:** Rules declare `configuration: &'static [&'static str]`, e.g. `&["banned", "extend_banned", "allowed"]`. Validated in test by reflecting over serde JSON values of the 4 config structs.
- **PB Implementation:** Rules declare `configuration: &'static [ConfigShape]`, e.g. `&[ConfigShape::DenyList]`. Key names are derived statically.
- **Architectural Discovery:** `RuleDocumentation` is a foundational primitive (`FoundationPrimitives`) and cannot depend on `CoreVocabulary` (`core`). In PB, `ConfigShape` lives in `RuleDocumentation`, while the key resolution function `shape_keys(shape: ConfigShape) -> &'static [&'static str]` is placed in `RuleCatalog` (which depends on both `RuleDocumentation` and `CoreVocabulary`).
- **Evaluation:**
  - PA requires every rule author to re-type standard option names and maintain exact casing/spelling. While tested, it introduces avoidable repetition across rules.
  - PB provides a type-safe enum. Rules cannot have typos in their declared configuration shapes, and changes to a config shape automatically reflect across all rules using it.
  - **Decision: PB wins.** Strongly-typed `ConfigShape` with catalog-level key derivation maintains architectural isolation while eliminating repetition and typo risk.

### DI12: Adoption Strategy (`TODO` placeholder vs full content pass)
- **Both POCs:** Proved that `RuleDoc::TODO` allows immediate introduction of the documentation architecture without blocking on authoring 26 long-form rationale texts.
- **Evaluation:** The style linter test skips placeholder rules (`!rule.doc.is_placeholder()`), while ensuring all documented rules strictly adhere to formatting and heading conventions. Full documentation for remaining rules can proceed atomically or in subsequent PRs.
- **Decision: Settled.** Adopt `RuleDoc::TODO` placeholder with 4 fully documented initial rules.

### DI13: Active Status in `explain`
- **PA Implementation:** Docs only. `--explain` prints purely static metadata.
- **PB Implementation:** Evaluates loaded `.omnilint.toml` (without requiring a specific path) and prints:
  `Status: enabled (default)`
  or `Status: enabled by select = ["testing"] (via testing > test-timing)`
  or `Status: disabled by ignore = ["test-doubles"]`
- **Evaluation:**
  - The implementation in `rule_selection.rs` required ~230 lines of code to retain selector provenance in `Plan` decisions and format the resolution trail.
  - User feedback from CUJs shows that "Why is this rule on/off?" is one of the most frequent friction points when configuring linters. Displaying the winning selector and branch inheritance (`via testing > test-timing`) directly in `--explain` provides immediate clarity.
  - **Decision: PB wins.** The status output is compact, highly ergonomic, and reuses the existing selection engine cleanly.

---

## 3. Recommended Unified Model

The recommended final design merges the best elements into a clean, idiomatic architecture:
1. **RuleDoc structure:**
   - `summary: &'static str` (one sentence, period-terminated).
   - `what_it_does: &'static str` (multi-sentence Markdown explaining behavior and exemptions).
   - `why_is_this_bad: &'static str` (authoritative rationale).
   - `configuration: &'static [ConfigShape]` (strongly-typed configuration shapes).
   - `references: &'static [Reference]` (external links).
2. **Configuration derivation:** `shape_keys` mapped in `RuleCatalog` to keep `RuleDocumentation` purely foundational.
3. **Status surface:** Global on/off status and provenance displayed in `--explain <rule>`.
4. **Shared flags & footer:** `--list-rules`, `--explain`, and `explain_footer` shared across all Omni binaries.
