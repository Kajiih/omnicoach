# Abstraction graph: 02 References

> **Status: ALPHA / EXPERIMENTAL — Active Investigation & Design** (subject to change at any time). Next: 03_design.md (CUJs, then prototypes).

Scope reminder: Stage A, one module, Python `class`, or Rust `impl` as the linter sees it ([01_understand.md](01_understand.md), D10).

Markers: **[V]** verified in code or a primary source; **[S]** checked through search summaries of official pages or bibliographic records, not read in full; **[U]** single secondary source.

## 1. Internal References

| ID | Reference | Key idea | Relevance |
| :--- | :--- | :--- | :--- |
| RI1 | Call-cluster analyzer in [ast.rs](../../../src/code_lint/ast.rs), [ADR 011](../decisions/011_colocated_abstraction_ordering.md) | Per scope: declarations, ownership `Roots(h)`, typed abstraction ordering (§3.4), recursion groups, findings [V] | **The data source for Stage A.** Computed for lint findings, then discarded. |
| RI2 | [ADR 006](../decisions/006_architectural_dag_and_conformance.md) and [architecture_conformance.rs](../../../tests/architecture_conformance.rs) | Declared component DAG plus a checker of actual `crate::` references; ADR 006 names its design a "Reflexion Architectural DAG" [V] | Stage B foundation. Already implements the reflexion model's declared-versus-actual idea (RL3). |
| RI3 | `summarize_rust_file` / `RustFileSummary` in [rust.rs](../../../src/code_lint/ast/rust.rs) | Single-pass, per-file summary of uses and paths, used by the conformance test [V] | Stage B: cheap extraction of references between files. |
| RI4 | `declaration_graph_export_investigation.md` (earlier artifact, D5) | JSON Graph Format v2 sketch with raw and derived edge families | Input only. RT4 and RT3 below give better targets for nesting and folding. |

## 2. External Tools

| ID | Tool | Key idea | Difference from what we have |
| :--- | :--- | :--- | :--- |
| RT1 | [cargo-modules](https://github.com/regexident/cargo-modules) [S] | Rust module and item graph from rust-analyzer HIR, visibility shown; `owns` (containment) and `uses` edges [U for the edge names]; focus and depth filters; DOT only | Closest Rust extractor. No typed reasons, no ownership by entrypoint, no levels, no folding, Rust only. |
| RT2 | [C4 model](https://c4model.com) / [Structurizr](https://docs.structurizr.com/dsl/language) / [LikeC4](https://likec4.dev) [S] | One model, many views; zoom levels (context → container → component → code); drill-down navigation | Written by hand; the code level is explicitly left to generated tooling. Our module view would be a generated level 4. |
| RT3 | [Cytoscape.js expand-collapse](https://github.com/iVis-at-Bilkent/cytoscape.js-expand-collapse), [DGML](https://learn.microsoft.com/visualstudio/modeling/directed-graph-markup-language-dgml-reference) [S] | Compound nodes that collapse; collapsed nodes keep aggregated "meta edges"; typed edge categories | Consumers that fold a hierarchy they are given. They don't compute one; our ownership data is that hierarchy. |
| RT4 | [ELK JSON](https://eclipse.dev/elk/documentation/tooldevelopers/graphdatastructure/jsonformat.html) [S] | Nested `children`, edges across levels, layered layout with constraints | Layout-ready target where our levels can be explicit layer constraints. |
| RT5 | [jQAssistant](https://jqassistant.github.io/jqassistant/) [S] | Code loaded as a typed property graph; queries add concepts and constraints; "living documentation" | Model for keeping typed reasons and findings as data attributes, not baked into a picture. Java and bytecode only. |
| RT6 | Levelized DSM tools (Structure101, Lattix) [S] | Matrix view showing layers and cycles ("tangles") | A non-node-link alternative for large scopes; maps onto our ordering plus recursion groups. |

Also surveyed, less relevant: pydeps, pyreverse, Doxygen call and collaboration graphs, import-linter and tach, SciTools Understand, SCIP. Sourcetrail is archived and CodeSee is discontinued [S].

## 3. Research

| ID | Reference | Key idea | Implication for Stage A |
| :--- | :--- | :--- | :--- |
| RL1 | Brooks 1983; Soloway & Ehrlich 1984; von Mayrhauser & Vans 1995 [S] | Comprehension is top-down via hypotheses and "beacons", switching to bottom-up when needed | Public abstractions as the first screen; names and signatures as beacons on nodes; expanding drops to helpers and source. |
| RL2 | Storey, Fracchia & Müller 1999; Storey, Wong & Müller 2000 (SHriMP) [S] | Nested graphs that expand in place, multiple abstraction levels, code proximity; tools that force one strategy hinder | Supports folding helpers into owners in one nested view; every node must link to its source. |
| RL3 | Murphy, Notkin & Sullivan 1995; Murphy & Notkin 1997 (reflexion models) [S] | Summary edges between groups drive understanding; declared versus actual shows divergences and absences | Folded views need summary edges with counts. Stage B: our declared components replace the hand-written mapping. |
| RL4 | Lakos 1996 (levelization); Sugiyama et al. 1981 (layered drawing) [S] | Levels are defined by dependencies, cycles collapse into one unit; layered layout pipeline behind Graphviz `dot` | Export our levels as explicit layer constraints instead of letting the layout infer them; draw recursion groups as one unit. |
| RL5 | Sillito, Murphy & De Volder 2006/2008 [S] | 44 developer questions in four groups; IDEs handle "find" and "expand a focus point" well, "understand a subgraph" poorly | Our graph should target the subgraph questions. These become Phase 3 reader journeys. |
| RL6 | Merino et al. 2018 (systematic review of visualization evaluation); Reiss 2005; Cherubini et al. 2007 [S] | Most visualizations are adopted poorly and evaluated weakly; diagrams die when keeping them current is costly | Regenerate cheaply with stable output; evaluate on real tasks, beyond time and correctness. |

Supporting: Ghoniem, Fekete & Castagliola 2004 report node-link diagrams lose to matrices for overview tasks beyond about 20 nodes, but stay better for following paths [S]. Lanza & Ducasse 2003 (polymetric views) encode metrics as node size and color [S].

## 4. Critical Points

| ID | Point | Source |
| :--- | :--- | :--- |
| CP1 | **The novelty is the data, not the rendering.** No surveyed tool derives an ownership hierarchy inside a module from visibility and reachability, distinguishes exclusive from shared helpers, types abstraction reasons such as encapsulation or signature contract, or treats Rust and Python with one model. | RT1–RT6 |
| CP2 | **Hierarchy must be explicit in the data** (containment, not just edges) for folding consumers to work. Formats without nesting (plain DOT) can only show a degraded, flat view. | RT3, RT4 |
| CP3 | **Folding needs summary edges.** When helpers fold into an owner, their edges must aggregate into owner-to-owner edges with counts and kinds, or the folded view lies. | RL3, RT3 |
| CP4 | **Shared helpers don't fold into a single owner.** A helper with two or more public roots has no single owner. The tools survey suggests a separate "shared" group, mirroring ADR 011's trailing helper section (Place 2). This refines D12. | Tools survey, ADR 011 |
| CP5 | **Levels should be exported explicitly**, as attributes or layer constraints, with recursion groups as single units. Layout tools otherwise infer layers by breaking cycles heuristically. | RL4, RT4 |
| CP6 | **Code proximity is mandatory**: every node carries its file and line. | RL2, RL6 |
| CP7 | **The default view must stay small**: about 20 nodes is the evidence-based limit for node-link overviews. Large modules such as `ast.rs` may exceed it even with only public items, which Phase 3 must measure. | Ghoniem 2004, RL2 |
| CP8 | **Adoption, not representation, is the main risk.** Success must be judged on real reader tasks (RL5), which fits D13. | RL6 |
| CP9 | **C4 leaves the code level to generated tooling.** A generated module view fits under C4-style component views in Stage B. | RT2 |

## 5. Suggestions and Combinations

1. **One neutral typed graph plus thin adapters for prototyping.** Explicit containment (scope → owner group → item), typed edges (raw references and derived ordering reasons), node attributes (kind, visibility, level, recursion group, position, findings, file and line). Adapters to ELK JSON and Cytoscape JSON for folding and layered layout; DOT as a degraded fallback. This combines RI4's edge families with RT3 and RT4's nesting.
2. **Collapsed by default:** the first screen shows public items, the shared group, and summary edges with counts; expanding an owner shows its exclusive helpers (RL1, RL2, RT3).
3. **Findings as an overlay** on the same graph rather than a separate view (RT5).
4. **Cheap structural metrics** computed automatically for every module: folded node count, back-edges against levels, a Lakos-style cumulative dependency figure. Useful to compare prototypes before involving people (RL4, RL6).

## 6. New Open Questions

| ID | Question |
| :--- | :--- |
| Q14 | ~~Where do shared helpers go?~~ Answered (D15). |
| Q15 | ~~First screen when public items alone are too many?~~ Deferred (D16). |
| Q16 | ~~Evaluation without real newcomers?~~ Answered (D17). |

## 7. Decisions

| ID | Decision | Source |
| :--- | :--- | :--- |
| D15 | A helper used by two or more components is not folded into any owner: being shared makes it an abstraction of its own, at a lower level, possibly grouped with other shared helpers at the same level. This replaces CP4's single "shared" bucket; prototypes explore how to group them. | User (Q14) |
| D16 | Readability limits for large modules (CP7) are not a design input yet; prototypes will show whether they matter. | User (Q15) |
| D17 | Evaluation combines structural metrics (§5.4), walkthroughs by the user on chosen modules, and an agent answering onboarding questions from the graph versus from the source. Using the graph to understand a project is itself a valid outcome. | User (Q16) |

## 8. Addendum: Viewers and Formats (for Q22)

Added after Phase 2 validation, at the user's request for "the most modern and standard" viewer. Checked against official pages through search summaries [S]; versions as of 2026-10.

### 8.1 Formats

| Format | Who reads it | Nesting | Typed attributes | Text and diffable |
| :--- | :--- | :--- | :--- | :--- |
| **GraphML** | yEd, Cytoscape, Gephi, NetworkX, igraph, Neo4j | In the spec (a `<graph>` inside a `<node>`); only yEd honors it fully, others flatten | Declared, typed keys | XML |
| **DOT** | Graphviz and very many tools | Cluster subgraphs, no collapse | Untyped strings | Excellent |
| GEXF | Gephi ecosystem, NetworkX | Partly | Typed | XML |
| Cytoscape JSON | Cytoscape.js, Cytoscape Desktop, NetworkX | `data.parent` compound nodes | Free JSON | JSON |
| ELK JSON | elkjs, ELK tooling | Recursive `children` | Layout options | JSON |
| D2 / Mermaid | Their own tools; Mermaid renders on GitHub | Containers / subgraphs | Classes | Excellent |
| JSON Graph Format v2 | Almost no viewer | No compound nodes | Metadata | JSON |
| DGML | Visual Studio on Windows only | Yes, with collapse | Categories | XML |

Only **GraphML and DOT** are real interchange standards across tools. No existing converter carries nesting, typed relations, and levels across formats without loss, so the practical approach is our own model with thin serializers per format (D18, D19).

### 8.2 Viewers

| Viewer | Status | Fit for our needs |
| :--- | :--- | :--- |
| **yEd** (desktop) | Free, closed source, Java; 3.25.1 (2025) | Reads GraphML with nesting; group nodes open and close; maps custom data keys to styles; hierarchic layout. Layers come from layout or coordinates, not from a data key. Whether collapsed groups aggregate edges is unverified. |
| **Cytoscape.js** with expand-collapse and ELK layout | MIT; 3.33+ (2025), very active | The only option where collapse with aggregated "meta edges", styling by edge type, and explicit ELK layer constraints were all verified. Needs a small static HTML page that loads and configures the libraries. |
| **Cytoscape Desktop** | LGPL; 3.10.4 (2025) | Reads GraphML and Cytoscape JSON; groups collapse into meta-nodes; strong styling and filters. Nested GraphML may flatten on import. |
| **D2** / **Mermaid** | Active (D2 0.7, Mermaid 11) | Text-first and diffable; Mermaid renders natively on GitHub. Static output: no interactive collapse, no data-driven layers. |
| Graphviz with an interactive preview | Graphviz 14 | Explicit ranks (`rank=same`); clusters without collapse. |
| Gephi / Gephi Lite | Active | Force layouts only and weak nesting: poor fit. |
| LikeC4 / Structurizr | Active | Drill-down views, but models are written in their DSL by hand. Relevant to Stage B, not Stage A. |
