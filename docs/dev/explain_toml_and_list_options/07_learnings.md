# Phase 7: Learnings — `--explain` TOML Block & List Option Operations

This document records **Phase 7 (Learn)** for [03_plan.md](03_plan.md), capturing process and design principles from this cycle.

> Status: **DRAFT — AWAITING USER REVIEW** (2026-10-02).

---

## 1. Round-Trip Tests Surface Representation Asymmetries

Requiring user-facing documentation (`--explain` TOML snippets) to round-trip through the real config parser (`parse_config`) and resolve to the exact same values as compile-time defaults immediately exposed an asymmetry: internal defaults (`FilterListDefaults`) had three operations (`base`, `extend`, `remove`), while `.omnilint.toml` only had two (`replace`, `extend-*`).

- **Principle**: When compile-time defaults and user configuration describe the same domain values, give them an isomorphic algebra and guard the bijection with a round-trip test across all registered rules.

---

## 2. Keep Set Operations Orthogonal to Polarity and Cascade General-to-Specific

An earlier config schema overloaded `allowed` inside a denylist rule to mean "remove from `banned`", conflating list polarity (`Deny` vs. `Allow`) with set mutation (`replace`, `extend`, `remove`). Likewise, resolving all `replace` layers before all `extend` layers caused global `extend` entries to leak into language-specific `replace` tables.

- **Principle**:
  1. Name configuration keys by combining the operation (`<kind>`, `extend-<kind>`, `remove-<kind>`) with the list's polarity (`banned` or `allowed`) so polarity never flips.
  2. Resolve layered configuration as a fold from general to specific (`default` $\to$ `global` $\to$ `language`), applying the same intra-layer pipeline (`replace` $\to$ `extend` $\to$ `remove`) at every level.

---

## 3. Colocate Parsing and Emission in One Boundary Module Using One Representation

Formatting `[rules.<name>]` lines with `format!` in `rule_catalog.rs` leaked TOML syntax into the Markdown catalog renderer, while considering a separate library (`toml_edit` / `taplo`) risked either missing what was already in `Cargo.lock` or introducing a second TOML AST alongside `toml::Table`.

- **Principle**:
  1. Inspect `cargo tree` / `Cargo.lock` before making dependency assumptions.
  2. Keep both deserialization (`RuleOverrides::parse`) and serialization (`DeclaredOptions::default_toml`) inside the same boundary module (`rule_declaration::options`), using the single library representation (`toml::Table` / `toml::Value`) already used at that boundary rather than manual format strings or a second AST type.
