# ADR 013: The `tagonomy` Crate

## 1. Problem Statement

The rule taxonomy of [ADR 007](007_rule_taxonomy_and_selection.md) was built from bespoke parts: a `Topic` tree, `Tag`, `Facet` and `Derived` enums, a root-to-leaf branch enumeration for Model B, a label registry, and invariant tests. None of it was specific to lint rules, yet none of it could be reused by another project (a test harness, a plugin registry) without copying it.

No second consumer exists in this repository yet, so extracting the crate now is a deliberate exception to the in-repo reuse criterion of [crate_split_roadmap.md](../crate_split_roadmap.md) §1: while `define_architecture!` could isolate an internal module from `omnicoach` imports, publishing `tagonomy` to crates.io (under `0.0.x` initially while its API settles) makes it reusable across external projects with its own minimal Cargo dependencies.

## 2. Decision

Tags, typed tag groups, validated catalogs and include/exclude selection live in the standalone workspace crate [crates/tagonomy](../../../crates/tagonomy/src/lib.rs), designed from first principles in [tag_crate/](../tag_crate/01_understand.md), with a public surface restricted to `std` and domain-independent types. `omnicoach` is an ordinary consumer.

- **Crate (domain-independent)**: `const` tags (`Tag`, `TagGroup`, `Cardinality`) with aliases and a polyhierarchy (`implies: &'static [Tag]`, where a cycle is a `const` evaluation cycle, `E0391`), a `Vocabulary` (precomputed upward and strict-narrower closure bitsets, shortest `chain` and `root_chain` queries, label registry, unsatisfiable tags), a typed `Catalog<T>` of named items carrying domain values `T` (tag and cardinality validation, label resolution), and an allocation-free bitset `Plan<'a, T>` that decides include/exclude by specificity and explains the decision. Every construction error is reported at once.
- **`omnicoach` (policy)**: the concrete facets and topics (`Topic` wrapping a `Tag`, built by `Topic::root` / `Topic::sub` from a label, a `&'static Topic` parent and a `TopicDoc` struct literal that requires both `description` and `scope_note`; single-valued facets deriving labels and descriptions via `strum`), a single unified `LazyLock<Catalog<RegisteredRule>>` registry, the mapping of `select` and `ignore` to a `Plan`, CLI-over-file precedence, `per-file-ignores`, typo suggestions, and the wording of `--explain`.

The API and migration map are in [04_plan.md](../tag_crate/04_plan.md); the reasons for the polyhierarchy, the two query models, and `const` vocabularies are in [03_design.md](../tag_crate/03_design.md); the comparison of declaration styles behind `Topic`'s current form, and the planned derive, is in [05_declaration_prototypes.md](../tag_crate/05_declaration_prototypes.md).

### Specificity on a polyhierarchy

On a polyhierarchy, root-to-leaf branches no longer generalize: a tag reached along two paths would let a directive on a broad tag resurface past a narrower one through the second path. `Plan` evaluates specificity **per explicit tag**: among the directives on that tag and the tags it implies, only the most specific count, and exclusion wins across all counting directives. On a tree this is exactly ADR 007's Model B, so `omnicoach`'s behavior did not change (outputs of `--list-tags`, `--list-rules` and `--explain` were compared byte for byte before and after the migration).

## 3. Consequences

- `omnicoach` depends on `tagonomy` by path and version; `tagonomy` must be published before an `omnicoach` release that depends on it.
- `omnicoach` restricts `Topic` to a single-parent tree by policy (`Topic::root` / `Topic::sub`), and its `--list-tags` table and `--explain` facet rendering assume single-parent topic chains; supporting multi-parent topics in `omnicoach` would require updating those views alongside `Topic`.
- Deferred until a consumer needs them: boolean tag expressions (`Expr`), and conjunctive directive specificity ([crate_split_roadmap.md](../crate_split_roadmap.md) §3.3).
