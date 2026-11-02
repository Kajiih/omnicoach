//! Compile-time tag vocabularies: tag groups, tags with aliases and a polyhierarchy,
//! validated catalogs of tagged items, and specificity-ordered include/exclude selection with
//! structured explanations.
//!
//! Design: [`docs/dev/tag_crate/`](https://github.com/Kajiih/omnicoach/tree/main/docs/dev/tag_crate).
//!
//! # Example
//!
//! Tags are `const` definitions. `ebpf` implies both `linux` and `kernel`, which both imply
//! `os`; `Speed` is a group whose items carry exactly one value. A consumer that wants the type
//! checker to keep tags of different groups apart wraps `Tag` in its own type per group.
//!
//! ```
//! use tagonomy::{Cardinality, Catalog, Item, Plan, Reason, Tag, TagGroup, Verdict, Vocabulary};
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
//! // Run operating-system tests, except slow ones.
//! let selector = |label| catalog.resolve(label).unwrap();
//! let plan = Plan::new(
//!     &catalog,
//!     Verdict::Exclude,
//!     [(selector("os"), Verdict::Include), (selector("slow"), Verdict::Exclude)],
//! )
//! .unwrap();
//! let included: Vec<_> = catalog
//!     .items()
//!     .filter(|&item| plan.decide(item).verdict == Verdict::Include)
//!     .map(|item| catalog.name(item))
//!     .collect();
//! assert_eq!(included, ["syscall-test", "scheduler-test"]);
//!
//! // Every decision says why. `scheduler-test` carries `os` through `kernel`.
//! let vocabulary = catalog.vocabulary();
//! let scheduler = catalog.item("scheduler-test").unwrap();
//! let Reason::Tag { tag, source } = plan.decide(scheduler).reason else {
//!     unreachable!()
//! };
//! let chain: Vec<_> = vocabulary
//!     .chain(source, tag)
//!     .iter()
//!     .map(|&id| vocabulary.tag(id).label)
//!     .collect();
//! assert_eq!(chain, ["os", "kernel"]);
//!
//! // `probe-test` carries `os` through `ebpf`, but `slow` is an independent reason, and
//! // exclusion wins.
//! let probe = catalog.item("probe-test").unwrap();
//! let decision = plan.decide(probe);
//! assert_eq!(decision.verdict, Verdict::Exclude);
//! let Reason::Tag { tag, .. } = decision.reason else { unreachable!() };
//! assert_eq!(vocabulary.tag(tag).label, "slow");
//! ```

mod catalog;
mod plan;
mod tag;
mod vocabulary;

pub use self::catalog::{Catalog, CatalogError, Item, ItemId, LabelError, Selector};
pub use self::plan::{Conflict, Decision, Plan, Reason, Verdict};
pub use self::tag::{Cardinality, Tag, TagGroup};
pub use self::vocabulary::{TagId, Vocabulary, VocabularyError};
