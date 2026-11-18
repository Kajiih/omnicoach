//! Compile-time tag vocabularies: tag groups and tags with aliases and a polyhierarchy.

mod tag;
mod vocabulary;

pub use self::tag::{Cardinality, Tag, TagGroup};
pub use self::vocabulary::{TagId, TagLookupError, Vocabulary, VocabularyError};
