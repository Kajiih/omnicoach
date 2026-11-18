# tagonomy

Compile-time faceted tag vocabularies for Rust.

- **Faceted vocabularies**: `const` `Tag` and `TagGroup` definitions with `Cardinality` (`Any`, `AtMostOne`, `AtLeastOne`, `ExactlyOne`), lexical aliases, scope notes, and polyhierarchical `implies` DAGs (`E0391` compile-time cycle rejection).
