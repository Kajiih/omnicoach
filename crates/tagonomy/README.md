# tagonomy

Compile-time faceted tag vocabularies for Rust.

- **Faceted vocabularies**: `const` `Tag` and `TagGroup` definitions with `Cardinality` (`Any`, `AtMostOne`, `AtLeastOne`, `ExactlyOne`), lexical aliases, scope notes, and polyhierarchical `implies` DAGs (`E0391` compile-time cycle rejection).
<!-- TODO: What does antichain mean? and FixedBitSet, this is all hard to understand -->
- **Standalone `TagSet` & optional `Catalog<T>`**: Validate explicit antichains and precomputed `FixedBitSet` closures per item without requiring item names, or combine items and tags in one selector namespace with two-pass key-aware `Catalog::new_with`.
