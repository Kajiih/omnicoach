# Phase 4: Execution Log — `--explain` TOML Block & List Option Operations

This document records **Phase 4 (Execute)** for [03_plan.md](03_plan.md).

> Status: **VALIDATED** (2026-10-02). Next: **Phase 5 (Clean up)**.

---

## 1. Task Execution Summary

| Task | RED Outcome | GREEN Implementation | Verification |
| :--- | :--- | :--- | :--- |
| **T1**: Symmetric list keys & layer-by-layer `ListOption::resolve` | `list_applies_global_table_then_language_table` (2 cases) and `invalid_options_are_rejected_with_their_key_path::case_02_unrelated_key` failed on unknown `remove-banned` / `remove-allowed` keys. | Added `ListKind::remove_key` (`remove-banned` / `remove-allowed`), `ListOverride::remove`, `ListOverride::apply_to`, layer-by-layer `ListOption::resolve` (`default` $\to$ `global` $\to$ `language`), `OptionValues::set` arm, and `DeclaredOptions::enforcement_mode` in [src/rule_declaration/options.rs](../../../src/rule_declaration/options.rs). | `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps --document-private-items` passed. |
| **T2**: Normalize `unstructured-task` defaults & registry option-language guard | `test_registry_integrity::case_1_code_rules` failed with: `Single-language rule unstructured-task must declare its extend-banned default in base, not per-language`. | Moved `unstructured-task`'s 7 Python call patterns from `extend` to `base` in [src/code_lint/rules/unstructured_task.rs](../../../src/code_lint/rules/unstructured_task.rs), guarded by `validate_option_languages` in [tests/registry.rs](../../../tests/registry.rs). | Full verification suite passed. |
| **T3**: Render `remove-*` bullet and fenced `toml` block in `--explain` + round-trip test | `explain_toml_block_round_trips_to_default_options_for_every_rule` failed on `Rule unstructured-task rendered no ```toml block under ## Configuration`. | Added `remove_key` bullet in `configuration_lines`, `configuration_toml` in [src/rule_catalog.rs](../../../src/rule_catalog.rs), and updated [tests/snapshots/cli__explain_sleep_in_tests.snap](../../../tests/snapshots/cli__explain_sleep_in_tests.snap). Refactored the round-trip test to use a single structural `assert_eq!` helper (`resolved_rule_options`) after `test_self_dogfooding_code_lint` flagged `too-many-assertions` (5 > 4). | Full verification suite passed (including self-dogfooding and round-trip over all 27 rules). |
| **T4**: Update ADRs, developer guides, and `ROADMAP.md` | — | Updated [decisions/009_rule_declaration_and_options.md](../../../decisions/009_rule_declaration_and_options.md), [decisions/010_naming_and_message_conventions.md](../../../decisions/010_naming_and_message_conventions.md), [docs/dev/naming_and_message_style_guide.md](../naming_and_message_style_guide.md), and [ROADMAP.md](../../../ROADMAP.md). | Full verification suite passed. |

---

## 2. Manual Checkpoint Output (`--explain`)

Verified via `cargo run --quiet --bin omni-code-lint -- --explain <rule>`:

1. **`abbreviated-name`** (`ListOption` with per-language `remove`):
   ```toml
   [rules.abbreviated-name]
   enforcement-mode = "ban"
   banned = ["err", "ctx", "cfg", "res", "msg", "str", "num", "btn", "cb", "ch", "diag", "ty", "cat", "stmt", "ext", "fmt", "arch", "vis"]

   [rules.abbreviated-name.rust]
   remove-banned = ["str"]
   ```
2. **`sleep-in-tests`** (`ListOption` with per-language `extend`):
   ```toml
   [rules.sleep-in-tests]
   enforcement-mode = "ban"
   banned = ["sleep"]

   [rules.sleep-in-tests.python]
   extend-banned = ["time.sleep", "asyncio.sleep", "anyio.sleep", "trio.sleep"]

   [rules.sleep-in-tests.rust]
   extend-banned = ["thread::sleep", "std::thread::sleep", "time::sleep", "tokio::time::sleep"]
   ```
3. **`repeated-index-access`** (`(CountOption, CountOption)`):
   ```toml
   [rules.repeated-index-access]
   enforcement-mode = "ban"
   min-positions = 2
   max-placeholders = 2
   ```
4. **`unstructured-task`** (single-language `ListOption`, normalized to `base`):
   ```toml
   [rules.unstructured-task]
   enforcement-mode = "ban"
   banned = ["create_task", "ensure_future", "asyncio.create_task", "asyncio.ensure_future", "loop.create_task", "event_loop.create_task", "$LOOP($$$LOOP_ARGS).create_task"]
   ```
