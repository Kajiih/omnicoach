# tagonomy

Structured tag vocabularies, validated item tag sets, named item catalogs, boolean filter queries, and include/exclude selection rules for Rust.

## What It Gives You

- **Compile-time tag vocabularies (`Tag`, `TagGroup`, `Vocabulary`)**: Define tags as plain `const` values organized into groups (`TagGroup`) with rules for how many tags an item may carry (`Any`, `AtMostOne`, `AtLeastOne`, `ExactlyOne`). Tags can imply one or more broader tags (for example, `ebpf` implies both `linux` and `kernel`, and `linux` implies `os`), accept aliases (`bpf` → `ebpf`), and carry descriptions and scope notes. Circular `implies` links fail to compile (`E0391`).
- **Validated item tags (`TagSet`) & optional named registries (`Catalog<T>`)**: Validate one item's tags into a `TagSet` without inventing unique item names. `TagSet` rejects duplicates, redundant parent tags (listing `[ebpf, linux]` errors because `ebpf` already implies `linux`), missing required groups, and conflicting exclusive tags (`[fast, slow]`), while precomputing implied tags so `tags.carries(os)` is a fast constant-time check. When items *do* have unique names that users can type alongside tags (such as lint rules), index them in a `Catalog<T>` (`Catalog::new_with` validates all items first and passes each item's `ItemId` to your builder so your value can store its own ID).
- **Boolean filter queries (`Expr`)**: Parse user queries like `"(linux | kernel) & !slow"` (supporting `!`/`not`, `&`/`&&`/`and`, `|`/`||`/`or` with zero parser dependencies), match them against any `TagSet`, and run `Expr::lint` to catch queries that can never match (`ebpf & !os`, `fast & slow`), always match (`os | !ebpf`, `!fast | !slow`), or repeat an implied tag (`linux & os`).
- **Include/exclude overrides (`TagSelection`, `Selection`)**: Resolve config-style include/exclude lists where exact item names beat tags, narrower tags override broader ones (excluding `os` while including `linux` keeps `linux` and `ebpf`), and `Exclude` wins ties across independent branches. Layer CLI or profile overrides on top with `override_with(...)`, inspect `decide(...)` + `Vocabulary::chain_through(...)` to tell users *why* an item was included or excluded, find no-op config entries with `ineffective()`, or convert a `TagSelection` into an equivalent `Expr` (`to_expr()`) to combine config defaults with a CLI filter.

## Examples

### 1. Vocabularies, Validated `TagSet`s, and Boolean `Expr` Queries

```rust
use tagonomy::{Cardinality, Expr, Tag, TagGroup, TagSet, Vocabulary};

const AREA: TagGroup = TagGroup { name: "Area", cardinality: Cardinality::AtLeastOne };
const SPEED: TagGroup = TagGroup { name: "Speed", cardinality: Cardinality::ExactlyOne };

const OS: Tag = Tag::new(&AREA, "os", "Operating-system behavior.");
const LINUX: Tag = Tag::new(&AREA, "linux", "Linux.").implies(&[OS]);
const KERNEL: Tag = Tag::new(&AREA, "kernel", "Kernel.").implies(&[OS]);
const EBPF: Tag = Tag::new(&AREA, "ebpf", "eBPF programs.")
    .implies(&[LINUX, KERNEL])
    .aliases(&["bpf"]);
const FAST: Tag = Tag::new(&SPEED, "fast", "Fast.");
const SLOW: Tag = Tag::new(&SPEED, "slow", "Slow.");

// Implied tags (`linux`, `kernel`, `os`) are pulled into the vocabulary automatically.
let vocab = Vocabulary::new([EBPF, FAST, SLOW]).unwrap();
let os = vocab.id("os").unwrap();

// Validating an item's tags expands implied parents for fast lookup:
let probe = TagSet::new(&vocab, [EBPF, SLOW]).unwrap();
let syscall = TagSet::new(&vocab, [LINUX, FAST]).unwrap();
assert!(probe.carries(os)); // `ebpf` implies `linux` implies `os`

// Redundant ancestors and conflicting exclusive tags are rejected loudly:
assert!(TagSet::new(&vocab, [EBPF, LINUX, SLOW]).is_err()); // `linux` already implied by `ebpf`
assert!(TagSet::new(&vocab, [LINUX, FAST, SLOW]).is_err()); // `Speed` allows only one

// Parse and evaluate boolean filter queries (aliases like `bpf` resolve automatically):
let query = Expr::parse(&vocab, "(linux | kernel) & !slow").unwrap();
assert!(query.lint(&vocab).is_empty());
assert!(!query.matches(&probe));
assert!(query.matches(&syscall));

// Static query linting catches impossible, always-true, and redundant queries:
assert!(!Expr::parse(&vocab, "bpf & !os").unwrap().lint(&vocab).is_empty());
assert!(!Expr::parse(&vocab, "fast & slow").unwrap().lint(&vocab).is_empty());
assert!(!Expr::parse(&vocab, "linux & os").unwrap().lint(&vocab).is_empty());
```

### 2. Include/Exclude Rules (`TagSelection`) Combined with a CLI `Expr`

```rust
use tagonomy::{Expr, TagSelection, Verdict};

// Given the `vocab`, `probe` ([ebpf, slow]), and `syscall` ([linux, fast]) from above:
let os = vocab.id("os").unwrap();
let linux = vocab.id("linux").unwrap();

// Exclude all `os` items, except re-include `linux` (and `ebpf`, which implies `linux`):
let config_selection = TagSelection::new(
    &vocab,
    Verdict::Include,
    [(os, Verdict::Exclude), (linux, Verdict::Include)],
)
.unwrap();

assert!(config_selection.matches(&probe));
assert!(config_selection.matches(&syscall));

// Convert the config selection into an equivalent `Expr` and combine with an ad-hoc CLI filter:
let cli_filter = Expr::parse(&vocab, "!slow").unwrap();
let combined = config_selection.to_expr() & cli_filter;
assert!(!combined.matches(&probe));
assert!(combined.matches(&syscall));
```

### 3. Named Items with `Catalog` and `Selection`

When items also have unique names users can include or exclude directly (like lint rules or test cases), wrap them in a `Catalog` and resolve directives with `Selection`:

```rust
use tagonomy::{Catalog, Item, Reason, Selection, Verdict};

// Given `vocab`, `EBPF`, `LINUX`, `KERNEL`, `FAST`, and `SLOW` from Example 1:
let catalog = Catalog::new(
    vocab,
    [
        Item::named("probe-test", [EBPF, SLOW]),
        Item::named("syscall-test", [LINUX, FAST]),
        Item::named("scheduler-test", [KERNEL, FAST]),
    ],
)
.unwrap();

// Include `os`, exclude `slow`, but force-include `probe-test` by exact item name:
let resolve = |label| catalog.resolve(label).unwrap();
let selection = Selection::new(
    &catalog,
    Verdict::Exclude,
    [
        (resolve("os"), Verdict::Include),
        (resolve("slow"), Verdict::Exclude),
        (resolve("probe-test"), Verdict::Include),
    ],
)
.unwrap();

// Exact item name beats tags; `decide` explains which selector won:
let probe = catalog.item("probe-test").unwrap();
assert_eq!(selection.decide(probe).verdict, Verdict::Include);
assert_eq!(selection.decide(probe).reason, Reason::Item);

// Layer higher-precedence CLI overrides without mutating the base selection:
let cli_overridden = selection
    .override_with([(resolve("probe-test"), Verdict::Exclude)])
    .unwrap();
assert!(!cli_overridden.matches(probe));
assert!(selection.matches(probe));

// For tag-based decisions, `Vocabulary::chain_through` gives the full root-to-source path
// through the deciding tag (even on a multi-parent diamond):
let tag_only = Selection::new(
    &catalog,
    Verdict::Exclude,
    [(resolve("os"), Verdict::Include), (resolve("kernel"), Verdict::Include)],
)
.unwrap();
if let Reason::Tag { tag, source } = tag_only.decide(probe).reason {
    let vocab = catalog.vocabulary();
    let path: Vec<_> = vocab
        .chain_through(source, tag)
        .into_iter()
        .map(|id| vocab.tag(id).label)
        .collect();
    assert_eq!(path, ["os", "kernel", "ebpf"]);
}
```
