//! Structured tag vocabularies, validated item tag sets, named catalogs, boolean filter queries,
//! and include/exclude selection rules for Rust.
//!
//! # Standalone `Vocabulary`, `TagSet`, and `Expr`
//!
//! If your items are already stored in your own structs or keyed by file path, validate each
//! item's tags into a [`TagSet`] and filter them with [`Expr`] or [`TagSelection`]:
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
//!
//! // Aliases resolve in queries (`bpf` -> `ebpf`), and static lints catch contradictions:
//! let alias_query = Expr::parse(&vocabulary, "bpf & !os").unwrap();
//! assert_eq!(alias_query.lint(&vocabulary).len(), 1);
//! ```
//!
//! # Named `Catalog` and `Selection`
//!
//! If items also have unique names that users can type alongside tags (such as lint rules or test
//! cases), put them in a [`Catalog`] and resolve overrides with [`Selection`]. Exact item names
//! beat tags, narrower tags override broader ones, `Exclude` wins ties across independent branches,
//! and [`Selection::decide`] reports which item or tag decided the result:
//!
//! ```
//! use tagonomy::{Cardinality, Catalog, Item, Reason, Selection, Tag, TagGroup, Verdict, Vocabulary};
//!
//! const AREA: TagGroup = TagGroup { name: "Area", cardinality: Cardinality::AtLeastOne };
//! const SPEED: TagGroup = TagGroup { name: "Speed", cardinality: Cardinality::ExactlyOne };
//!
//! const OS: Tag = Tag::new(&AREA, "os", "Operating-system behavior.");
//! const LINUX: Tag = Tag::new(&AREA, "linux", "Linux.").implies(&[OS]);
//! const KERNEL: Tag = Tag::new(&AREA, "kernel", "Kernel.").implies(&[OS]);
//! const EBPF: Tag = Tag::new(&AREA, "ebpf", "eBPF programs.").implies(&[LINUX, KERNEL]);
//! const FAST: Tag = Tag::new(&SPEED, "fast", "Fast.");
//! const SLOW: Tag = Tag::new(&SPEED, "slow", "Slow.");
//!
//! let vocabulary = Vocabulary::new([EBPF, FAST, SLOW]).unwrap();
//! let catalog = Catalog::new(
//!     vocabulary,
//!     [
//!         Item::named("probe-test", [EBPF, SLOW]),
//!         Item::named("syscall-test", [LINUX, FAST]),
//!         Item::named("scheduler-test", [KERNEL, FAST]),
//!     ],
//! )
//! .unwrap();
//!
//! let resolve = |label| catalog.resolve(label).unwrap();
//! let selection = Selection::new(
//!     &catalog,
//!     Verdict::Exclude,
//!     [(resolve("os"), Verdict::Include), (resolve("slow"), Verdict::Exclude)],
//! )
//! .unwrap();
//!
//! let included: Vec<_> = catalog
//!     .items()
//!     .filter(|&item| selection.matches(item))
//!     .map(|item| catalog.name(item))
//!     .collect();
//! assert_eq!(included, ["syscall-test", "scheduler-test"]);
//!
//! // Layer CLI overrides on top of `selection` without mutating it, and explain the
//! // winning decision through its full root-to-source path:
//! let overridden = selection
//!     .override_with([
//!         (resolve("kernel"), Verdict::Include),
//!         (resolve("slow"), Verdict::Include),
//!     ])
//!     .unwrap();
//! let vocabulary = catalog.vocabulary();
//! let probe = catalog.item("probe-test").unwrap();
//! let Reason::Tag { tag, source } = overridden.decide(probe).reason else {
//!     unreachable!()
//! };
//! let chain: Vec<_> = vocabulary
//!     .chain_through(source, tag)
//!     .into_iter()
//!     .map(|id| vocabulary.tag(id).label)
//!     .collect();
//! assert_eq!(chain, ["os", "kernel", "ebpf"]);
//! ```

mod catalog;
mod expr;
mod selection;
mod tag;
mod tag_selection;
mod tag_set;
mod vocabulary;

pub use self::catalog::{Catalog, CatalogError, Item, ItemId, LabelError, Selector};
pub use self::expr::{
    ContradictionKind, Expr, ExprParseError, ExprParseErrorKind, ExprWarning, TautologyKind,
};
pub use self::selection::{Conflict, Decision, Reason, Selection};
pub use self::tag::{Cardinality, Tag, TagGroup};
pub use self::tag_selection::{TagConflict, TagDecision, TagReason, TagSelection, Verdict};
pub use self::tag_set::{TagSet, TagSetError};
pub use self::vocabulary::{TagId, TagLookupError, Vocabulary, VocabularyError};
