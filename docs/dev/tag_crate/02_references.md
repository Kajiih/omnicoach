# `tagonomy` Crate: Literature & SOTA References

## TL;DR

- **Established classification standards settle the vocabulary rules**: ISO 25964 / W3C SKOS separate a tag's identity (`TagId` and canonical `label`) from aliases and scope notes; faceted classification (Ranganathan; Yee et al.) splits tags into independent groups with explicit per-group count rules; Formal Concept Analysis (FCA) motivates storing only each item's most specific tags while precomputing all implied parents as bitsets; and lightweight description logic ($\mathcal{EL}_\bot$) shows why implication and exclusive-group checks run in fast polynomial time without a general SAT solver.
- **CLI filter grammars converge on dual symbolic and keyword operators**: Supporting both symbols (`!`, `&`/`&&`, `|`/`||`) and case-insensitive words (`not`, `and`, `or`) with `(...)` grouping—while keeping `-` and `+` available inside bare `kebab-case` and slash-path tag names and rejecting empty strings loudly—avoids the syntax pitfalls of `cargo-nextest`, `pytest -m`, Cucumber, and Jujutsu revsets.

---

## 1. Foundations & How Formal Terms Map to `tagonomy`

| Design Question | Primary Source | Plain-English Principle Adopted in `tagonomy` |
| :--- | :--- | :--- |
| **Canonical names, aliases & scope notes** | ISO 25964-1:2011; W3C SKOS | Give each tag one canonical `label` and `TagId`, keep alternative names in `aliases` (for example, `bpf` → `ebpf`), and document boundary edges in `scope_note`. |
| **Independent tag groups ("facets")** | Ranganathan (1937); Yee et al. (2003) | Group tags by the question they answer (`OS`, `Speed`, `Topic`) with a `Cardinality` rule (`Any`, `AtMostOne`, `AtLeastOne`, `ExactlyOne`) instead of minting combo tags like `fast-linux`. |
| **Most-specific tags & implied parents ("antichain" / "upward closure")** | Ganter & Wille (1999, FCA) | An item lists only its most specific tags (an *antichain*: no tag already implied by another tag in the list, so `[ebpf]` is valid and `[ebpf, linux]` is rejected as `RedundantTag`). Expanding each tag to itself plus all broader tags it implies (its *upward closure*, stored as a bitset) answers `carries(os)` in constant time. |
| **Fast conflict & query checks ($\mathcal{EL}_\bot$)** | Baader, Brandt & Lutz (2005) | Checking tag implications and exclusive-group conflicts over precomputed parent bitsets runs in polynomial time, powering vocabulary validation, `TagSet` validation, and `Expr::lint` without a heavy solver. |
| **Narrower-wins overrides & `Exclude` tie-breaking** | Poole (1985); OASIS XACML v3.0 (`deny-overrides`) | Along each explicit tag's parent chain, the most specific matching directive wins (excluding `os` and including `linux` keeps `linux` and `ebpf`); when two independent branches disagree, `Exclude` wins (`deny-overrides`). |

**Shared Conflict Check Across Three Layers**:
Because `Vocabulary` precomputes, for every tag, both the broader tags it implies and the strictly narrower tags that imply it as `FixedBitSet`s, a single helper (`Vocabulary::exclusive_conflict`) enforces three checks with zero duplicated logic:

1. **Vocabulary validation** (`VocabularyError::Unsatisfiable`): catches a single tag like `portable` that implies both `linux` and `windows` in an `AtMostOne` `OS` group.
2. **Item validation** (`TagSetError::ExclusiveGroup`): catches an item tagged with both `fast` and `slow` (or `ebpf` and `windows`).
3. **Query linting** (`ContradictionKind::ExclusiveGroup` / `TautologyKind::ExclusiveGroup`): catches impossible queries like `ebpf & windows` and always-true queries like `!fast | !slow`.

---

## 2. SOTA Comparison: CLI Boolean Tag Filter Grammars

| System | Operators & Precedence (`tight → loose`) | Identifier Rules | Trade-offs for Bare Tag Labels |
| :--- | :--- | :--- | :--- |
| **Cucumber Tag Expressions** (`io.cucumber:tag-expressions`) | `(...)` → `not` → `and` → `or` | Bare tokens (`@smoke`, `fast`), `\`-escaped whitespace/parens | Minimal 3-level precedence; rejects symbolic `!`, `&`, `\|` common on the command line; treats empty `""` as match-all `true`. |
| **`pytest -m`** (`_pytest/mark/expression.py`) | `(...)` → `not` → `and` → `or` | Bare identifiers containing `-`, `_`, `.`, `/` | Rejects empty `""` loudly (catching unexpanded shell variables); rejects symbolic `&`, `\|`, `!`. |
| **`cargo-nextest` Filtersets** (`nextest-filtering`) | `(...)` → `!`/`not` → `&`/`and`/`-` → `\|`/`or`/`+` | Wrapped predicates (`tag(...)`, `test(...)`) | Supports both symbols and keywords, but binary `-` (difference) and `+` (union) force predicate wrappers because `-` collides with `kebab-case` labels (`sleep-in-tests`). |
| **Jujutsu Revsets** (`jj_lib::revset`) | `(...)` → `~` → `&` → `\|` | Bare symbols or quoted strings | Prefix `~` negation is unfamiliar outside VCS revision graphs. |

**Grammar Choice for `Expr::parse`**:
`Expr::parse` supports `(...)` → `!` / `not` → `&` / `&&` / `and` → `|` / `||` / `or` (matching keywords ASCII case-insensitively), rejects empty input with `ExprParseErrorKind::Empty`, and leaves `-` and `+` out of operator tokens so bare `kebab-case` (`test-timing`), `snake_case`, and slash-path (`dance/latin/rumba`) tag names work with no quotes or wrapper functions.

---

## 3. Why `Expr` Distinguishes Implied vs. Direct-Only Tag Matching

When `ebpf` implies `linux` and `linux` implies `unix`:

- `Expr::Tag(unix)` matches any item carrying `unix` directly **or** via a narrower tag like `ebpf`.
- `Expr::ExplicitTag(unix)` matches **only** items that list `unix` directly (so it matches `[unix]` but not `[ebpf]`).

Having both variants serves two concrete jobs:

1. **Exact conversion from `TagSelection` (`TagSelection::to_expr`)**: When an item carries an explicit tag like `ebpf`, whether that tag contributes an `Include` or `Exclude` vote depends on the most specific directives matching `ebpf` and its parents. Testing each deciding explicit tag with `Expr::ExplicitTag` lets `tag_selection.to_expr()` match the exact same items as `tag_selection` even on multi-parent diamonds where a child overrides one branch while leaving a sibling tag untouched.
2. **Accurate query linting (`Expr::lint`)**: `Expr::Tag(unix) & !Expr::ExplicitTag(unix)` *can* match items (for example, `[ebpf]`, which implies `unix` without listing it directly), so `Expr::lint` accepts it. By contrast, `Expr::Tag(ebpf) & Expr::ExplicitTag(unix)` can never match any valid `TagSet`, because `TagSet` forbids listing both `ebpf` and its implied parent `unix` (`ContradictionKind::Antichain`). Pushing `!` inward (known in logic as *Negation Normal Form*, turning `!(a & b)` into `!a | !b`) and reusing the `AND` checker for `OR` clauses via negation (`!(A | B) == !A & !B`, De Morgan's law) ensures negated queries receive the exact same warnings as positive ones without duplicating check code.

## References

- **ISO 25964-1:2011**, *Information and documentation — Thesauri and interoperability with other vocabularies — Part 1: Thesauri for information retrieval* — Standard model for preferred labels, aliases, multi-parent hierarchies, and scope notes.
- **Miles & Bechhofer (2009), *SKOS Simple Knowledge Organization System Reference*, W3C Recommendation** ([w3.org/TR/skos-reference](https://www.w3.org/TR/skos-reference/)) — Formalizes `skos:prefLabel`, `skos:altLabel`, `skos:broader`, and `skos:scopeNote`.
- **Ranganathan (1937), *Prolegomena to Library Classification*; Yee et al. (2003), *Faceted Metadata for Image Search and Browsing*, ACM CHI** ([doi:10.1145/642611.642681](https://doi.org/10.1145/642611.642681)) — Motivates orthogonal tag groups over single rigid trees.
- **Ganter & Wille (1999), *Formal Concept Analysis: Mathematical Foundations*, Springer** ([doi:10.1007/978-3-642-59830-2](https://doi.org/10.1007/978-3-642-59830-2)) — Proves the one-to-one correspondence between minimal explicit tag lists (antichains) and full implied tag sets.
- **Baader, Brandt & Lutz (2005), *Pushing the $\mathcal{EL}$ Envelope*, IJCAI** ([ijcai.org/Proceedings/05/Papers/0372.pdf](https://www.ijcai.org/Proceedings/05/Papers/0372.pdf)) — Proves polynomial-time implication and disjointness checking in $\mathcal{EL}_\bot$.
- **Poole (1985), *On the Comparison of Theories: Preferring the Most Specific Explanation*, IJCAI; OASIS XACML v3.0 (2013)** ([docs.oasis-open.org/xacml/3.0](https://docs.oasis-open.org/xacml/3.0/xacml-3.0-core-spec-os-en.html)) — Formalizes most-specific-wins overrides and `deny-overrides` tie-breaking.
- **pytest Mark Expression Parser (`_pytest/mark/expression.py`) & `cargo-nextest` Filtersets (`nexte.st/docs/filtersets`)** ([docs.pytest.org](https://docs.pytest.org/en/stable/example/markers.html), [nexte.st/docs/filtersets](https://nexte.st/docs/filtersets/)) — Production references for recursive-descent CLI boolean filter parsers.
- **`slotmap::SlotMap::insert_with_key`** ([docs.rs/slotmap](https://docs.rs/slotmap/latest/slotmap/struct.SlotMap.html#method.insert_with_key)) — Reference pattern for passing an assigned handle (`ItemId`) into value construction (`Catalog::new_with`).
