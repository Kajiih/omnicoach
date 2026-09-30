# Phase 6: Review & Architectural Conformance — Rule Docs & Discovery (T2/T3)

This document records **Phase 6 (Review & Audit)** of the T2/T3 exploration cycle.

## 1. Verification of Prototypes & Evaluation

The two competing prototypes (PA — Minimal, PB — Derived) were audited across correctness, compiler diagnostics, architectural conformance, and CLI UX:
1. **Compilation & Clippy:** Both prototypes compiled under `-D warnings` with zero warnings from `clippy::pedantic` and `clippy::nursery`.
2. **Architecture DAG:** Both prototypes were validated against `tests/architecture_conformance.rs`. An important boundary condition was identified and resolved during PB development: `RuleDocumentation` belongs to `FoundationPrimitives` and cannot import `core::*Config`. Mapping `shape_keys` in `RuleCatalog` cleanly preserves the DAG edge `RuleCatalog => [RuleSelection, RuleDocumentation, CoreVocabulary]`.
3. **End-to-End Snapshot Coverage:** Discovery flags (`--list-rules [--tag <label>]`, `--explain <rule>`), diagnostics footers, typo suggestions, and exit codes were verified across 26 CLI integration tests.
4. **Self-Dogfooding:** Omni ran on its own source files without diagnostic violations, successfully catching and preventing abbreviation leakage (`err` -> `error`).

---

## 2. Decision Settlement Summary

| Decision ID | Area | Resolution | Rationale |
| :--- | :--- | :--- | :--- |
| **DI8** | Discovery CLI Surface | Flags: `--list-rules [--tag <label>]` and `--explain <rule>` | Avoids subcommands for now, keeping CLI interface atomic and simple. Subcommands deferred to ROADMAP. |
| **DI9** | Binary Parity | Both `omni-code-lint` and `omni-command-lint` provide the discovery surface for all registered rules | Users don't need to know which binary owns which rule just to read documentation or search the catalog. |
| **DI10** | Summary Line | Dedicated `summary: &'static str` + detailed `what_it_does: &'static str` (PB model) | Forcing `what_it_does` to a single sentence compromised `--explain` depth; separating them keeps lists crisp and docs comprehensive. |
| **DI11** | Configuration Keys | Strongly-typed `ConfigShape` enum with catalog key derivation (PB model) | Eliminates typo risks and repetitive string duplication in rule files while respecting DAG boundaries. |
| **DI12** | Incremental Adoption | `RuleDoc::TODO` placeholder with 4 fully documented initial rules | Decouples infrastructure rollout from authoring 26 rationale texts without breaking compile-time completeness. |
| **DI13** | Status in `explain` | Status included without file paths (PB model) | Answers "Why is this rule on/off?" by displaying active status and provenance (`via testing > test-timing`). |
| **DI14 / D48** | Plain Diagnostics Footer | Single-line terminal footer: `For details on a rule, run: <binary> --explain <rule>` | Guides users naturally to discovery without spamming diagnostics. |
| **DI15 / D49** | Config Section Name | `## Configuration` | Standard terminology aligned with linter conventions. |
| **DI16 / D50** | Render Format | Plain Markdown rendered directly to terminal stdout | Clean, readable, works out-of-the-box with pagers and CI logs without ANSI terminal quirks. |
| **D51** | OutputFormat ValueEnum | Replaces stringly `format: String` with `OutputFormat::{Plain, Json}` on diagnostics | Strongly typed CLI options. |
| **D52** | Catalog Removal | Remove hand-maintained catalog from `README.md` | Replaced by canonical in-tool `--list-rules`. Regeneration script deferred to ROADMAP. |
| **D53** | File Grouping | Existing multi-rule files remain grouped; new rules use one file per rule | Minimal disruption, strictly conforming to surgical changes rule. |

---

## 3. Residual Debt & Non-Goals Check

- No macros were introduced for regular Rust code.
- No new external crate dependencies were added.
- Rule examples (D42) remain cleanly deferred to a separate roadmap item.
- Catalog markdown generator script (D44) remains cleanly deferred to roadmap.
