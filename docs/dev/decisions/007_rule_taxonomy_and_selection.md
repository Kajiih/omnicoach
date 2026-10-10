# Faceted Rule Taxonomy, Hierarchical Selection, and Tag-Free Rule Execution

## 1. Problem Statement

Omni's initial rule tagging and selection mechanism in `src/core.rs` and `src/code_lint/runner.rs` had five structural defects:

1. **Mixed Axes in One Flat Enum**: `Tag` combined subject domains (`Testing`, `Naming`), rule dispositions (`Heuristic`, `Opinionated`), derived languages (`Python`, `Rust`), and an execution-routing flag (`Suppression`) in a single flat `strum` enum. A rule could omit its precision or consensus classification with no compiler or test error.
2. **Flat Membership Without Hierarchy**: `Rule::has_tag` checked flat slice membership. Adding a specific tag (`JJ`) forced every rule to manually repeat its broader ancestors (`Vcs`) or lose broad selection, and nothing prevented a rule from forgetting a parent tag.
3. **Silent Config Footguns & Drift**: `Tag::Cli` had zero rules (`select = ["cli"]` silently matched nothing), `Workflow`, `Vcs`, and `JJ` selected the exact same singleton set, and `Safety` covered a typing rule while its description claimed operational command restrictions.
4. **Coarse Precedence**: Any matching `ignore` beat any `select`, making it impossible to ignore a broad tag (`ignore = ["testing"]` or `ignore = ["vcs"]`) while re-enabling a child tag (`select = ["jj"]`) or a specific rule (`select = ["no-sleep-in-tests"]`).
5. **Behaviour Coupled to Tags**: `src/code_lint/runner.rs` branched on `rule.tags().contains(&Tag::Suppression)` in four places to separate suppression-directive audits from ordinary AST rules, and `Rule::tags()` exposed tags to rule implementations and runners.

Full exploration records, SOTA survey, experiments (E1/E2), and five competing prototypes (`P0`–`P4`) are documented in [docs/dev/rule_docs_and_tags/t1/](../docs/dev/rule_docs_and_tags/t1/) (`D1`–`D38`). Contributor guidelines and yes/no classification tests live in [docs/dev/tag_guide.md](../docs/dev/tag_guide.md).

---

## 2. Decision

### 2.1. Faceted Classification Model (`D17`, `D19`–`D21`, `D23`, `D28`, `D38`)

Every rule is classified across seven orthogonal **facets**—four declared in the rule's own file and three derived automatically:

| Facet (display label) | Cardinality | Values | Source |
|---|---|---|---|
| **Topic** | `>= 1` (most specific only) | 22-topic tree (§2.2) | Declared on `Classification::topics` |
| **Precision** | Exactly `1` | `exact`, `heuristic` | Declared on `Classification::precision` |
| **Consensus** | Exactly `1` | `opinionated`, `unopinionated` | Declared on `Classification::consensus` |
| **Impacted quality** | Exactly `1` | `reliability`, `maintainability` (from ISO/IEC 25010) | Declared on `Classification::impacted_quality` |
| **Languages** | `>= 1` (code) / `0` (command) | `python`, `rust` | Derived from `supported_languages()` |
| **Analyzed input** | Exactly `1` | `code`, `command` | Derived from the rule registry |
| **File scope** | `0..=1` | `tests-only`, `source-only` | Derived from `RuleTarget` |

```rust
pub struct Classification {
    pub topics: &'static [Topic],
    pub precision: Precision,
    pub consensus: Consensus,
    pub impacted_quality: ImpactedQuality,
}
```

- **Compile-Time Completeness**: Because each single-valued declared facet is a mandatory struct field typed by its own enum, omitting a facet (`E0063`), supplying two values (`E0062`), placing a facet value in `topics` (`E0308`), or attempting to declare a derived facet (`E0560`) is a compile error.
- **Single Global Namespace (`DI5`)**: Rule names, canonical topic labels, facet value labels, and topic synonyms share one flat selector namespace (`heuristic`, not `precision:heuristic`). Facet display labels (`Topic`, `Precision`, `Consensus`, `Impacted quality`, `Languages`, `Analyzed input`, `File scope`) are never valid selectors (`D19`).
- **Orthogonal Boundary Between Topic and Impacted Quality (`D38`)**: Topics name the *code construct, API, or domain inspected* (`testing`, `static-typing`, `async`), whereas `ImpactedQuality` names the *ISO/IEC 25010 software quality that suffers when violated* (`reliability`, `maintainability`, and future ISO values when first needed by a rule). Quality words (`security`, `performance`, `reliability`, `maintainability`, `safety`) are never topics.

### 2.2. Topic Tree as Plain `const` Structs (`D16`, `D24`, `D25`, `D32`–`D34`)

`Topic` is a plain `struct` (`label`, `parent: Option<&'static Topic>`, `description`, `scope_note`, `synonyms`), with each topic declared in **one spot** as an associated `const` on `Topic` (`Topic::NAMING`, `Topic::JJ`, …):

```rust
pub struct Topic {
    pub label: &'static str,
    pub parent: Option<&'static Self>,
    pub description: &'static str,
    pub scope_note: &'static str,
    pub synonyms: &'static [&'static str],
}
```

1. **Single-Spot Declaration & Mandatory Documentation (`C2`, `D32`)**: Every topic's `label`, `parent`, `description`, `scope_note`, and `synonyms` (`version-control` → `vcs`, `jujutsu` → `jj`) live in a single struct literal. Omitting `description` or `scope_note` is a compile error (`E0063`).
2. **Native Compile-Time Cycle Prevention (`D34`, `E0391`)**: Because each topic is a `const` and `parent` is `Option<&'static Topic>`, any cycle in `parent` links fails to compile with `rustc`'s native `E0391` (`cycle detected when const-evaluating`), regardless of declaration order. There is no arbitrary maximum depth limit.
3. **Active Topics Derived from Rules & Unused-Topic Detection (`M10`)**: `RuleSelection` collects `all_topics()` by walking registered rules' `classification.topics` and their `.ancestors()` (no second `ALL` list or macro). In production, `pub(crate) const` topic declarations that are not referenced by any rule or child topic are caught at compile time by `rustc`'s `dead_code` lint.
4. **Encapsulated Hierarchy Traversal (`D33`)**: Selection and description callers in `RuleSelection` read hierarchy exclusively through `ancestors()` and `path()`, never reading `Topic::parent` directly.
5. **Most-Specific Rule Tagging**: A rule lists only its most specific topics (`&[Topic::TEST_TIMING]`, never `&[Topic::TESTING, Topic::TEST_TIMING]`), enforced by unit test.

### 2.3. Selection Precedence Model B and Config Validation (`D15`, `D18`, `D35`)

Every rule has a set of **branches**: one root-to-leaf path per declared topic (`[testing, test-timing]`), plus one single-element path per declared or derived facet value (`[exact]`, `[unopinionated]`, `[reliability]`, `[python]`, `[code]`, `[tests-only]`). The rule's own name (`no-sleep-in-tests`) is the implicit leaf at the end of every branch (`D15`).

1. **Along One Branch — Nearest Wins**: The selector closest to the rule leaf on a branch determines that branch's verdict. This supports both narrowing an ignore (`ignore = ["vcs"], select = ["jj"]` or `ignore = ["testing"], select = ["no-sleep-in-tests"]`) and carving an exception out of a select (`select = ["testing"], ignore = ["test-doubles"]`).
2. **Across Branches — Ignore Wins**: If any branch resolves to `Ignore`, the rule is disabled; otherwise, if any branch resolves to `Select`, the rule is enabled.
3. **Default When Unmatched**: When no branch matches any selector, the rule is enabled iff `select` is absent from the configuration.
4. **Per-File Ignores**: `per_file_ignores` is evaluated after main selection as a subtract-only stage. `D15` (rule name beats tag) applies within main selection, not across stages: a tag in `per_file_ignores` subtracts matching rules on that path even if globally selected by rule name.
5. **Loud Config Validation**: String parsing occurs only in the config adapter. It rejects:
   - unknown selector labels (reporting the closest registered label via case-insensitive Levenshtein edit distance when within `max(1, len / 3)`);
   - facet display labels used as selectors, matched case-insensitively in both space-separated (`"Impacted quality"`) and `kebab-case` (`"impacted-quality"`) forms (listing the facet's valid values);
   - the same canonical selector appearing in both `select` and `ignore` (after resolving synonyms).
   If cheap to surface from `Config::load`, a warning is emitted when a selector entry changes no rule's outcome (`D35`).

### 2.4. Architectural Isolation of Taxonomy and Suppression Audits (`DI4`, `D37`)

Rules and runners must never branch on tags (`M13`):

1. **No Tag Access in Runners or Rules**:
   - Rule contracts expose no tag getters or `has_tag` helpers, and runners are verified by `tests/architecture_conformance.rs` never to read `Classification` or `Topic`.
   - Each rule's `Declaration` (`RuleDeclaration`) includes `classification: Classification` alongside its name, template, languages, options, and doc (`ADR 009`), so registering an unclassified rule does not compile.
2. **Eager `RuleName` Resolution Across the Runner Boundary**:
   - Because no facet depends on the runtime file `path` and the rule registries are static, `RuleSelection` eagerly resolves all tag and synonym selectors in `select`, `ignore`, and `per_file_ignores` at config load time into tag-free `RuleName` sets on `config::Config` (`disabled_rules: HashSet<RuleName>` and `per_file_ignores: Vec<(GlobMatcher, HashSet<RuleName>)>`).
   - `config::Config::is_rule_enabled` and `config::Config::is_rule_enabled_for_path` live in `Config` (`src/config.rs`) and check only `RuleName`, with zero tag visibility in runners or rules.
   - Selection is resolved only by `rule_selection::load_config` / `rule_selection::parse_config`.
3. **Separate Suppression-Audit Registry**:
   - The four suppression meta-rules (`MISSING_SUPPRESSION_REASON`, `UNUSED_SUPPRESSION`, `UNKNOWN_SUPPRESSION_RULE`, `BLANKET_SUPPRESSION`) live in their own `SUPPRESSION_AUDITS: &[Declaration]` registry in `CodeSuppressionEngine` rather than checking a tag in `src/code_lint/runner.rs`. Both code rules and suppression-audit rules derive the `code` analyzed-input value.

---

## 3. Verification Suite

The verification suite lives in production: `compile_fail` doctests on `Classification` in `src/rule_declaration/taxonomy.rs` and invariant tests in `src/rule_selection/taxonomy.rs`:

- **Compile-Time Checks (7 `compile_fail` Doctests)**:
  - Missing or duplicate single-valued facet field (`M1`: `E0063`, `M2`: `E0062`)
  - Facet value in `topics` or non-topic parent (`M5`: `E0308`, `M11`: `E0308`)
  - Hand-declared derived facet (`M6`: `E0560`)
  - Unknown topic constant (`M7`: `E0599`)
  - Cycle in `Topic` parent links (`M8`: `E0391`, `D34`)
  - Tag-free `Rule` contract (`M13`, `D37`)
- **Taxonomy Invariant Tests**:
  - `every_rule_has_a_topic` (`M3`)
  - `no_rule_lists_a_topic_with_its_ancestor_or_twice` (`M4`)
  - `labels_are_globally_unique_and_not_facet_labels` (`M9`, `DI5`, `D19`, including `kebab-case` syntax and multi-word facet label checks)
  - `every_tag_has_a_rule` (`M10`, across all topics and facet values in `all_tags()`; unused `pub(crate) const` topics also fail `dead_code` at compile time)
  - `every_topic_is_documented` (non-empty `description` and `scope_note` on every topic; facet values are covered by the `missing_docs` lint)
