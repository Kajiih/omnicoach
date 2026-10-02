# Phase 5: Clean Up — `--explain` TOML Block & List Option Operations

This document records **Phase 5 (Clean up)** for [03_plan.md](03_plan.md).

> Status: **VALIDATED** (2026-10-02). Next: **Phase 6 (Review and Audit)**.

---

## 1. Code, Documentation, and Artifact Audit

| Area | File / Target | Finding | Action Taken |
| :--- | :--- | :--- | :--- |
| **Per-language override lookup in `--explain` TOML renderer** | [src/rule_catalog.rs](../../../src/rule_catalog.rs) (`configuration_toml`) | Repeated `.iter().find(\|&&(target_lang, _)\| target_lang == language)` across 4 call sites and duplicated `extend` / `remove` emission blocks. | Extracted private `for_language<T: Copy>` helper and folded `extend` and `remove` emission over `[(list.kind.extend_key(), list.default.extend), (list.kind.remove_key(), list.default.remove)]`, removing 25 lines of repetition. |
| **Option declaration & resolution** | [src/rule_declaration/options.rs](../../../src/rule_declaration/options.rs) | Audited `ListKind`, `ListOption::resolve`, `ListOverride`, `OptionValues::set`, and `resolve_enforcement_mode`. | No dead code, redundant helpers, or stale comments found; docstrings match the 3-operation (`replace` / `extend` / `remove`) layer-by-layer model. |
| **Rule declarations & registry tests** | [src/code_lint/rules/unstructured_task.rs](../../../src/code_lint/rules/unstructured_task.rs), [tests/registry.rs](../../../tests/registry.rs) | Audited all 27 rules' `FilterListDefaults` and `LanguageDefaults` against `validate_option_languages`. | Clean; no legacy or unused per-language overrides remain. |
| **ADRs, Guides, & Roadmap** | [decisions/009_rule_declaration_and_options.md](../../../decisions/009_rule_declaration_and_options.md), [decisions/010_naming_and_message_conventions.md](../../../decisions/010_naming_and_message_conventions.md), [docs/dev/naming_and_message_style_guide.md](../naming_and_message_style_guide.md), [docs/dev/adding_a_rule.md](../adding_a_rule.md), [README.md](../../../README.md), [ROADMAP.md](../../../ROADMAP.md) | Searched repository-wide for `extend-banned`, `extend-allowed`, and `FilterListDefaults`. | All live docs and ADRs reflect the 3-key list option model and updated `--explain` behavior; completed `ROADMAP.md` §5 item removed. |
| **Scratch artifacts** | `docs/dev/explain_toml_and_list_options/` | Checked for temporary scripts or scratch files created during Phases 1–4. | None created; only structured phase documents (`01`–`05`) exist. |
