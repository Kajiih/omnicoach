//! Compile-time tag vocabularies, validated [`TagSet`]s, named item [`Catalog`]s, and
//! boolean [`Expr`] queries.
//!
//! # Standalone `Vocabulary`, `TagSet`, and `Expr`
//!
//! ```
//! use tagonomy::{Cardinality, Expr, Tag, TagGroup, TagSet, Vocabulary};
//!
//! const AREA: TagGroup = TagGroup { name: "Area", cardinality: Cardinality::AtLeastOne };
//! const SPEED: TagGroup = TagGroup { name: "Speed", cardinality: Cardinality::ExactlyOne };
//!
//! const OS: Tag = Tag::new(&AREA, "os", "Operating-system behavior.");
//! const LINUX: Tag = Tag::new(&AREA, "linux", "Linux.").implies(&[OS]);
//! const KERNEL: Tag = Tag::new(&AREA, "kernel", "Kernel.").implies(&[OS]);
//! const EBPF: Tag = Tag::new(&AREA, "ebpf", "eBPF programs.")
//!     .implies(&[LINUX, KERNEL])
//!     .aliases(&["bpf"]);
//! const FAST: Tag = Tag::new(&SPEED, "fast", "Fast.");
//! const SLOW: Tag = Tag::new(&SPEED, "slow", "Slow.");
//!
//! let vocabulary = Vocabulary::new([EBPF, FAST, SLOW]).unwrap();
//! let probe = TagSet::new(&vocabulary, [EBPF, SLOW]).unwrap();
//! let syscall = TagSet::new(&vocabulary, [LINUX, FAST]).unwrap();
//!
//! let query = Expr::parse(&vocabulary, "(linux | kernel) & !slow").unwrap();
//! assert!(query.lint(&vocabulary).is_empty());
//! assert!(!query.matches(&probe));
//! assert!(query.matches(&syscall));
//! ```

mod catalog;
mod expr;
mod tag;
mod tag_set;
mod vocabulary;

pub use self::catalog::{Catalog, CatalogError, Item, ItemId, LabelError, Selector};
pub use self::expr::{
    ContradictionKind, Expr, ExprParseError, ExprParseErrorKind, ExprWarning, TautologyKind,
};
pub use self::tag::{Cardinality, Tag, TagGroup};
pub use self::tag_set::{TagSet, TagSetError};
pub use self::vocabulary::{TagId, TagLookupError, Vocabulary, VocabularyError};
