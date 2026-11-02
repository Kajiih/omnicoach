# General-Purpose Tag Crate: 05 Declaration Prototypes

> [!NOTE]
> **Status: DONE.** Record of a prototype cycle on how consumers declare tags, tag groups and hierarchies with `tagonomy`, run after the crate was extracted ([ADR 013](../decisions/013_tagonomy_crate.md)). It explains why `omnicoach` declares topics with `Topic::root` / `Topic::sub` and a `TopicDoc` today, and why a derive macro is the planned next step rather than the current one.

## TL;DR

- **Problem.** `tagonomy` takes tags as `const` values whose hierarchy is `implies: &'static [Tag]`. A consumer must store an already-built `Tag`, which a plain struct literal cannot compute, so `omnicoach`'s `Topic` needed a three-type typestate builder, stored its parent twice, and repeated each description in a doc comment.
- **Four declaration styles were prototyped** against one fixture: parents named by label, one enum per group, a declarative macro tree, and a derive macro. The derive macro was the most pleasant to write and had the best diagnostics; all four were easy for a fresh contributor to extend.
- **The comparison grew beyond the problem.** It also scored moving every whole-vocabulary and rule-level check to compile time. That goal drove most of the cost (const validators, a const-readable rule registry, a catalog-assertion macro), produced worse errors than the existing runtime checks, and could not share code with the runtime constructors because `const fn` cannot allocate. It was dropped.
- **Decision.** Fix the original problem with the smallest change: `Topic::root` / `Topic::sub` take a `TopicDoc { description, scope_note }` struct literal ([taxonomy.rs](../../../src/rule_declaration/taxonomy.rs)), `Topic` only wraps its tag, and the duplicated doc comments are gone. The engine is unchanged.
- **Next.** A minimal `#[derive(tagonomy::Group)]` on plain enums is the planned declaration API, built when a second consumer or frequent new groups justify it ([ROADMAP.md](../../../ROADMAP.md) §5).

## 1. The four styles

All four implemented the same fixture: a flat exactly-one group (`Precision`), a hierarchical at-least-one group with aliases (`testing > test-timing`, `vcs > jj`), a derived group (`Languages`), a multi-parent tag (`ebpf` implies `linux` and `kernel`), three rules, and a selection `Plan` with its explanation chain.

| Style | How a child topic is declared | Change to `tagonomy` |
| :--- | :--- | :--- |
| Parents by label | A plain `Topic { label, parent: Option<&'static Topic>, aliases, description, scope_note }` struct literal; `Tag.implies` holds parent labels | Breaking: parents resolved by label, every tag passed explicitly, new unknown-parent and cycle errors |
| One enum per group | A documented variant; parent and aliases in exhaustive `const fn` matches | Structural: a trait for tag sources next to `Tag::implies` |
| Declarative macro tree | A `vocabulary!` block where nesting means "child of" and the doc comment holds description and scope note | Additive: the macro and its helpers |
| Derive macro | A documented variant with `#[tag(parent = Vcs, aliases = ["jujutsu"])]` on an enum deriving `Group` | Additive: a proc-macro crate |

**Context and interpretation.** Every style kept the classification's compile-time checks (missing or duplicated facet, wrong value type, unknown topic). They differed on how many facts each tag repeats and where errors point:

- *Parents by label* still writes the name twice (const name and label) and lost topic rustdoc, because the description lives only in a string field.
- *One enum per group* spreads a tag's facts over the variant and two or three `match` arms.
- *Declarative macro tree* writes every fact once, but each macro-level error underlines the whole invocation, and rustfmt never formats the body.
- *Derive macro* writes every fact once, in one place, and reports all of an enum's mistakes in one build, each at the offending variant. Its costs are an owned proc-macro crate and errors without error codes.

## 2. Fresh-contributor trial

A fresh agent, given only a prototype's crate docs and existing code, added a child topic with an alias, a new exactly-one group and a rule.

| Style | Outcome | Failed builds | Places edited to add the group |
| :--- | :--- | :--- | :--- |
| One enum per group | done | 0 | about 10, in 4 files |
| Declarative macro tree | done | 0 | about 7, in 2 files |
| Derive macro | done | 0 | 5, in 1 file |

**Interpretation.** Declaring tags was easy in every style. The friction was wiring a new group into the rule catalog: the hand-written places that map classification fields to groups, most of which existed only to feed the compile-time checks. Two lessons carried over regardless of style: negative tests must assert the specific error (a `compile_fail` doctest that spells out a classification silently fails for a new reason when a field is added), and conventions such as the doc-paragraph split must be written down rather than inferred.

## 3. Why compile-time whole-vocabulary checks were dropped

| Aspect | Runtime constructors (kept) | Compile-time `const` checks (dropped) |
| :--- | :--- | :--- |
| When they run | `cargo test` (`the_registered_rules_form_a_valid_catalog`) and first use | Every build |
| Errors | All at once, each naming the tag or rule | First failure only, pointing at a `const _` item or a whole macro call |
| Implementation | `Vocabulary::new`, `Catalog::new` | A second, allocation-free copy of the same invariants, which drifts from the first |
| Prerequisites | None | A rule registry readable by `const` code, and a catalog-assertion macro |
| Unused topics | `dead_code` on unused topic consts, plus `every_tag_has_a_rule` | A generated list of all tags hides `dead_code`, so a compile-time "every tag used" check must replace it |

Checks local to one declaration stay with the compiler: a missing `TopicDoc` field (`E0063`), a parent from another facet (`E0308`), a parent cycle (`E0391`), an unknown topic (`E0599`).

## 4. Suggestions and next steps

1. **Keep the current declarations** until a trigger in [ROADMAP.md](../../../ROADMAP.md) §5 fires (a second consumer, frequent new groups, or a wish for one declaration style across topics and facets).
2. **When it fires, build the minimal derive:** generate tags and accessors from a plain enum; report only missing docs, a missing scope note and malformed attributes itself; let rustc report unknown parents and cycles by emitting ordinary code; leave whole-vocabulary checks to the runtime constructors; test its few custom errors with `trybuild`.
3. **Do not reintroduce compile-time whole-vocabulary checks** without a consumer whose vocabulary is never exercised by tests, or a real bug that escaped `cargo test`.
