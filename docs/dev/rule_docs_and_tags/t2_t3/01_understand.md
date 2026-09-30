# Phase 1: Understand — Rule Documentation Model (T2) & Surfaces (T3)

This document records **Phase 1 (Understand)** of the second exploration loop for Omni's rule documentation and tagging system. The first loop ([t1/](../t1/)) designed and shipped the taxonomy (T1, [ADR 007](../../../../decisions/007_rule_taxonomy_and_selection.md)).

**Status**: Validated. Next: **Phase 2 (Gather Resources and Reference)**.

---

## 1. Context & Problem Statement

T1 left two things unbuilt: the rule doc model (T2) and the surfaces that render it (T3). D13 deferred Q4–Q8 to this loop. Both are designed here because T3's outputs are the only consumers of T2's model: they decide what the model must hold and in which format. They still ship as two separate `impl/` cycles (D12).

Facts as of today (21 code rules, 4 suppression audits, 1 command rule: 26 in all):

1. **Rule prose lives in four places, none checked:**
   - the `//!` module doc (one per file; `no_sleep_in_tests.rs` holds two rules, `suppression.rs` holds four);
   - the `///` struct doc (one line);
   - the `ViolationTemplate` (`summary` / `rationale` / `suggestion`, each a `LanguageText` with per-language overrides and `{placeholders}`), which is the only prose reachable at runtime;
   - the hand-written [README.md](../../../../README.md) "Rules Catalog", grouped by the old flat tags, with per-rule TOML option snippets. Nothing checks it against the registries.
2. **Examples exist only as tests.** Every code rule has `rule_test!` pass/fail cases per language (named, `indoc!`-ed, fail cases optionally with the flagged snippet). They are inside `#[cfg(test)]`, so **the binary cannot see them**. Command rules and suppression audits use plain `#[test]` functions, not `rule_test!`.
3. **Options are defined by serde config structs** (`ThresholdConfig`, `DenyListConfig`, `AllowListConfig`, `EnforcementConfig`, `DynamicRuleConfig<T>`) with defaults in rule-level consts (e.g. `FilterListDefaults`). Only the README documents them.
4. **The taxonomy can now describe a rule**, but has no public query API yet: `ClassifiedRule` registries, `Topic` descriptions, scope notes and synonyms exist; `describe_rule` / `describe_tag` / `explain` (I1–I3) were deferred to T3 (ROADMAP §5). The P0 reference is `scratch/tag_poc/p0_typed_facets/src/facade.rs`.
5. **Surfaces today:** two binaries (`omni-code-lint`, `omni-command-lint`) with `--format plain|json`. Diagnostics carry `rule_name`, `message` (rendered `summary` / `rationale` / `suggestion`) and `location`. There is no subcommand, no rule listing, no doc pointer and no tag in diagnostics.

---

## 2. Carried Over from T1 (not reopened)

| Item | Content |
|---|---|
| G1 | Rule docs live with the rule, reachable from the binary. |
| G3 / D9 | Tags are rendered wherever rules are listed or explained. |
| G4 | Derived facts are computed, never declared (language, input, target, configurable, later fixable). |
| G5 / D5 | One style guide for rule docs and violation messages; shared text has one source. |
| G7 / D3 | Readers: humans in the terminal and AI agents; the README catalog is replaced by declared docs. |
| D4 | Ruff-style sections (What it does / Why is this bad / Example / Use instead / Options / References). **Amended by D42**: Example / Use instead are deferred. |
| D12 | Order: T2 → T3 → content pass. Rule prose is rewritten only in the content pass. |
| D36 | `explain` shows branch provenance (`testing [via test-doubles]`). |
| DEF3 | "Which tool or document backs this rule?" is a **References** field, not a facet. |
| NG4, NG5, NG6 | No rewriting every rule's prose; no severity/lifecycle/presets; no hosted website. |
| A1–A4 | No config backward compatibility; docs in the binary; command rules in scope; design for hundreds of rules. |

---

## 3. Goals & Explicit Non-Goals (this loop)

### Goals

- **H1 — One declared doc per rule, colocated with the rule, covering the D4 sections except examples (D42).**
  - *Reason*: G1. Today's prose is split and partly unreachable.
- **H3 — Options are documented from the config types and defaults, not restated.**
  - *Reason*: G4. The README option snippets are already hand-copied.
- **H4 — A human can list rules, filter them by tag, and read one rule's docs and tags from the terminal.**
  - *Reason*: G7.
- **H5 — An agent fixing a diagnostic can get the rule's full docs without reading source.**
  - *Reason*: G7, D3.
- **H6 — The README "Rules Catalog" is removed; rule docs have one source.**
  - *Reason*: D3. The README catalog has drifted. Whether a generated catalog comes back, or the commands are the only surface for now, is Q7 (D44).
- **H7 — `explain` tells a user why a rule is on or off for a path.**
  - *Reason*: I3, D36. Precedence model B is correct but not obvious.

### Explicit Non-Goals

- **NG7 — Rewriting existing rule prose** beyond what the mechanism needs to compile (content pass, D12, NG4).
- **NG8 — Autofix and a `fixable` facet.**
- **NG9 — SARIF / JUnit / LSP output.** (ROADMAP items; the model should not preclude them.)
- **NG10 — Merging the two binaries or redesigning the CLI beyond the new discovery surfaces.**
- **NG11 — Examples in rule docs (Example / Use instead).** Sourcing them from `rule_test!` cases needs the cases outside `#[cfg(test)]` and a marked subset per language, which reshapes the test macro. It is a separate ROADMAP item (D42).

---

## 4. Decisions (D39–)

- **D39 — Scope**: This loop designs T2 and T3 together (Q4–Q8 plus the applicable Adjacent topics of T1 §5) and ships them as two `impl/` cycles.
- **D40 — Work directory**: `docs/dev/rule_docs_and_tags/t2_t3/`. The T1 records moved to `t1/`.
- **D41 — Process**: All 7 phases. Phase 2 is narrower than T1's: the doc systems of Ruff, Clippy and ESLint / Biome, and Rust mechanisms for embedding docs. One prototype covers Q4.
- **D42 — Examples are out of scope** (NG11, ROADMAP §5). Former A6 / A7 and Q5 move there.
- **D43 — Keep each change simple and atomic.** Anything that needs a large change (a new mechanism, a CLI redesign, a new output format) becomes its own ROADMAP item instead of joining this loop.
- **D44 — The README "Rules Catalog" is removed.** Phase 3 decides whether a generated catalog comes back (and how it is kept fresh) or whether the commands are the only surface for now (Q7).

---

## 5. Assumptions (to confirm)

- ~~**A6 / A7 — Examples in the binary, from a marked subset of test cases.**~~ *Withdrawn.* Out of scope (D42).
- **A8 — Doc text is Markdown**, ✅ *Confirmed, if cheap*. A library for terminal rendering is acceptable only if it adds little complexity (Q4).
- **A9 — One doc per rule, not per file.** ✅ *Confirmed.* Whether to also standardize one file per rule is Q13.
- **A10 — The README catalog goes away.** ✅ *Confirmed* (D44).

---

## 6. Open Questions for Phase 2 (SOTA) & Phase 3 (Design)

- **Q4 — Colocation mechanism**: How does a rule's doc get into the binary? Explicit const struct, declarative macro, `///` extraction by a crate (`documented`, `strum::EnumMessage`), `include_str!` of a sibling Markdown file, or something else? How is Markdown shown in the terminal (plain, or a small renderer crate)?
- ~~**Q5 — Examples from tests.**~~ *Deferred* (D42).
- **Q6 — Shared text**: Is "Why is this bad" the same text as `ViolationTemplate::rationale`, or a longer version? What does each piece own? How are `{placeholders}` and per-language overrides shown in docs?
- **Q7 — Surfaces**: Command names and output formats (a `rules` / `explain` subcommand on each binary? a shared one?). Is a generated catalog (e.g. under `docs/rules/`) worth adding now, and if so is drift prevented by a test or by generation? Or are the commands enough for now (D44)?
- **Q8 — Tags and doc pointers in diagnostics**: Does JSON output carry tags and a pointer to the docs? Does plain output?
- **Q10 — Options docs**: How are option names, types, defaults and descriptions extracted from the config structs and per-rule defaults (e.g. `FilterListDefaults`)?
- **Q11 — Style guide**: What form does the G5 style guide take, and which checks can enforce it (e.g. section presence, length limits, no duplicated rationale)?
- **Q12 — Adjacent topics**: Which of "See also" links between rules and "Overlap" with external rules (Ruff, Clippy) belong in the model now (References, DEF3) and which stay on the roadmap?
- **Q13 — File layout**: Should every rule get its own file (splitting `no_sleep_in_tests.rs` and `suppression.rs`), or is one doc per rule enough? Decided on RICR, not symmetry.
