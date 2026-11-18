# ADR 013: Standalone `tagonomy` Crate (`Vocabulary`, `TagSet`, `Catalog`, `Expr`, and `Selection`)

## 1. Problem Statement

`omnicoach` classifies rules with hierarchical, grouped tags and resolves `select` / `ignore` lists by specificity ([ADR 007](007_rule_taxonomy_and_selection.md)): tag groups with per-group count rules (for example, exactly one `Consensus` value), parent-tag implications (`jj` implies `vcs`), rejecting redundant parent tags on the same rule (`[jj]`, not `[jj, vcs]`), unique names across rules and tags, and structured decision reasons for `--explain`.

Keeping that engine inside `omnicoach`'s `rule_declaration` and `rule_selection` modules had three limitations:

1. **Reusable Engine Mixed with Linter Concepts ([ADR 012](012_module_placement_by_abstraction.md) P1, P7)**: Tag hierarchies, validated tag sets, include/exclude override rules, and boolean tag queries are general tools usable by CLI runners, test suites, note organizers, and plugin registries—none of which know about `omnicoach`'s `RuleName`, `RuleTarget`, or `.omnicoach.toml`.
2. **Tag Validation Coupled to Named-Item Registries**: Callers that attach tags directly to structs or file paths want per-item tag validation (`TagSet`), boolean filtering (`Expr`), and tag override rules (`TagSelection`) without inventing globally unique selector names in a `Catalog<T>`.
3. **No Boolean Query Support Alongside Config Overrides**: Config files work well with flat `select` / `ignore` lists where narrower tags override broader ones, whereas CLI filters (`(linux | kernel) & !slow`) need `AND` / `OR` / `NOT` expressions, query-hygiene warnings (`ebpf & !os`, `fast & slow`), and an exact way to combine a config selection with an ad-hoc CLI filter.

Supporting scope, SOTA references, and detailed design live in [01_understand.md](../tag_crate/01_understand.md), [02_references.md](../tag_crate/02_references.md), and [03_design.md](../tag_crate/03_design.md).

## 2. Decision

Extract the domain-independent tag, validation, query, and selection engine into a standalone workspace crate, [crates/tagonomy](../../../crates/tagonomy/src/lib.rs), organized as a seven-module dependency DAG over two lightweight dependencies (`fixedbitset` and `thiserror`; `wasm32-unknown-unknown`-compatible, with no custom proc-macro crate):

| Layer | Module | Responsibility |
| :--- | :--- | :--- |
| **Definitions & Vocabulary** | [tag.rs](../../../crates/tagonomy/src/tag.rs), [vocabulary.rs](../../../crates/tagonomy/src/vocabulary.rs) | Plain `const` `Tag`, `TagGroup`, and `Cardinality` (`Any`, `AtMostOne`, `AtLeastOne`, `ExactlyOne`) compiled into an indexed `Vocabulary` (`TagId`) with precomputed implied-tag bitsets, alias lookups, and conflict checks for exclusive groups. |
| **Validated Items** | [tag_set.rs](../../../crates/tagonomy/src/tag_set.rs), [catalog.rs](../../../crates/tagonomy/src/catalog.rs) | Standalone `TagSet` ensuring each item lists only its most specific tags and obeys group rules; optional `Catalog<T>` adding a shared item-and-tag selector namespace plus `Catalog::new_with` (validates everything first, then hands each item's `ItemId` to your constructor). |
| **Boolean Queries** | [expr.rs](../../../crates/tagonomy/src/expr.rs) | Boolean `Expr` (`All`, `None`, `Tag`, `ExplicitTag`, `Not`, `And`, `Or`), zero-dependency string parser (`Expr::parse`), and static query checks (`Expr::lint`) that push `!` inward so negated queries get the same warnings as positive ones. |
| **Include/Exclude Overrides** | [tag_selection.rs](../../../crates/tagonomy/src/tag_selection.rs), [selection.rs](../../../crates/tagonomy/src/selection.rs) | `TagSelection<'a>` over `&'a Vocabulary` (narrower tags override broader ones, `Exclude` wins ties across branches, `override_with` layering, `TagDecision` explains what won, `to_expr() -> Expr` converts to an equivalent boolean query) and `Selection<'a, T>` over `&'a Catalog<T>` (exact item names beat tags, `override_with` layering, `ineffective()` returns `(Selector, Verdict)` pairs without cloning bitsets). |

Key architectural invariants:

- **Opt-in layers**: Callers with unnamed items stop at `Vocabulary` + `TagSet` + `Expr` / `TagSelection`; callers whose items also act as named selectors add `Catalog<T>` + `Selection<'a, T>`. Neither `TagSelection` nor `Expr` depends on `catalog`.
- **Validate-then-build `Catalog::new_with`**: Validates every item name and `TagSet` before calling the user's builder closure `(ItemId, &TagSet, S) -> T`, so `omnicoach`'s `RegisteredRule` ([src/rule_selection/taxonomy.rs](../../../src/rule_selection/taxonomy.rs)) stores its `ItemId` directly with zero name lookups later.
- **Exact conversion to `Expr` (`TagSelection::to_expr`)**: Evaluates each vocabulary tag once and builds an `Expr` over `Expr::ExplicitTag` nodes so `tag_selection.to_expr().matches(&tags) == tag_selection.matches(&tags)` for every valid `TagSet`.
- **Encapsulation inside `omnicoach`**: In [src/architecture.rs](../../../src/architecture.rs), only `RuleDeclaration` and `RuleSelection` declare `owns_external: ["tagonomy"]`. `Topic` stays a single-parent tree wrapper (`Topic::root`, `Topic::sub`) in [src/rule_declaration/taxonomy.rs](../../../src/rule_declaration/taxonomy.rs), and `RuleCatalog`, `Config`, rules, and runners stay `tagonomy`-free.

## 3. Excluded and Deferred Scope

- **Excluded — Tag-attached field schemas ("supertags") and host config formats**: Attaching typed key-value fields to a tag (like `due_date: Date`), JSON Schema generation, `.omnicoach.toml` parsing, and file-path globs (`per-file-ignores`) are separate concerns that belong in layers above `tagonomy`.
- **Deferred ([ROADMAP.md](../../../ROADMAP.md) §5)**:
  - Runtime config-loaded vocabularies (loading user-defined tag hierarchies from `.toml` / `.json` / SQLite with explicit cycle detection) once a real Rust consumer needs them.
  - Optional `#[derive(tagonomy::Group)]` proc-macro crate if external consumers define enough enum-backed groups to justify a companion macro crate.
  - Multi-tag override rules (for example, overriding items that carry both `linux` and `slow` together) if single-selector lists plus boolean `Expr` ever prove insufficient.
  - Pre-`0.1` API polish items tracked on [ROADMAP.md](../../../ROADMAP.md) §5 (cross-vocabulary ID safety, stronger `Expr::lint` checks, owned selections, `?`-friendly error wrappers, `Tag`/`TagGroup` encapsulation, and retiring [scripts/publish_crates.py](../../../scripts/publish_crates.py) once `tagonomy` moves to its own repository).

## 4. Consequences

- `omnicoach` delegates parent-tag expansion, label collision checks, group-rule enforcement, override precedence, and ineffective-selector warnings to `tagonomy` while preserving all 8 `compile_fail` doctests and byte-identical CLI outputs.
- While `tagonomy` incubates inside this workspace, publishing to crates.io runs through [scripts/publish_crates.py](../../../scripts/publish_crates.py), which checks crates.io first and passes `--exclude` for already-published crate versions (and non-crates.io crates) so partial release retries stay idempotent ([releasing.md](../releasing.md)).

## References

- **ISO 25964-1:2011 & W3C SKOS** ([SKOS Reference](https://www.w3.org/TR/skos-reference/)) — Establishes separating canonical tag identity (`TagId`, preferred `label`) from aliases and boundary `scope_note`s.
- **Ranganathan (1937), *Prolegomena to Library Classification*; Yee et al. (2003), *Faceted Metadata for Image Search and Browsing*, ACM CHI** ([doi:10.1145/642611.642681](https://doi.org/10.1145/642611.642681)) — Motivates splitting tags into independent `TagGroup` categories with explicit per-group count rules (`Cardinality`).
- **Ganter & Wille (1999), *Formal Concept Analysis: Mathematical Foundations*, Springer** ([doi:10.1007/978-3-642-59830-2](https://doi.org/10.1007/978-3-642-59830-2)) — Establishes why storing only the most specific tags per item uniquely determines the full set of implied tags.
- **Baader, Brandt & Lutz (2005), *Pushing the $\mathcal{EL}$ Envelope*, IJCAI** ([ijcai.org/Proceedings/05/Papers/0372.pdf](https://www.ijcai.org/Proceedings/05/Papers/0372.pdf)) — Shows why checking tag implications and exclusive-group conflicts runs in fast polynomial time using precomputed parent bitsets instead of general SAT solving.
- **Poole (1985), *On the Comparison of Theories: Preferring the Most Specific Explanation*, IJCAI; OASIS XACML v3.0 (2013)** ([docs.oasis-open.org/xacml/3.0](https://docs.oasis-open.org/xacml/3.0/xacml-3.0-core-spec-os-en.html)) — Grounds letting narrower tags override broader parents while breaking ties across independent branches in favor of `Exclude`.
