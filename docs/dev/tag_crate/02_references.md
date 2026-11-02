# General-Purpose Tag Crate: 02 References, Literature & Unbiased Designs

> [!NOTE]
> Synthesizes academic literature on Knowledge Organization Systems (KOS), software engineering tagging architectures, and two independent first-principles crate designs produced by isolated subagents with no access to the `omnicoach` repository.

---

## 1. Academic & Information-Science Foundations

### 1.1 The Spectrum of Knowledge Organization Systems (KOS)

| Paradigm | Primary References | Core Mathematical Structure | Strengths | Failure Modes |
| :--- | :--- | :--- | :--- | :--- |
| **Folksonomy (Uncontrolled Flat Tags)** | Vander Wal (2004, 2007); Golder & Huberman (2006, *J. Info. Sci.* 32(2)); Marlow et al. (2006, *ACM HYPERTEXT*) | Tripartite relation $Y \subseteq U \times T \times R$, or binary incidence $I \subseteq O \times \Sigma^*$ in single-authority (*narrow*) systems. | Zero schema friction; anyone can attach any string token. | **Synonymy** (Furnas et al. 1987 *CACM*: two people pick the same unrestricted term $<20\%$ of the time), **polysemy**, **basic-level variation** (`dog` vs `beagle` vs `mammal`), and **orphan/typo tags**. |
| **Controlled Vocabulary & Thesaurus (SKOS / ISO 25964)** | ISO 25964-1:2011 & 25964-2:2013; W3C SKOS (Miles & Bechhofer 2009) | Concepts $C$ with preferred label (`prefLabel`), synonyms (`altLabel` / `UF`), polyhierarchical DAG (`BTG`/`NTG` broader/narrower generic), and normative **Scope Notes (`SN`)**. | Eliminates synonym fragmentation via `UF`; supports polyhierarchy (a concept can have $>1$ broader concept); `SN` prevents semantic drift. | Does not by itself enforce orthogonal axes or per-axis cardinality on objects. |
| **Faceted Classification** | Ranganathan (1933 *Colon Classification*, 1937 *Prolegomena*); Broughton (2006); Yee, Swearingen, Li & Hearst (2003 *ACM CHI*); Hearst (2009); Tunkelang (2009) | Decomposes $T$ into $k$ orthogonal dimensions (facets) $F_1, \dots, F_k$. An object is assigned values across facets: $O \to \mathcal{P}(F_1) \times \dots \times \mathcal{P}(F_k)$. | Avoids $O(\prod |F_i|)$ pre-coordinated category explosion; enforces completeness and mutual exclusion per axis; supports **OR within facet, AND across facets** navigation. | Overly rigid if every tag must belong to a single-valued enum field. |
| **Formal Concept Analysis (FCA)** | Wille (1982); Ganter & Wille (1999 *Formal Concept Analysis: Mathematical Foundations*, Springer) | Formal context $\mathbb{K} = (G, M, I)$ with Galois connection $A' = \{m \mid \forall g \in A, gIm\}$ and $B' = \{g \mid \forall m \in B, gIm\}$, concept lattice $\underline{\mathfrak{B}}(G, M, I)$, and **attribute implications** $X \to Y$ ($X' \subseteq Y'$). | Unifies objects (extent) and tags (intent) in one complete lattice; reduces subsumption and conjunctive queries to bitmatrix operations (`BitSet`). | Full concept lattice construction can be exponential if unconstrained; Horn implication closure over singletons ($t_1 \to t_2$) is linear/quadratic and bitset-friendly. |

### 1.2 Key Lessons from Literature for a Software Tag Crate

1. **Concept Identity $\neq$ Lexical Label (ISO 25964 / SKOS)**:
   A tag is a semantic concept with a canonical label, optional non-preferred aliases (`UF`), and a scope definition (`SN`), not a raw string.
2. **Generic Hierarchy (`BTG`) Is Transitive; Instantial (`BTI`) Is Not (ISO 25964)**:
   When tag $t_1$ specializes tag $t_2$ ($t_1 \le t_2$), every object with $t_1$ automatically has $t_2$. When an *object* $o$ has tag $t_1$, $o$ is an instance at the bottom of all its tag branches (`o <= t_1 <= t_2`).
3. **Birkhoff's Representation Theorem / Antichain Canonicalization**:
   In any finite poset $(T, \le)$, there is a bijection between upward-closed sets (filters $\uparrow\! S$) and **antichains** (sets of pairwise incomparable elements). Therefore, an object's explicit tags should always be an antichain (never listing both $t_1$ and its ancestor $t_2$).
4. **Post-Coordination of Orthogonal Groups (Ranganathan / Yee et al. 2003)**:
   Rather than creating compound tags like `fast-linux-unit-test`, orthogonal groups (`speed`, `os`, `kind`) are combined at attach and query time.

---

## 2. Software Engineering Reference Systems

| System | How It Models Tags | How It Queries / Selects | Key Strength or Pitfall |
| :--- | :--- | :--- | :--- |
| **pytest (`@pytest.mark`)** | Open markers by default; `--strict-markers` enforces a registered vocabulary with descriptions in `pyproject.toml`. | `-m "slow and not integration"` parsed into a boolean AST evaluated over `item.iter_markers()`. | **Lesson**: Unregistered tags cause silent CI bugs (`@pytest.mark.slwo`); strict vocabulary registration is essential. |
| **Cucumber / Behave** | `@tag` strings inherited down the AST (`Feature` $\to$ `Rule` $\to$ `Scenario` $\to$ `Examples`). | Replaced legacy multi-flag CNF (`--tags @a,@b --tags @c`) with infix boolean **Tag Expressions** (`(@a or @b) and not @c`). | **Lesson**: Implicit list conjunction/disjunction confuses users when mixed; explicit boolean expressions or well-defined specificity rules are much clearer. |
| **Bazel (`tags` & `--test_tag_filters`)** | Flat `tags = ["manual", "local", ...]` string list on targets. | `test_suite(tags = ["a", "b", "-c"])` means `a AND b AND NOT c`, whereas CLI `--test_tag_filters=a,b,-c` means `(a OR b) AND NOT c`. | **Pitfall**: Inconsistent boolean semantics for positive tag lists between config and CLI is a notorious footgun. |
| **Kubernetes Labels vs. Annotations** | Strictly separates **Labels** (indexed, bounded RFC 1123 key-values used for selection) from **Annotations** (unindexed metadata read after selection). | Set-based label selectors: `env in (prod, staging), tier != frontend, !experimental`. | **Lesson**: Separating *selection predicates* (tags/labels) from *display/payload metadata* (docs, URLs, templates) keeps the selector engine pure. |
| **Bevy ECS (Marker Components)** | Zero-Sized Type (ZST) marker components (`struct IsDisabled;`) stored as 0-byte archetype bits. | Type-level `With<A>`, `Without<B>`, `Or<(With<C>, With<D>)>` evaluated per archetype bitset in $O(1)$. | **Lesson**: Bitset representation per tag/object turns set operations into single CPU instructions. |

---

## 3. Comparison of the Two Independent Unbiased Subagent Designs

Two subagents with **zero access to the local repository** were asked to design a general-purpose Rust tag crate from first principles:

- **Subagent A (Type, Bitset & Static Analysis Lens)** designed `tag` around a compile-time `Vocabulary` trait, fixed-width `[u64; WORDS]` `const` bitsets, `EffectiveTags` (explicit + implied bitsets), a boolean `Selector` AST with static contradiction/redundancy analysis, and an inverted-index `Catalog`.
- **Subagent B (Poset Algebra, Disjointness & Specificity Policy Lens)** designed `tag` around a poset with disjointness $\mathcal{V} = (T, \le, \perp, \equiv)$ (Description Logic $\mathcal{EL}_\bot$), separating **Layer A (Boolean Predicate `Expr`)** from **Layer B (Specificity-Ordered `Include`/`Exclude` `Selector`)** with Pareto-maximal poset dominance and structured `Witness` chains.

| Design Dimension | Subagent A (Type & Bitset Lens) | Subagent B (Poset & Specificity Lens) | Current `omnicoach` (`rule_declaration` + `rule_selection`) | Synthesis for Our Crate |
| :--- | :--- | :--- | :--- | :--- |
| **Core Math Model** | Preordered vocabulary $(T, \vdash)$ + `AxisMeta` (`exclusive`, `required`). | Poset with disjointness & aliases $\mathcal{V} = (T, \le, \perp, \equiv)$ ($\mathcal{EL}_\bot$). | Special-cased `Topic` tree + hardcoded 3 single-valued enums + hardcoded `Derived` enum. | **Unified $\mathcal{V} = (T, \le, \text{Groups}, \text{Aliases})$**: Every tag is a node in $(T, \le)$; any tag can have aliases and parents; `TagGroup` (axis/facet) adds cardinality rules (`0..`, `0..=1`, `1..`, `1`). |
| **Hierarchy Shape** | Polyhierarchy DAG (`implies: &[V]`, e.g. `Ebpf` implies both `Linux` and `Network`). | Polyhierarchy DAG (`parents: Vec<TagId>`). | Single-parent tree (`parent: Option<&'static Topic>`), restricted to `Topic` only. | Support `parent: Option<&'static Tag>` (or `implies: &'static [Tag]`) uniformly across any tag, with `const` cycle rejection. |
| **Explicit vs. Implied Provenance** | `EffectiveTags` stores two `[u64; W]` bitsets: `explicit` and `effective` ($\uparrow\! E$). | `TagSet` stores `explicit: BTreeSet<TagId>` and `effective: BTreeMap<TagId, TagId>` (mapping each implied tag to its most-specific explicit source). | Recomputes `branches() -> Vec<Vec<Tag>>` on the fly per query. | Store both `explicit` and `effective` (with witness path recovery $s \le \dots \le t$) so matching is $O(1)$ and witness formatting is instantaneous. |
| **Query / Selection Model** | **Boolean AST (`Selector`)**: `All`, `None`, `Has(t)`, `HasExplicit(t)`, `HasAnyInAxis(a)`, `And`, `Or`, `Not`, plus **static query analysis** (`analyze()` catches `ebpf & !unix` and `fast & slow`). | **Two-Layer Split**:<br>1. **Layer A (`Expr`)**: Boolean `Has`, `Exact`, `And`, `Or`, `Not`.<br>2. **Layer B (`Selector`)**: `Include` / `Exclude` directives resolved by **Poset Specificity** ($t_1 < t_2$ wins) + configurable `TieBreaker` (`DenyOverrides`, `AllowOverrides`, `LastWins`). | Only Layer B (`Plan::decide` over flat `select` and `ignore` lists, hardcoded to `DenyOverrides` across branches). | **Support both Layer A (`Expr`) and Layer B (`Plan` / `Selector`)!** Notice that `omnicoach`'s Model B is *literally* Subagent B's Poset Specificity + `DenyOverrides` tie-breaker! |
| **Explanation / Witness** | `MatchExplanation<V>` proof tree showing matched clauses and implication chain `Ebpf -> Linux -> Unix`. | `Witness` enum (`Exact(t)` or `Implied { explicit_source, target, chain }`) + `SelectionExplanation` (`winning_directive`, `overridden_directives`, `OverrideReason`). | `RuleStatus` struct with TOML-specific `Display` (`enabled by select = ["..."] (via a > b)`). | Expose structured `Witness` / `SelectionDecision` (containing the winning directive, polarity, and implication chain `&[Tag]`), with a clean default `Display` and full caller access. |
| **Static Query Lints** | Catches **unsatisfiable** (`A & !B` when $A \le B$; `A & B` when $A \perp B$) and **redundant** (`A & B` when $A \le B$) queries without scanning items. | Catches **unsatisfiable tags** in the vocabulary ($t \le a, t \le b$ with $a \perp b$) and redundant explicit tags on items. | Only checks `select` $\cap$ `ignore` exact/synonym conflict; `ROADMAP.md` §5 lists "Shadowed-selector config warning" as an open item! | Static query/plan analysis solves both contradictory selectors and `omnicoach`'s open "shadowed selector" roadmap item! |
