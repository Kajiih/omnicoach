# Phase 2: Gather Resources and Reference — `--explain` TOML Block & List Option Operations

This document records **Phase 2 (Gather Resources and Reference)** for adding a ready-to-paste `[rules.<name>]` TOML block to `--explain` and completing the symmetric 3-operation list-option model (`replace` / `extend-*` / `remove-*`).

> Status: **VALIDATED** (2026-10-02). Next: **Phase 3 (Design/Plan)**.

---

## 1. Selected References (Internal & External SOTA)

| # | Reference | Role | Key Idea | How It Differs / What We Adapt |
| :--- | :--- | :--- | :--- | :--- |
| **R1** | **Internal**: [FilterListDefaults](../../../src/rule_declaration/options.rs) (`options.rs` L14–42) | Adopt | Compile-time list defaults are expressed as three orthogonal operations: `base`, per-language `extend`, and per-language `remove`, evaluated sequentially (`base` $\to$ `+ extend` $\to$ `- remove`). | Previously lacked a matching `remove-*` key in `.omnilint.toml`, preventing `abbreviated-name`'s Rust exception (`remove = ["str"]`) from being represented in TOML without duplicating the 17-item base list. |
| **R2** | **Internal**: [ADR 009](../../../decisions/009_rule_declaration_and_options.md), [ADR 010 §3.4](../../../decisions/010_naming_and_message_conventions.md), and [naming_and_message_style_guide.md §4](../naming_and_message_style_guide.md) | Adopt & Update | Single-source-of-truth `RuleOptions<Options>` declarations drive runtime resolution, load-time validation, and `--explain`. Commit `mwllvkmo` removed the old polarity-flipped 3rd key (`allowed` on `Deny`, `banned` on `Allow`) and specified: *"If that becomes a recurring need, it will be a `remove-banned` / `remove-allowed` key, parsed into a `remove` field on `ListOverride` and applied after `extend`."* | ADR 010 §3.4 specified intra-table order (`replace` $\to$ `extend` $\to$ `remove`), which we now combine with a general-to-specific inter-table fold (`default` $\to$ `[rules.<name>]` $\to$ `[rules.<name>.<lang>]`). |
| **R3** | **External SOTA**: **Ruff** (`ruff rule <code>`, `select` / `extend-select`, plugin `extend-*` keys) | Adopt & Improve | Uses `<key>` (replace) + `extend-<key>` (add) in kebab-case TOML and documents rule options under `## Options` in `ruff rule`. | 1. Ruff lacks `remove-<key>` on rule-level list options (forcing users to copy-paste the whole default list to remove one entry).<br/>2. `ruff rule` prints option prose/tables without a copy-pasteable TOML snippet showing effective defaults. |
| **R4** | **External SOTA**: **Hierarchical State-Transducer Configs** (ESLint Flat Config cascade, Nix module options, Bazel/Cargo hierarchical merge) | Adopt | Models hierarchical configuration layers ordered from general to specific ($L_0 = \text{default}$, $L_1 = \text{rule table}$, $L_2 = \text{language table}$) as a sequential state fold $S_i = f_{L_i}(S_{i-1})$, where each layer transforms the set produced by the previous layer. | Replaces `ListOption::resolve`'s current two-pass across-layers lookup (which searched `replace` across all layers first and then unioned `extend` across all layers, causing `[rules.<name>.<lang>]` `replace` to leak `[rules.<name>]` `extend`). |
| **R5** | **Internal & Crate**: [Cargo.toml](../../../Cargo.toml) (`toml::Value::String`) + [src/rule_selection.rs](../../../src/rule_selection.rs) (`parse_config`) | Adopt | `toml::Value::String(s).to_string()` guarantees valid TOML string quoting/escaping, while `parse_config` parses a complete `.omnilint.toml` document through the production config loader. | Used to format string literals in `[rules.<name>]` and to run the round-trip test on every registered rule's rendered `--explain` Markdown. |

---

## 2. Critical Points & Technical Analysis

### CP1 — Why Layer-by-Layer Folding (R4) Beats Two-Pass Lookup
In [src/rule_declaration/options.rs](../../../src/rule_declaration/options.rs) (L228–236), `ListOption::resolve` currently evaluates:
```rust
let mut items = layers(overrides, language)
    .find_map(|values| values.list.replace.clone())
    .unwrap_or_else(|| self.default.resolve_default_for_lang(language));
for values in layers(overrides, language) {
    items.extend(values.list.extend.iter().cloned());
}
```
where `layers(overrides, language)` yields `[language_table, global_table]` (most specific first).

This two-pass approach has two flaws once `replace`, `extend`, and `remove` interact across `[rules.<name>]` and `[rules.<name>.<lang>]`:
1. **Language `replace` leaks global `extend`**: If `[rules.foo]` sets `extend-banned = ["global_bad"]` and `[rules.foo.python]` sets `banned = ["py_only_bad"]`, the user asked Python to replace the inherited banned set with `["py_only_bad"]`. Yet the second pass still adds `"global_bad"` from the global table.
2. **Cross-layer `extend` vs. `remove` order depends on specificity, not operation kind**:
   - If `[rules.foo]` sets `banned = ["a", "b"]` and `[rules.foo.rust]` sets `remove-banned = ["b"]` (as `--explain abbreviated-name` will render!), language `remove-banned` must run *after* global `banned`.
   - Conversely, if `[rules.foo]` sets `remove-banned = ["str"]` globally and `[rules.foo.python]` sets `extend-banned = ["str"]`, Python's more-specific `extend-banned` must run *after* the global `remove-banned`.

Defining each table's `ListOverride` as a state transformer:
```rust
impl ListOverride {
    fn apply_to(&self, items: &mut HashSet<String>) {
        if let Some(replacement) = &self.replace {
            items.clone_from(replacement);
        }
        items.extend(self.extend.iter().cloned());
        for item in &self.remove {
            items.remove(item);
        }
    }
}
```
and folding from general to specific:
1. `let mut items = self.default.resolve_default_for_lang(language);`
2. If `overrides` is `Some(overrides)`:
   - `overrides.global.list.apply_to(&mut items);`
   - If `let Some(lang_values) = overrides.for_language(language)`: `lang_values.list.apply_to(&mut items);`

makes both intra-table precedence (`replace` $\to$ `extend` $\to$ `remove`) and inter-table precedence (`default` $\to$ `global` $\to$ `language`) completely uniform. Note that `FilterListDefaults::resolve_default_for_lang` is the exact same `[replace(base) -> extend(lang) -> remove(lang)]` pipeline at compile time.

### CP2 — Impact on Existing Unit Test in `options.rs`
In [src/rule_declaration/options.rs](../../../src/rule_declaration/options.rs) (L733–760), the existing test `list_replaces_from_the_nearest_table_then_extends_from_every_table` asserted the old two-pass behavior where `[python] banned = ["py_only_bad"]` combined with global `extend-banned = ["global_bad"]` yielded `["py_only_bad", "global_bad"]`. Under D3 (layer-by-layer cascade), `[python] banned = ["py_only_bad"]` replaces the global set in the `[python]` layer, so Python resolves to `["py_only_bad"]` (unless `[python]` itself also specifies `extend-banned`). We will update this test and add dedicated cases covering `remove-banned` / `remove-allowed` and cross-layer `replace` + `extend` + `remove` interactions.

### CP3 — Round-Trip Verification Architecture
In [src/architecture.rs](../../../src/architecture.rs) (L135–138):
- `RuleSelection => [RuleDeclaration, CodeLintRules, CodeLintSuppression, CommandLintRules, Config]`
- `RuleCatalog => [RuleSelection, RuleDeclaration]`

Because `RuleCatalog` already depends on `RuleSelection` and `RuleDeclaration`:
- A unit test in `src/rule_catalog.rs` can iterate over every rule in `registered_rules()`, call `explain(rule.name.0)`, extract the fenced ```` ```toml ... ``` ```` block under `## Configuration`, pass it to `rule_selection::parse_config`, and compare the resolved values for every `lang` in `rule.languages` against `None` overrides.
- Every `RegisteredRule` already carries `options: DeclaredOptions`, which contains `enforcement_mode: Option<LanguageDefaults<EnforcementMode>>` and `options: Vec<OptionSpec>` (`OptionSpec::Count(CountOption)` and `OptionSpec::List(ListOption)`). Calling `count.resolve(lang, overrides)` and `list.resolve(lang, overrides)` directly exercises the `OptionsDeclaration` implementation for every rule without needing downcasting.

### CP4 — TOML Block Formatting Details
To keep the rendered TOML block clean, deterministic, and idiomatic (D4):
- **Base table `[rules.<name>]`**:
  - `enforcement-mode = "<mode>"` (when `options.enforcement_mode` is `Some`).
  - `<count-key> = <base>` for each `OptionSpec::Count`.
  - `<banned|allowed> = [...]` for each `OptionSpec::List` (using `list.default.base`).
- **Per-language sub-tables `[rules.<name>.<lang>]`**:
  - Iterated in `rule.languages` order (or declaration override order) and emitted **only** if at least one key has a language-specific delta for `<lang>`:
    - `enforcement-mode = "<mode>"` if `enforcement_mode.overrides` has an entry for `<lang>`.
    - `<count-key> = <val>` if `count.default.overrides` has an entry for `<lang>`.
    - `extend-<banned|allowed> = [...]` if `list.default.extend` has a non-empty entry for `<lang>`.
    - `remove-<banned|allowed> = [...]` if `list.default.remove` has a non-empty entry for `<lang>`.
- **Array formatting**:
  - Empty array: `[]`.
  - Short single-line array when it fits comfortably on one line (e.g. $\le 80$ chars total line length) or multi-line indented array when long (such as `abbreviated-name`'s 17 items), or a single consistent format. Both parse identically in `toml::from_str`.
