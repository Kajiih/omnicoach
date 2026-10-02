# Phase 1: Understand — `--explain` TOML Block & List Option Operations

This document records **Phase 1 (Understand)** for adding a ready-to-paste `[rules.<name>]` TOML configuration block to `--explain` (`ROADMAP.md` §5) and aligning the list-option configuration model (`replace` / `extend` / `remove`) so every rule's defaults can be expressed and round-tripped in `.omnilint.toml`.

> Status: **VALIDATED** (2026-10-02). All decisions D1–D6 validated; no open questions. Next: **Phase 2 (Gather Resources and Reference)**.

---

## 1. Context & Problem Statement

`ROADMAP.md` §5 specifies:
> Ready-to-paste `[rules.<name>]` TOML block in `--explain` (in addition to the current bullet list rendered from `DeclaredOptions`), guarded by a round-trip test that parses the rendered TOML back into `Config` and compares effective values per language.

Auditing all 27 registered rules (22 code rules, 4 suppression audits, 1 command rule) and `src/rule_declaration/options.rs` surfaced an architectural mismatch between compile-time defaults and the `.omnilint.toml` surface:

1. **Compile-time list defaults have 3 operations; `.omnilint.toml` has 2:**
   - `FilterListDefaults` (`src/rule_declaration/options.rs` L14–21) defines `base`, per-language `extend`, and per-language `remove`.
   - `abbreviated-name` uses `remove: &[(SupportLang::Rust, &["str"])]` because `"str"` is a general abbreviation banned by default, with Rust as an explicit exception (`str` is a primitive type keyword in Rust).
   - `ListKind` (`src/rule_declaration/options.rs` L176–201) currently exposes only two TOML keys: `replace_key()` (`banned` / `allowed`) and `extend_key()` (`extend-banned` / `extend-allowed`).
   - Earlier, a third key existed with flipped polarity (`allowed` on `ListKind::Deny`, `banned` on `ListKind::Allow`). Commit `mwllvkmo` removed it as confusing, and [ADR 010 §3.4](../../../decisions/010_naming_and_message_conventions.md) and [naming_and_message_style_guide.md §4](../naming_and_message_style_guide.md) recorded:
     > *"If that becomes a recurring need, it will be a `remove-banned` / `remove-allowed` key, parsed into a `remove` field on `ListOverride` and applied after `extend`."*
2. **Without a `remove-*` TOML key, `abbreviated-name` cannot express its Rust exception as a subtraction in TOML:**
   - When `[rules.abbreviated-name]` sets `banned = ["err", ..., "str", ...]`, `ListOption::resolve` (`src/rule_declaration/options.rs` L228–236) replaces the base list and bypasses `self.default.resolve_default_for_lang(Rust)`.
   - Without `remove-banned = ["str"]` in `[rules.abbreviated-name.rust]`, `[rules.abbreviated-name.rust]` would have to duplicate the entire 17-element `banned` list—hiding the fact that `"str"` in Rust is a single-item exception.
3. **Single-language `FilterListDefaults` inconsistency (`unstructured-task`):**
   - Out of 7 Python-only rules with a `ListOption`, 6 (`type-cast`, `suppressed-exception`, `mock-in-tests`, `mock-call-assertion`, `error-log-in-except`, `dynamic-attribute-access`) place their defaults in `base: &[...]`.
   - 1 (`unstructured-task`) places `base: &[]` and `extend: &[(SupportLang::Python, &[...])]`.
4. **Current `--explain` configuration output has no copy-pasteable TOML:**
   - `render_rule` (`src/rule_catalog.rs` L145–158) renders a Markdown bullet list describing each key's type, prose default summary, and doc string, but users must manually construct the `[rules.<name>]` and `[rules.<name>.<language>]` tables and quote/format list items themselves.

---

## 2. Inventory of Rule Option Shapes (27 Rules)

| Option shape | Count | Rules | Per-language defaults |
| :--- | :--- | :--- | :--- |
| `RuleOptions::none()` | 5 | 4 suppression audits, `edit-of-described-commit` | None (`## Configuration` omitted) |
| `()` (`enforcement-mode` only) | 4 | `mutable-dataclass`, `nested-function`, `packed-assertion`, `unslotted-dataclass` | `ban` |
| `CountOption` | 2 | `too-many-assertions` (4), `identical-positional-types` (3) | Base only |
| `(CountOption, CountOption)` | 1 | `repeated-index-access` (`min-positions` = 2, `max-placeholders` = 2) | Base only |
| `ListOption` (`Deny`), base only | 9 | `type-cast`, `suppressed-exception` (mode = `require-explanation`), `mock-in-tests`, `mock-call-assertion`, `error-log-in-except`, `dynamic-attribute-access`, `primitive-duration`, `type-suffixed-name`, `unstructured-task` (after D6) | Base only |
| `ListOption` (`Deny`), with `extend` | 3 | `sleep-in-tests`, `zero-sleep-in-tests`, `environment-variable-in-function` | Per-language `extend` |
| `ListOption` (`Deny`), with `remove` | 1 | `abbreviated-name` | `remove: &[(Rust, &["str"])]` |
| `ListOption` (`Allow`), with `extend` | 2 | `single-letter-name` (`base` + Rust `extend`), `bare-multiline-string` (Python & Rust `extend`) | Per-language `extend` |

---

## 3. Goals & Explicit Non-Goals

### Goals
- **G1 — Expressive symmetry between `FilterListDefaults` and `.omnilint.toml`**: Every compile-time list default (`base`, per-language `extend`, per-language `remove`) has a direct, explicit counterpart in `.omnilint.toml` (`banned`/`allowed`, `extend-banned`/`extend-allowed`, `remove-banned`/`remove-allowed`), and users can remove individual default items in `.omnilint.toml` without copying the full list.
- **G2 — Ready-to-paste TOML block in `--explain`**: Every configurable rule's `--explain` output includes a valid `toml` code block under `## Configuration` showing its default configuration.
- **G3 — Exact round-trip invariant**: Parsing the rendered TOML block from `--explain` via `rule_selection::parse_config` produces effective option values (`enforcement_mode` and `OptionsDeclaration::resolve`) for every supported language of the rule that are identical to resolving with `None` overrides.
- **G4 — Clear, predictable layer-by-layer composition semantics**: Combining `replace`, `extend`, and `remove` across `default` → `[rules.<name>]` → `[rules.<name>.<language>]` follows a strict general-to-specific cascade.

### Explicit Non-Goals
- **NG1 — Commented-out full `.omnilint.toml` file generation (`--init` / `--dump-config`)**: Out of scope; this work focuses on per-rule `--explain` and list-option resolution.
- **NG2 — Changing any rule's effective default set**: For every existing rule and language, `resolve(lang, None)` must return the exact same value before and after.
- **NG3 — Supporting `remove-*` on non-list options**: Only `ListOption` has set-subtraction semantics.

---

## 4. Validated Decisions

| ID | Decision | Rationale |
| :--- | :--- | :--- |
| **D1** | **Work directory**: `docs/dev/explain_toml_and_list_options/`. | Matches the repository's 7-phase documentation layout (`docs/dev/rule_layering/`, `docs/dev/prefer_tuple_unpacking/`). |
| **D2** | **Add `remove-banned` and `remove-allowed` to `ListKind`**: `ListKind::Deny` exposes `banned`, `extend-banned`, `remove-banned`; `ListKind::Allow` exposes `allowed`, `extend-allowed`, `remove-allowed`. Update ADR 009, ADR 010, and `naming_and_message_style_guide.md` §4. | Restores symmetric 3-operation expressiveness (`replace`, `extend`, `remove`) without the polarity flip of the old `allowed`/`banned` keys. |
| **D3** | **Layer-by-layer resolution (`default` → `[rules.<name>]` → `[rules.<name>.<lang>]`)**: Start with `self.default.resolve_default_for_lang(language)`, then apply `overrides.global` (`replace`, then `extend`, then `remove`), then apply `overrides.for_language(language)` (`replace`, then `extend`, then `remove`). | Ensures a more specific table (`[rules.<name>.<lang>]`) always overrides the general table (`[rules.<name>]`), whether replacing, extending, or removing. |
| **D4** | **Concise TOML block in `--explain`**: In `[rules.<name>]`, emit `enforcement-mode`, count keys, and `banned`/`allowed`, omitting empty `extend-* = []` and `remove-* = []`. Emit `[rules.<name>.<lang>]` only when `<lang>` has a language-specific override (`LanguageDefaults::overrides`, `FilterListDefaults::extend`, or `FilterListDefaults::remove`). | Keeps the copy-paste snippet clean and noise-free while expressing every default and per-language difference. |
| **D5** | **Keep the bullet list and add the TOML block below it in `## Configuration`**: The bullet list continues to document each key, its type, its default, and its description (adding a third bullet for `remove-banned` / `remove-allowed`), followed by the fenced `toml` block. | Matches `ROADMAP.md` §5; bullets explain the schema (including `extend-*` and `remove-*` which are omitted from the default TOML block when empty), while the TOML block gives the copy-pasteable defaults. |
| **D6** | **Move `unstructured-task` defaults from `extend: &[(Python, ...)]` to `base`**: Since `unstructured-task` only analyzes `Python`, place its default list in `base` like the 6 other Python-only rules. | Eliminates an unnecessary `[rules.unstructured-task.python]` sub-table on a single-language rule. |
