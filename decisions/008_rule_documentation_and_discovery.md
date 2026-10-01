# ADR 008: In-Tree Rule Documentation, Terminal Rendering, and Discovery Surfaces

## 1. Problem Statement

Following the establishment of the faceted taxonomy and hierarchical selector model in ADR 007, Omni lacked an integrated documentation and discovery mechanism:

1. **No Authoritative In-Code Documentation**: Rule rationale, standard violation messages, and configuration options were scattered across README tables, AST visitor comments, or tests.
2. **Missing Discovery Surface**: Users configuring Omni in `.omnilint.toml` had to inspect source files or read a manual README catalog to discover rules, available tags, or configuration keys.
3. **No Resolution Explanation**: Linters frequently frustrate users when rules do or do not run. There was no way to ask Omni: "Is rule X currently active in my project, and which selector enabled or ignored it?"
4. **README Drift Risk**: The hand-maintained "Rules Catalog" in `README.md` was prone to obsolescence as rules were added or refactored.
5. **Stringly CLI Format Flag**: Both binaries accepted raw strings for `--format` (e.g. `format: String`), failing to leverage `clap::ValueEnum` for typed validation and error suggestions.

---

## 2. Decision

### 2.1. In-Tree Rule Documentation Model (`RuleDoc`)

Every rule declares a static `RuleDoc` next to its `Classification`:

```rust
pub struct RuleDoc {
    pub summary: &'static str,
    pub what_it_does: &'static str,
    pub why_is_this_bad: &'static str,
    pub references: &'static [Reference],
}

pub struct Reference {
    pub title: &'static str,
    pub url: &'static str,
}
```

- **Two-Tier Summaries (`DI10`)**: `summary` is strictly one sentence on one line, ending with a period. It is used in `--list-rules` outputs. `what_it_does` is a comprehensive multi-sentence description detailing covered constructs and explicit exemptions, displayed under `## What it does` in `--explain`.
- **Compile-Time Completeness**: Omitting a field from `RuleDoc` is a compile error (`E0063`).
- **Incremental Content Rollout (`DI12`)**: A `RuleDoc::TODO` placeholder allows progressive rollout across existing rules while maintaining compile-time completeness. Automated style linter tests enforce formatting on all non-placeholder docs.

### 2.2. Discovery CLI Flags (`DI8`, `DI9`, `DI14`, `D50`)

Both `omni-code-lint` and `omni-command-lint` provide symmetric rule discovery for all registered rules:
- `--list-rules`: Lists every rule sorted by name, displaying its name, languages/input, and single-sentence summary.
- `--list-rules --tag <LABEL>`: Filters the list to rules matched by `select = ["<LABEL>"]` (topics, subtopics, facet values, or synonyms).
- `--explain <RULE>`: Renders the full Markdown documentation of a rule, including its message templates, configuration keys, external references, and tag paths.
- **Terminal Plain Markdown Rendering (`D50`)**: Markdown is printed directly as plain text without ANSI escapes, ensuring compatibility across terminal emulators, pagers, and CI logs.
- **Diagnostics Pointer Footer (`D48`, `DI14`)**: Plain diagnostics output appends a single terminal line: `For details on a rule, run: <binary> --explain <rule>`.

### 2.3. Active Configuration Status in `explain` (`DI13`)

`--explain <rule>` checks the current project's `.omnilint.toml` and displays:
```markdown
# <rule-name>

Status: enabled (default)
# or: Status: disabled (not in `select`)
# or: Status: enabled by `select = ["testing"]` (via testing > test-timing)
# or: Status: disabled by `ignore = ["test-doubles"]`
```
This directly resolves the user question "Why is this rule on or off?" by reporting the winning selector and its branch hierarchy.

### 2.4. Strongly-Typed Diagnostic Output Format (`D51`)

`--format` on diagnostic execution uses `OutputFormat`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    Plain,
    Json,
}
```

---

## 3. Verification & Testing

1. **Registry Style Conformance Test**: Every documented rule is tested for:
   - `summary` is exactly one sentence ending with `.`.
   - No markdown `#` headings inside field text (headings are owned by the catalog renderer).
   - Non-empty rationale and valid URLs on references.
   - Placeholder rules are skipped until written.
2. **Architecture DAG Conformance**: Validated by `tests/architecture_conformance.rs`.
3. **Snapshot Tests for Discovery**: CLI tests assert exact snapshots for `--list-rules`, `--tag` filtering, `--explain`, typo corrections, and error exits across both binaries.
4. **Self-Dogfooding**: `omni-code-lint` runs over its own codebase to ensure all doc rendering complies with active rules.

---

## 4. Non-Goals & Future Roadmap

- **Rule Examples (`D42`)**: In-doc code examples (good/bad snippets) are deferred to a dedicated roadmap item.
- **Subcommands (`rules`, `explain`)**: Kept as flags (`--list-rules`, `--explain`) for now; CLI subcommands deferred to roadmap.
- **JSON Output for Discovery**: `--format json` for `--list-rules` / `--explain` deferred to roadmap.
- **Path-Aware Status in `explain`**: Status reports global config resolution; evaluating specific `per_file_ignores` for a supplied file path is deferred to roadmap.
- **README Catalog Regeneration Script (`D44`)**: Generating Markdown files or updating `README.md` from the in-tool catalog is deferred to roadmap.
