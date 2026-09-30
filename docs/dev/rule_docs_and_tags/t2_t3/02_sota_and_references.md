# Phase 2: Gather Resources & Reference — Rule Docs (T2) & Surfaces (T3)

**Status**: Validated (decisions in §5). Next: **Phase 3 (Design Plan)**.

Sources: two read-only research agents ([linter doc systems](conversation://02eedfc8-0b48-4285-9894-f0b9d1691598), [Rust embedding mechanisms](conversation://672da768-d727-44bb-9b15-13ae2616ed1d)).

> [!WARNING]
> Both agents could search but not open pages, so claims about other projects' source code are partly from recall. Items marked *(verify)* must be checked against the linked source before an ADR cites them. None of the recommendations below depends on an unverified item.

---

## 1. How mature tools document rules

| Tool | Where the doc lives | Sections enforced by | Message vs doc | Options docs | Discovery CLI | Doc pointer in diagnostics |
|---|---|---|---|---|---|---|
| **Ruff** | `///` Markdown on the violation struct, captured by a `ViolationMetadata` derive ([source](https://github.com/astral-sh/ruff/blob/main/crates/ruff_macros/src/violation_metadata.rs)) | Template + review; `## Options` keys checked against settings by `generate_docs.rs` | Separate. `message()` + `fix_title()`; doc owns the rationale. Raw message templates shown via `message_formats` | Once, on `Options` structs (`OptionsMetadata` derive, hand-written default strings) | `ruff rule <code>`, `ruff rule --all --output-format json`, `ruff linter`, `ruff config` | JSON `url` *(verify)* |
| **Clippy** | `///` in `declare_clippy_lint!`, `concat!`-ed into `LintInfo` | Template (`What it does / Why is this bad / Known problems / Example`) + review | Separate. Call-site message, one-line description, long doc | `define_Conf!` (real default expr + `#[lints(..)]`), generated `lint_configuration.md` guarded by a drift test ([test](https://github.com/rust-lang/rust-clippy/blob/master/tests/config-metadata.rs)) | `cargo clippy --explain <lint>` (raw Markdown) | "for further information visit <url>" note |
| **rustc** | One `.md` per error code, `include_str!`-ed ([dir](https://github.com/rust-lang/rust/tree/master/compiler/rustc_error_codes/src/error_codes)) | `tidy`: every code has a doc | Separate | — | `rustc --explain E0308` | Footer once per run: "try `rustc --explain E0308`"; JSON `code.explanation` holds the full text ([json](https://doc.rust-lang.org/rustc/json.html)) |
| **Biome** | `///` in `declare_lint_rule!` (`macro_rules!`) | `xtask` checks examples and options blocks | Separate; diagnostics follow three "pillars" (what / why / what to do) — the same split as Omni's `summary` / `rationale` / `suggestion` | Rust types; `## Options` prose hand-written, examples deserialized | `biome explain <rule>` | Category rendered as a terminal hyperlink |
| **ESLint** | `meta.docs` (description, url) + separate Markdown files | Build checks: doc exists; typescript-eslint test requires headings | Three texts: `messages`, `docs.description`, Markdown | `meta.schema` (JSON Schema) | None | `json-with-metadata`: top-level `rulesMeta` map, deduplicated ([docs](https://eslint.org/docs/latest/use/formatters/#json-with-metadata)) |
| **Pylint** | One tuple per message: template, symbol, description | — | Template and description declared together | — | `--list-msgs`, `--help-msg=<x>` | — |
| **Semgrep** | Rule YAML `message` + free-form `metadata` | Repo CI for security rules | — | — | — | JSON copies all `metadata` into each finding |

## 2. Recurring patterns

1. **Three texts, not one.** Per-occurrence message (with placeholders), a one-line summary for lists, and a long doc.
2. **The long doc owns the rationale; no tool shares text between message and doc.** The only reuse is showing the *raw message template* inside the doc (Ruff `message_formats`).
3. **Docs sit with the rule and ship in the binary** (Ruff, Clippy, Biome, oxlint, rustc, Pylint). ESLint's separate files drifted and needed `eslint-doc-generator --check`.
4. **Section enforcement is light**: a template plus review. Machines check only what breaks silently: a doc exists for every rule, required headings, option keys exist.
5. **Options are documented once, on the config types**, and rules reference them (Clippy, Ruff).
6. **Generated files are kept fresh by a check**, never by discipline (Clippy `config-metadata` test, Ruff `generate-all --mode check`).
7. **Discovery is a pair of commands**: list (`ruff rule --all`, `pylint --list-msgs`, `oxlint --rules`) and show one (`ruff rule X`, `clippy --explain X`, `biome explain X`, `rustc --explain X`).
8. **Without a website, the pointer is a command** (rustc's footer). Plain Markdown output is the norm (`ruff rule`, `clippy --explain`).
9. **Grouped rule files are common when rules share detection code** (Ruff `pyflakes/rules/strings.rs`, Clippy `booleans.rs`); Biome and oxlint use strictly one rule per file.

---

## 3. Options per open question

### Q4 — Colocation mechanism

| Mechanism | Missing section fails | New deps / macro | Shares text with `ViolationTemplate` | Several rules per file | Notes |
|---|---|---|---|---|---|
| **(a) Const `RuleDoc` struct** (`impl X { pub(crate) const DOC }`, a `doc` field on `ClassifiedRule`) | **Build** (`E0063`) | None | **Yes** (const reference) | Trivial | Same shape as `CLASSIFICATION`. Prose inside Rust strings (`indoc!` already a dependency). Renderer owns headings. No rustdoc lints on user prose. |
| (b) `include_str!` of a sibling `.md` | Missing file: build. Section: test | None | No | N `.md` files | rustc precedent; best authoring; weaker colocation; path can point at the wrong rule. |
| (c1) `macro_rules!` capturing `///` (Biome, Clippy) | Missing doc: build. Section: test | Macro | No | Yes | One text for rustdoc and runtime, but pedantic `doc_markdown`, intra-doc links and doctests apply to user prose. Violates "no macro for what Rust can do". |
| (c2) `documented` crate | Missing doc: build. Section: test | Proc-macro dep | No | Yes | Untyped sections. |
| (c3) `strum::EnumMessage` | — | — | — | — | Enum variants only; rules are structs. Not applicable. |
| (d) `#[doc = include_str!]` | As (b) | None | No | As (b) | Costs of (b) and (c1) together. |
| (e) `build.rs` codegen | Build | Build script | No | Yes | Disproportionate (D43). |

### Q6 — Message vs doc

- **(i) Separate texts (SOTA).** "Why is this bad?" is the longer, authoritative rationale; `ViolationTemplate::rationale` stays a one-sentence version. The style guide keeps them consistent.
- **(ii) One text.** `why_is_this_bad` points at `TEMPLATE.rationale.base` (possible only with Q4 (a)). Strict G5, but the doc can never be longer than a diagnostic line, and per-language rationale overrides have no place in the doc.
- **(iii) Hybrid.** The doc's "Why" defaults to the template rationale and may add further prose after it.
- In every option, `explain` can show the **raw `ViolationTemplate`** (placeholders intact, per-language overrides labelled), as Ruff does, so that text is never retyped.

### Q7 — Surfaces

- **Commands**: `rules [--tag …] [--format plain|json]` (list) and `explain <rule> [--format plain|json]` (show one: doc, tags with D36 provenance, options, raw message templates). Open: subcommands vs flags, given both binaries take positional paths; one shared implementation for both binaries.
- **Catalog**: no surveyed tool commits a generated rule catalog without a website behind it. Commands alone meet H4/H5. A generated `docs/rules.md` with a golden-file drift test (Clippy pattern) is a small, separate ROADMAP item.

### Q8 — Tags and doc pointers in diagnostics

- **Plain**: one footer per run pointing at `explain` (rustc).
- **JSON**: either `tags` (and `summary`) on every diagnostic (Semgrep), or a deduplicated top-level `rules` map (ESLint `json-with-metadata`). Embedding the full doc per diagnostic (rustc `code.explanation`) bloats agent input.
- The second option changes the JSON shape from an array to an object, which breaks any consumer.

### Q10 — Options docs

- **(a) Typed keys + real defaults.** A `KEYS: &[OptionKey]` const on each of the 4 value shapes (`ThresholdConfig`, `DenyListConfig`, `AllowListConfig`, `EnforcementConfig`), `RuleDoc.options: Option<RuleOptions { keys, defaults: &DEFAULT_* }>`, `Display` on `FilterListDefaults` / `LanguageDefaults`, and a serde test comparing keys to fields. No dependency; `configurable` becomes derived. Risk: nothing ties the shape named in the doc to the one `check_file` reads.
- **(b) References only** (Ruff style): the doc lists option keys; a test checks they exist. Defaults not shown.
- Rejected: `schemars` (cannot see per-rule default consts; the `flatten`s produce awkward schemas; its real use is a config JSON Schema, a separate ROADMAP item), `documented::DocumentedFields` (developer text, no types or defaults), `serde-reflection` (no `flatten` support *(verify)*).

### A8 — Markdown in the terminal

- **Plain Markdown** (Ruff, Clippy): zero cost, best for agents.
- A styled renderer (`pulldown-cmark` + ~60 lines gated on `IsTerminal` / `NO_COLOR`) is a later ROADMAP item. `termimad` (crossterm) and `mdcat` / `bat` (syntect) are too heavy.

### Q11 — Style guide enforcement

- One registry unit test over every rule: a doc exists (compile-time with Q4 (a)), fields are non-empty, the summary is one line, no `#` headings inside fields, referenced options exist.
- Everything else is the written guide plus review (every surveyed tool).

### Q12 — Adjacent topics

- **References**: a free-form list of `{title, url}` (DEF3). Covers "backed by" and overlap links.
- A typed overlap field (Biome `sources` + `SameLogic` / `Inspired`) only pays off with a consumer such as config migration. ROADMAP.
- "See also" between Omni rules: free-form in References for now.

### Q13 — File layout

- Ruff and Clippy group rules that share detection code; Biome and oxlint use one rule per file.
- In Omni, `no_sleep_in_tests.rs` (two complementary rules sharing `DEFAULT_BANNED_CALLS` and `zero_duration_arg`) and `suppression.rs` (four audits sharing one parser) share logic. Splitting them adds `pub(super)` plumbing and hides the relationship.
- With Q4 (a) or (c), grouping has no doc cost.

---

## 4. Deferred to the ROADMAP (outside this loop, D43)

- JSON output (`--format json`) for `rules` and `explain`.
- `tags` on JSON diagnostics.
- Typed configuration keys with types and real defaults (Q10 (a)).
- Generated in-repo rule catalog with a golden-file drift test.
- Styled Markdown rendering in the terminal.
- JSON Schema for `.omnilint.toml` (`schemars`) for editor completion.
- Typed overlap / sources field (Biome style).
- Whether plain diagnostics should keep printing the full rationale and suggestion on every hit (heavier than any surveyed tool).

---

## 5. Validated Decisions (D45–D53)

- **D45 — Q4**: Each rule declares a const `RuleDoc` struct literal (`impl X { pub(crate) const DOC: RuleDoc }`), registered as a `doc` field on `ClassifiedRule`. A missing section does not compile. No macro, no dependency.
- **D46 — Q6**: The doc and the violation message are separate texts. "Why is this bad?" is the longer, authoritative rationale; `ViolationTemplate::rationale` stays a one-sentence version, kept consistent by the style guide. `explain` shows the raw `ViolationTemplate` (placeholders intact, per-language overrides labelled), so that text is never retyped.
- **D47 — Q7**: Two commands, `rules` (list) and `explain <rule>` (show one), in plain output only. No generated catalog. JSON output and the catalog are ROADMAP items (§4).
- **D48 — Q8**: Plain diagnostic output ends with one footer per run pointing at `explain <rule>`. `tags` on JSON diagnostics is a ROADMAP item.
- **D49 — Q10**: The doc section is named **Configuration** (matches `*Config` types and the README wording; amends D4's "Options"). It lists the rule's keys under `[rules.<name>]`, and a test checks each key exists on the config type. Types and defaults are a ROADMAP item.
- **D50 — A8**: "Plain" is the human-readable text output. For `explain` it is the rule's doc as Markdown, printed as-is.
- **D51 — Output formats**: A shared `OutputFormat` enum (`Plain`, `Json`) parsed by clap replaces the stringly-typed `--format` of both binaries' diagnostics, so an unknown format is an error. `rules` / `explain` get `--format` only when JSON arrives.
- **D52 — Q12**: `References` is a free-form list of `{title, url}` (DEF3, overlap and "see also" links). A typed overlap field is a ROADMAP item.
- **D53 — Q13**: Rules that share detection code stay grouped in one file (`no_sleep_in_tests.rs`, `suppression.rs`); new rules default to one file each. One doc per rule either way.
- **Resulting doc sections (amends D4, D42)**: What it does / Why is this bad? / Configuration / References.
