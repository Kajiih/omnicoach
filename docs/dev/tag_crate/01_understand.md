# `tagonomy` Crate: Scope & Problem Analysis

## TL;DR

- **Reusable tag engine**: `tagonomy` provides compile-time tag vocabularies (`Tag`, `TagGroup`, `Cardinality`, multi-parent `implies`), validated per-item tag sets (`TagSet`), optional named-item catalogs (`Catalog<T>`), boolean filter expressions (`Expr`), and include/exclude override rules (`TagSelection` and `Selection`).
- **Opt-in layers (`Vocabulary` → `TagSet` → `Catalog<T>`)**: Separating per-item tag validation from named catalogs lets callers with unnamed or file-path-indexed items validate tags and run queries without inventing unique selector names, while `omnicoach` and similar rule/test registries get unified item-and-tag lookup through `Catalog<T>`.
- **Clear scope boundary**: `tagonomy` handles attaching and querying sets of tags on items; typed key-value fields attached to tags (like `due_date: Date`), host config files, and file-path globs belong in layers above `tagonomy`.

---

## 1. Problem & Consumer Patterns

Tools that organize items with hierarchical, grouped tags fall into two common patterns:

1. **Named-item selector registries (`omnicoach` rules, test suites, check catalogs)**:
   - Every item has a globally unique name (`sleep-in-tests`) that shares one flat selector namespace with tag labels (`testing`, `test-timing`, `reliability`) and aliases (`jujutsu` → `jj`).
   - Users write config-style `select` / `ignore` lists where narrower tags override broader parents (`ignore = ["vcs"], select = ["jj"]`), exact item names beat tags, `Exclude` wins ties across independent branches, and commands like `--explain` report which rule or tag decided the result (`Decision` and `Reason`).
2. **Standalone tagged records (notes, documents, assets, repository metadata)**:
   - Items are already identified by file path, database ID, or struct ownership rather than a selector name in the tag namespace, and store a `TagSet` directly on the struct.
   - Users filter items with boolean CLI queries (`(linux | kernel) & !slow`), optionally combined with a baseline `TagSelection` profile (`tag_selection.to_expr() & cli_expr`), and get static warnings when a query can never match (`ebpf & !unix`, `fast & slow`), always matches (`!fast | !slow`), or repeats an implied tag (`unix & linux`).

Coupling tag validation and override rules to `Catalog<T>` forces Pattern 2 callers into a named-item registry they do not need, while omitting `Expr` leaves CLI filtering to one-off parsers in every application.

---

## 2. Scope Boundaries

### In Scope (`v0.0.1`)

- **Compile-time `const` tag declarations**: `Tag`, `TagGroup`, and `Cardinality` (`Any`, `AtMostOne`, `AtLeastOne`, `ExactlyOne`) with multi-parent `implies: &'static [Tag]` (`E0391` compile-time error on circular `const` links), canonical `label`, `aliases`, `description`, and optional `scope_note`.
- **Validated `Vocabulary` and standalone `TagSet`**: Global label/alias/group uniqueness checks, exclusive-group conflict checks, precomputed implied-tag bitsets, shortest parent chains (`chain`, `root_chain`, `chain_through`), redundant-parent rejection (`RedundantTag`: `[ebpf, linux]` fails when `ebpf` implies `linux`), and group count/exclusivity validation.
- **Named `Catalog<T>`**: Unified item/tag selector namespace, `Catalog::new`, validate-then-build `Catalog::new_with` (passing each item's `ItemId` and `&TagSet` to the builder closure only after all items pass validation), and `Catalog::unused_tags`.
- **Boolean `Expr` and include/exclude `TagSelection` / `Selection`**:
  - `Expr`: `All`, `None`, `Tag`, `ExplicitTag`, `Not`, `And`, `Or`; `&`, `|`, `!` operators; zero-dependency string parser (`Expr::parse`); static query checks (`Expr::lint`) that push `!` inward so negated queries receive the same diagnostics as positive ones.
  - `TagSelection<'a>`: Narrower-overrides-broader `Include` / `Exclude` rules over `&'a Vocabulary` with `override_with` layering, `TagDecision` reasons, and exact `to_expr() -> Expr` conversion.
  - `Selection<'a, T>`: Wraps `TagSelection<'a>` over `&'a Catalog<T>` with exact `Selector::Item` overrides, `override_with` layering, and `ineffective()` auditing returning `(Selector, Verdict)` pairs without cloning bitsets.

### Out of Scope

- **Tag-attached field schemas ("supertags") and JSON Schema exporters**: Validating typed key-value fields attached to a tag (for example, requiring a `due_date: Date` field whenever an item carries `#task`) needs scalar types, range rules, and serialization crates (`serde`, `toml`, `serde_json`) unrelated to tag-set matching. A higher-level schema layer can map `TagId -> FieldContract` on top of `tagonomy` without changing `Vocabulary` or `TagSet`.
- **Host configuration formats and path globs**: `.omnicoach.toml` deserialization, `per-file-ignores` glob matching, and typo suggestions stay in host applications.
- **Runtime config-loaded vocabularies and proc-macro derives in `v0.0.1`**: Tracked on [ROADMAP.md](../../../ROADMAP.md) §5 with clear triggers so `v0.0.1` stays small and `const`-friendly.

---

## 3. Scale & Runtime Targets

- **Vocabulary size**: `10–500` tags across `1–15` groups (backed by `FixedBitSet`, so there is no 64-tag limit and multi-word bitsets work transparently).
- **Fast evaluation paths**: Once built, `TagSet::carries`, `Expr::matches`, `TagSelection::decide`, and `Selection::decide` allocate zero heap memory during evaluation; `Selection::ineffective` allocates only its selector/baseline/result `Vec`s and avoids cloning bitsets while probing each directive.
- **Portability**: Pure Rust (`fixedbitset` and `thiserror`), compiling cleanly to `wasm32-unknown-unknown` with zero host imports ([ADR 014](../decisions/014_distribution_and_wasm_targets.md)).

## References

- **[ADR 007: Faceted Rule Taxonomy, Hierarchical Selection, and Tag-Free Rule Execution](../decisions/007_rule_taxonomy_and_selection.md)** — Defines `omnicoach`'s six-facet rule classification and selection precedence rules.
- **[ADR 012: Module Placement by Abstraction](../decisions/012_module_placement_by_abstraction.md)** — Establishes placing domain-independent engines in modules named in their own vocabulary.
- **[ADR 013: Standalone tagonomy Crate](../decisions/013_tagonomy_crate.md)** — Records the architectural decision and component boundaries for `crates/tagonomy`.
