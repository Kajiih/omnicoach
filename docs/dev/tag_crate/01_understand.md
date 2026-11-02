# General-Purpose Tag Crate: 01 Understand (First Principles)

> [!NOTE]
> **Status: IN REVIEW (Phase 1–3)** — First-principles analysis of what tags are, why software needs them compared to other metadata mechanisms, and what belongs in a general-purpose Rust tag crate without overfitting to `omnicoach`.
> Next: [02_references.md](02_references.md) (literature, SOTA systems, and independent subagent designs) and [03_design.md](03_design.md) (deep-dive investigations into polyhierarchy DAGs, query/selection algebras, and `const` representations).

---

## 1. First Principles: What Fundamentally *Is* a Tag?

### 1.1 The Five Ways Software Attaches Information to Objects

Given a universe of domain objects $O$ (test cases, lint rules, CLI commands, benchmark targets, plugins, entities), software systems attach metadata using five distinct mechanisms. Each has a different mathematical signature and solves a different problem:

| Mechanism | Mathematical Model | Cardinality & Structure | What It Does Well | Where It Breaks Down Compared to Tags |
| :--- | :--- | :--- | :--- | :--- |
| **1. Types & Marker Traits** (`x: T`, `impl Fast for X`) | Intrinsic type judgment | Fixed per Rust type at compile time; erased at runtime | Zero-cost static dispatch and compile-time trait bounds | **Tied to types, not instances.** Two instances of `TestCase` or `Rule` have the same Rust type; you cannot filter a runtime slice `&[Item]` by marker traits or parse user filter strings (`"fast & !flaky"`). |
| **2. Struct Fields / Typed Attributes** (`x.is_fast: bool`, `x.os: Os`) | Cartesian product $O \to A_1 \times \dots \times A_n$ | Dense, fixed schema: every object must populate every field | Direct field access in imperative code (`if x.is_fast`) | **Not uniform or introspectable as a set.** You cannot ask *"what labels does `x` carry?"* without custom glue per field, and adding sparse cross-cutting descriptors bloats every struct with mostly-`false` booleans. |
| **3. Key-Value Properties / Labels** (`Map<String, Value>`) | Partial function $O \to (K \rightharpoonup V)$ | At most **one** value $v \in V$ per key $k \in K$ (e.g. `timeout = 500`, `owner = "alice"`) | Attaching arbitrary scalar payloads (numbers, strings, paths) | **Functional per key, not set-valued.** When an object belongs to *multiple* topics (`logging` and `error-handling`), `Map<K, V>` forces awkward comma-stuffed strings or loses set algebra; arbitrary string values also lack a closed vocabulary or implication hierarchy. |
| **4. Folders / Strict Category Trees** (`path = "a/b/c"`) | Total function $f: O \to C$ into a single-parent tree | Partition ($|f(x)| = 1$): every object lives in **exactly one** place | Physical file layout and mutually exclusive hierarchies | **Tyranny of the Dominant Decomposition.** Real objects are cross-cutting: a check that is both `linux`, `network`, and `security` cannot live in three folders at once. A tree forces one primary axis and scatters all others. |
| **5. Tags** (`tags(x) ⊆ T`) | Binary incidence relation $I \subseteq O \times T$, or $\text{tags}: O \to \mathcal{P}(T)$ | Many-to-many, sparse, non-exclusive ($|\text{tags}(x)| \ge 0$) | **Cross-cutting set membership, boolean/hierarchical querying, and faceted grouping** | Cannot carry arbitrary non-finite scalar payloads (use a Key-Value property or struct field for `timeout = 500ms`). |

### 1.2 Formal Definition of a Tag

Mathematically (from **Formal Concept Analysis**, Ganter & Wille 1999), a **tag** $t \in T$ is a **reified unary predicate** over objects:

$$t : O \to \{\top, \bot\}$$

Because $t$ is reified as a first-class symbol (rather than an opaque closure `Fn(&O) -> bool`), every tag has two dual views:

1. **Intension (Object $\to$ Tags)**: The set of tags attached to an object $o$, $\text{int}(o) = \{t \in T \mid (o, t) \in I\} \subseteq T$.
2. **Extension (Tag $\to$ Objects)**: The subset of objects possessing tag $t$, $\text{ext}(t) = \{o \in O \mid (o, t) \in I\} \subseteq O$.

**Why we need tags**: Tags are the unique representation where **predicates are data**. Because each tag denotes a subset of objects $\text{ext}(t) \subseteq O$, tags can be listed, documented, serialized, autocompleted, inverted into bitmap indices, and combined using set algebra ($\cap, \cup, \setminus, \subseteq$) without writing imperative filter code.

---

## 2. Why Flat Strings Are Not Enough: The Three Fundamental Relations Between Tags

In a naive tagging system (*folksonomy*), $T$ is just an unstructured set of strings (`HashSet<String>`). Both information-science literature and our independent first-principles subagent designs ([02_references.md](02_references.md)) show that flat tags fail in three predictable ways—and that fixing them requires **three relations on $T$**, *not* a heavy domain-specific schema:

### 2.1. Equivalence / Aliases ($t_1 \equiv t_2$) — Solving Synonym Drift

- **Failure mode**: One author or query uses `pg`, another uses `postgres`; one uses `jj`, another uses `jujutsu`.
- **Primitive**: A tag has one **canonical label** and zero or more **aliases (synonyms)** that resolve to the same canonical `Tag` before storage or querying.

### 2.2. Implication / Hierarchy ($t_1 \le t_2$, "$t_1$ implies $t_2$") — Solving Granularity Mismatch

- **Failure mode**: An author tags a test with the specific fact `postgres` (or `ebpf`). A user queries for `database` or `io` (or `linux`). With flat tags, `database` misses `postgres` unless every author manually maintains the full ancestor list `#[tag(postgres, database, io)]` on every item.
- **Primitive**: A partial order / DAG of **implications** $(T, \le)$ where $t_1 \le t_2$ means:
  $$\forall o \in O,\; t_1 \in \text{tags}(o) \implies t_2 \in \text{tags}(o) \quad (\text{i.e. } \text{ext}(t_1) \subseteq \text{ext}(t_2))$$
- **Upward Closure**: When an object is tagged with explicit tags $S_o \subseteq T$, its **effective tags** are the upward closure $\uparrow\! S_o = \{t \in T \mid \exists s \in S_o.\; s \le t\}$.
- **Tree vs. Polyhierarchy (DAG)**: In a simple taxonomy, each tag has at most one parent (`jj <= vcs`). In general, a tag can imply multiple parents (`ebpf <= linux` and `ebpf <= network`). Both are just edges in $(T, \le)$.
- **Antichain Normalization**: Because $t_1 \le t_2$ already implies $t_2$, explicitly tagging an object with both `jj` and `vcs` is redundant; the minimal generators of $\uparrow\! S_o$ form an **antichain** ($\forall a \neq b \in S_o, a \not\le b$).

### 2.3. Tag Groups / Axes / Facets ($\perp$ and Cardinality) — Solving Contradictory or Incomplete Tagging

- **Question**: *What is the link between "facets" and "tags"?*
- **First-principles answer**: A **facet (or axis / group)** is simply **a named partition of tags that answer the same question**, optionally imposing a **cardinality constraint** on how many tags from that group an object may carry:
  1. **Unconstrained (`0..`)**: Ordinary cross-cutting tags (an object can have any number of them).
  2. **AtMostOne / Mutually Exclusive (`0..=1`, $t_i \perp t_j$)**: Tags in the group are pairwise disjoint (e.g., `speed = {fast, medium, slow}`). Tagging an object with both `fast` and `slow` is a contradiction.
  3. **AtLeastOne (`1..`)**: Every object in a catalog must carry at least one tag from this group (e.g., every rule or test must have at least one `topic` or `domain` tag).
  4. **ExactlyOne (`1`, `AtLeastOne` + `AtMostOne`)**: Every object must carry *one and only one* tag from this group (e.g., `precision = {exact, heuristic}`).
- Notice what this means: **a facet is not a separate kind of metadata from a tag**. The values of a facet *are* tags (`exact`, `heuristic`, `fast`, `slow`), and the facet itself is a **Tag Group (Axis)** with a name (`"precision"`, `"speed"`) and a cardinality rule (`0..`, `0..=1`, `1..`, `1`).

---

## 3. What Do We *Do* With Tags? (The Four Core Operations)

Once a tag vocabulary $\mathcal{V} = (T, \le, \text{Groups}, \text{Aliases})$ is defined and objects carry `TagSet`s, any consumer application performs up to four operations:

### Operation 1: Vocabulary & Object Validation (Hygiene)

- **Vocabulary invariants**:
  - No cycles in the implication graph $(T, \le)$.
  - No tag inherits from two mutually exclusive tags ("diamond of death": $t \le a, t \le b$ where $a \perp b$, making $t$ unsatisfiable).
  - Global uniqueness of canonical labels and aliases; no tag label collides with a group/axis name.
- **Object / Catalog invariants**:
  - No object carries mutually exclusive tags ($\forall a, b \in \uparrow\! S_o, \neg(a \perp b)$).
  - Every object satisfies `AtLeastOne` / `ExactlyOne` group constraints.
  - No **redundant ancestor tags** in explicit tag lists ($a < b$ both listed explicitly).
  - Optional catalog check: no **dead/orphan tags** (tags defined in $\mathcal{V}$ that match 0 objects in the catalog).

### Operation 2: Matching & Querying (Two Complementary Algebras)

Both independent subagents ([02_references.md](02_references.md)) identified that real applications filter tagged objects in two ways:

1. **Boolean Predicate Expressions (`Expr`)**:
   - Standard boolean algebra over upward-closed tag sets: `Has(t)` ($t \in \uparrow\! S_o$), `HasExplicit(t)` ($t \in S_o$), `InGroup(g)` ($\uparrow\! S_o \cap g \neq \emptyset$), `And`, `Or`, `Not`.
   - Used in CLI filter flags (`(postgres | sqlite) & !slow`).
   - Supports **static query lints** against the vocabulary: detecting contradictory queries (`linux & !unix` when `linux <= unix`, or `fast & slow` when `fast ⊥ slow`) and redundant clauses (`linux & unix` $\equiv$ `linux`) before inspecting any objects!
2. **Specificity-Ordered Include/Exclude Directives (`Selector` / `Plan`)**:
   - Used in declarative config files and hierarchical overrides (`select = ["vcs"]`, `ignore = ["jj"]`, or vice versa: `ignore = ["vcs"]`, `select = ["jj"]`).
   - When an object matches both an `Include(t1)` and an `Exclude(t2)` directive, **poset specificity ($t_1 < t_2$)** decides along an implication chain (the more specific sub-tag beats the broader ancestor tag), and an explicit tie-breaker (`DenyOverrides` / `ExcludeWins`, `AllowOverrides`, or `LastWins`) decides across independent branches/groups.
   - An object's own identifier (if named) acts as the most specific leaf below all of its tags.

### Operation 3: Provenance & Witness Tracing (Why Did It Match?)

- In any hierarchical tag system, `matches(obj) -> bool` is not enough when debugging why `obj` was selected or rejected.
- Because `Has(t)` holds via upward closure $\uparrow\! S_o$, every positive match has a **witness path** in the implication graph:
  $$s_{\text{explicit}} \le t_1 \le \dots \le t_{\text{queried}}$$
  (e.g., `[ebpf, linux, unix]`).
- Returning this structured witness path (without hardcoding TOML or CLI formatting) lets any caller format explanations in its own UI.

### Operation 4: Grouping, Faceted Counting & Introspection

- Given a collection of tagged objects (or a filtered subset), compute:
  - **Tag / Group counts**: how many objects match each tag (`fast: 42, slow: 8`).
  - **Group partitioning**: bucket objects by the tags of a `TagGroup` (e.g., grouping rules or tests by `category` or `language`).

---

## 4. Goals and Non-Goals for the Standalone Tag Crate

### Goals

- **G1 (General-Purpose Core)**: Model tags from first principles—canonical tags, aliases, implication hierarchy $(T, \le)$, and optional tag groups/axes with cardinality rules (`0..`, `0..=1`, `1..`, `1`)—with zero mention of linters, rules, ASTs, or `.omnicoach.toml`.
- **G2 (Compile-Time Ergonomics + Zero-Boilerplate Safety)**: Allow static Rust callers to declare tag vocabularies and object classifications using `const` definitions and Rust types so missing required groups, type mismatches, and implication cycles are caught at compile time where possible, and provide a one-call test-time validator for catalog-wide invariants (uniqueness, antichain non-redundancy, dead tags).
- **G3 (Fast Set Operations & Querying)**: Support both direct boolean predicate evaluation (`Expr`) and specificity-ordered `Include`/`Exclude` resolution (`Selector`), backed by compact bitsets/tables and returning structured implication-path witnesses.
- **G4 (Drop-In Foundation for `omnicoach`)**: Express `omnicoach`'s existing rule taxonomy and `select`/`ignore` resolution as an ordinary consumer of the crate, deleting bespoke taxonomy/selector engine code from `omnicoach`.

### Non-Goals

- **NG1 (Config File Formats & Path Globs)**: The tag crate must not depend on `toml`, `serde_json`, or `globset`, and must not know about file paths or `.omnicoach.toml`.
- **NG2 (Arbitrary Key-Value Scalar Storage)**: Key-value properties (`timeout = 500`, `author = "alice"`) are attributes, not tags, and are out of scope.
