//! Compile-time tag vocabularies, validated [`TagSet`]s, and named item [`Catalog`]s.

mod catalog;
mod tag;
mod tag_set;
mod vocabulary;

pub use self::catalog::{Catalog, CatalogError, Item, ItemId, LabelError, Selector};
pub use self::tag::{Cardinality, Tag, TagGroup};
pub use self::tag_set::{TagSet, TagSetError};
pub use self::vocabulary::{TagId, TagLookupError, Vocabulary, VocabularyError};
