# Phase 1: Understand — Colocated Component Units and Abstraction-Layer Ordering

This document records **Phase 1 (Understand)** for the roadmap investigation **"Declaration Ordering of Functions, Methods, and Objects"** ([ROADMAP.md](../../../ROADMAP.md)) and [ADR 011](../../../decisions/011_colocated_abstraction_ordering.md). It derives the design from first principles (graph-theoretic root ownership, abstraction stratification, and cognitive models of program comprehension), SOTA static analysis rules across ecosystems, and empirical measurements on five real-world corpora (**CPython 3.14.8**, **The legacy linter**, **Omni `src/`**, **ripgrep 15.2.0**, and **cargo 0.100.0**).

> Status: **VALIDATED — First-Principles Colocated Abstraction Model & Empirical Survey Complete** (2026-10-08).

---

## 1. First-Principles Theory: Colocation & Monotonic Abstraction Layers

### 1.1 Cognitive Science of Program Comprehension (Top-Down Plan Recognition & Vertical Distance)
In empirical software engineering, program comprehension is governed by three foundational principles:
1. **Top-Down Comprehension & Discourse Rules (Brooks 1983; Soloway & Ehrlich 1984; Von Mayrhauser & Vans 1995)**:
   - Developers reading a module or type start with a **global hypothesis** of its contract and high-level responsibilities, scanning for structural *beacons* (state fields, constructors, public entrypoints) before inspecting low-level implementation helpers.
   - Soloway & Ehrlich (1984) demonstrated that when code layout violates standard discourse rules (such as placing low-level incidental helpers before primary entrypoints or burying initialization logic), expert comprehension degrades to novice levels.
2. **Vertical Distance, Dependent Functions & The Stepdown Rule (Martin 2008, *Clean Code* Ch. 5; Busjahn et al. 2015 Eye-Tracking)**:
   - Eye-tracking studies (Busjahn et al. 2015) show that developers scan entrypoints and step downward along call edges (`caller -> callee`) rather than reading files bottom-up from leaf utilities to entrypoints.
   - *Clean Code* formalizes this as **Vertical Distance** and **The Stepdown Rule**: *"If one function calls another, they should be vertically close, and the caller should be above the callee."*
3. **Locality of Behavior (Colocation) vs. Scope-Wide Visibility Buckets**:
   - Traditional visibility-sorting linters (`StyleCop SA1202`, `@typescript-eslint/member-ordering`, `WPS338`, `CCE001`) enforce a blind scope-wide split (`[all public -> all private]`).
   - When a module or class contains multiple public entrypoints (`pub_a`, `pub_b`) that each delegate to dedicated private helpers (`_a1`, `_b1`), scope-wide buckets force `[pub_a, pub_b, _a1, _b1]`, separating `pub_a` from `_a1` by unrelated public methods (`pub_b`).

---

### 1.2 Structural Isomorphism with ADR 006 (`src/architecture.rs`)

In [ADR 006](../../../decisions/006_architectural_dag_and_conformance.md), Omni established three architectural invariants across modules:
1. **Self-Contained Component Units**: Each feature module colocates its entrypoint and its exclusive implementation details.
2. **Sibling Isolation & Shared Lower Layer**: Sibling rule modules never import from each other; whenever logic is needed by two or more siblings, it drops down to a lower shared abstraction layer (`code_lint::policy`, `code_lint::semantic`, `code_lint::ast`).
3. **Monotonic Abstraction Gradient**: Higher-level orchestrators depend downward on lower-level foundations.

Inside a single module, class, or inherent `impl` block, the **exact same three invariants** apply automatically without annotations, because **visibility modifiers and the intra-scope call graph already define the dependency DAG**:

1. **Private-Subgraph Root Ownership (`Roots(h)`)**:
   - Let `P` be the public entrypoints of a scope (exported functions/methods, constructors, or private functions directly referenced by exported constants `pub const RULE`, trait `impl`s, or `__main__` guards; if a scope has no public entrypoints, functions with in-degree 0 in the scope's DAG act as roots).
   - For each private helper `h`, define `Roots(h)` as the set of public entrypoints in `P` that can reach `h` **walking only through private helpers** (stopping at public boundaries so `pub_b -> pub_a -> _a1` keeps `_a1` exclusive to `pub_a`).
2. **Why `Roots(h)` Beats Naive `LCOM4` Connected Components**:
   - In naive `LCOM4` (Hitz & Montazeri 1995), if `pub_a` calls exclusive helper `_a1`, `pub_b` calls exclusive helper `_b1`, and both call a low-level utility `_shared`, the undirected graph merges `pub_a` and `pub_b` into a single connected component—failing to detect `[pub_a, pub_b, _a1, _b1]`.
   - Under `Roots(h)`:
     - `_a1` has `Roots(_a1) = {pub_a}` (**1 root -> Exclusive Helper of `pub_a`**).
     - `_b1` has `Roots(_b1) = {pub_b}` (**1 root -> Exclusive Helper of `pub_b`**).
     - `_shared` has `Roots(_shared) = {pub_a, pub_b}` (**2+ roots -> Shared Helper Layer**).
   - Thus `_shared` never collapses `Unit(pub_a)` and `Unit(pub_b)` into one component!
3. **Why Shared Helpers Belong at the End and Callers Precede Callees (`callee-before-caller`)**:
   - Every private helper is at a **lower level of abstraction** than the public entrypoint that calls it, and a **shared helper** used by multiple public units is at an **even lower level of abstraction** (an intra-scope foundation layer).
   - Placing exclusive helpers immediately below their owning public entrypoint (`[pub_a, _a_step_1, _a_step_2]`), placing shared helpers at the end below all public units that call them (`[Unit(pub_a), Unit(pub_b), _shared_1, _shared_2]`), and ordering callers before callees within each group (`callee-before-caller`) creates a **single monotonic top-to-bottom abstraction gradient**:
     - `State Schema` -> `Constructors` -> `Unit A (pub_a -> _a1 -> _a2)` -> `Unit B (pub_b -> _b1)` -> `Shared Layer (_shared_1 -> _shared_2)` -> `Execution / Test Footer`.
   - By contrast, bottom-up `call-before-definition` (`[_a2, _a1]`) flips the abstraction gradient inside the private tier (`[pub_a, _a2, _a1]`), wedging the leaf helper `_a2` between `pub_a` and its direct callee `_a1`.

---

### 1.3 Why 2-Tier Visibility (`Exported` vs. `Private`) Beats 3-Tier in Rust & Python (`D4`)

1. **Rust Inherent `impl` Blocks & Modules**:
   - In Rust systems programming, a type is frequently **constructed** only by its owning subsystem (`pub(crate) fn new`, `pub(in crate::code_lint::ast) const fn from_span`), while its **accessors** are `pub fn`.
   - Requiring `pub` before `pub(crate)` / `pub(in ...)` would force restricted constructors below `pub fn` accessors, contradicting `constructor-after-method`.
   - Partitioning functions/methods into **2 tiers (`Exported: pub | pub(...)` vs. `Private: bare fn`)** is 100% compatible with constructor-first ordering and produces 0 false positives on restricted constructors across Omni, ripgrep, and cargo.
2. **Python Classes & Modules**:
   - In Python (PEP 8), double leading underscores (`__name`) invoke name mangling to avoid subclass collisions rather than creating a third visibility tier below `_name`. Treating `name` and `__dunder__` as **Public** and `_name` / `__mangled` as **Private** provides a clean 2-tier split.

---

## 2. Empirical Measurements Across the 5 Corpora

### 2.1 Call-Cluster Colocation, Shared Helpers, and Within-Cluster Call Direction

| Metric / Invariant | **Omni `src/`** (88 Rust files) | **ripgrep 15.2.0** (84 Rust files) | **cargo 0.100.0** (261 Rust files) | **The legacy linter** (Python) | **CPython 3.14.8** (661 Python modules) |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Call-Cluster Colocation** *(private helpers contiguous with their caller's cluster)* | **99.2%** (`117/118`) | **94.3%** (`33/35`) | **93.9%** (`494/526`) | **93.8%** modules (`15/16`)<br>**100.0%** classes (`36/36`) | **79.5%** modules (`517/650`)<br>**75.1%** classes (`822/1,094`) |
| **Cluster-Aware `private-before-public` in `impl` / `class`** *(vs. strict scope-wide)* | **100.0%** (`33/33` after 3 fixes) | **84.8%** (`28/33`, up from `75.8%`) | **60.3%** (`47/78`, up from `38.5%`) | **100.0%** (`26/26`) | **37.6%** (`172/458`, up from `23.4%`) |
| **Shared Helpers (`|Roots| >= 2`) Placed After Callers** | **100.0%** in `impl`s (`1/1`) | **81.2%–100%** (`13/16` `impl`s, `2/2` modules) | **75.5%** modules (`80/106`)<br>**75.0%** after 1st caller in `impl`s | — | **61.7%** after 1st caller in classes (`320/519`) |
| **Private-to-Private Call Direction When Public Is First** | Top-down in `impl`s (`89%`) | **87.5% Top-Down** (`35` vs `5`) | **84.9% Top-Down** (`477` vs `85`) | **90.0% Top-Down** in classes (`18` vs `2`) | Mixed (`47%` top-down) |

### 2.2 Evaluation of All Declaration-Ordering Concepts

| ID | Rule / Concept | Languages | Verdict & Rationale |
| :--- | :--- | :--- | :--- |
| **C1** | **`field-after-method`** | Python | **ADOPT (`Precision::Exact`, `Consensus::Unopinionated`)**: **100.0% compliant** across all 1,591 classes in CPython 3.14.8 (`0` hits for `Stmt::AnnAssign` `x: T` after `def`). Protects `@dataclass` / Pydantic field order and reinforces `inline-public-attribute-annotation`. |
| **C2** | **`associated-item-after-method`** | Rust | **ADOPT (`Precision::Exact`, `Consensus::Unopinionated`)**: **100.0%** in Omni `src/`, **98.5%** in ripgrep, **95.4%** in cargo. Associated `type` and `const` items parameterize `fn` signatures below them. |
| **C3** | **`constructor-after-method`** | Python, Rust | **ADOPT (`Precision::Exact`, `Consensus::Unopinionated`)**: **97.5%–100%** compliant across all 5 corpora. Primary lifecycle initializers always precede regular methods. |
| **C4** | **`uncolocated-helper`** *(Priority 1)* | Python, Rust | **ADOPT (`Precision::Exact`, `Consensus::Opinionated`)**: **93.8%–99.2% compliant** across Omni, ripgrep, cargo, and the legacy linter. Enforces that exclusive private helpers (`|Roots(h)| = 1`) stay contiguous with their owning public entrypoint without unrelated functions in between. |
| **C5** | **`private-before-public-function`** *(Priority 2, cluster-aware)* | Python, Rust | **ADOPT (`Precision::Exact`, `Consensus::Opinionated`)**: Enforces that exclusive helpers appear below their owning public entrypoint (`[pub_a, _a1, pub_b]` allowed) and shared helpers (`|Roots(h)| >= 2`) appear in the shared layer below all of their public callers. |
| **C6** | **`callee-before-caller`** *(Priority 3, same-tier & same-group)* | Python, Rust | **ADOPT (`Precision::Exact`, `Consensus::Opinionated`)**: Enforces top-down abstraction order (`caller` before `callee`) within the same visibility and root-set group (`pub -> pub`, exclusive `priv -> priv` of the same owner, or shared `priv -> priv`), exempting mutual recursion (SCCs). Replaces bottom-up `call-before-definition`. |
| **C7** | **`statement-after-main-guard`** | Python | **ADOPT (`Precision::Exact`, `Consensus::Unopinionated`, `ImpactedQuality::Reliability`)**: **99.1% compliant** in CPython (`217/219`). Catches runtime `NameError`s when a `def`/`class` declared below `if __name__ == "__main__":` is called from `main()` (which Ruff `F821` misses). |
| **C8** | **File-wide `constant-after-function` / `use-after-item`** | Python, Rust | **REJECT**: Tested on Omni `src/`, file-wide `constant-after-function` would force feature-local constants (`LITERAL_EXEMPT_MACROS` in `ast/rust.rs:1254`, `MUTATING_METHODS` in `ast/python.rs:689`) 680–1,250 lines away from the only function cluster that uses them, actively hurting colocation. |

---

## 3. Summary of Validated Decisions (`D1`–`D6`)

| ID | Decision | Validated Resolution |
| :--- | :--- | :--- |
| **D1** | **Replace bottom-up `call-before-definition` with top-down `callee-before-caller` (Priority 3)** | Bottom-up ordering inverted abstraction levels inside colocated clusters (`[pub_a, _a2, _a1]`). Top-down `callee-before-caller` within the same `(visibility, root_set)` group ensures a monotonic high-to-low abstraction gradient (`[pub_a, _a1, _a2]`). |
| **D2** | **3-Rule Precedence Hierarchy over a Shared Call-Cluster Engine** | Implement one shared root-ownership call-cluster analyzer in `src/code_lint/ast.rs` evaluating:<br>1. `uncolocated-helper` (Priority 1)<br>2. `private-before-public-function` (Priority 2)<br>3. `callee-before-caller` (Priority 3)<br>Across both module-level functions and class/`impl` methods in Python and Rust, suppressing lower-priority diagnostics on any function already flagged by a higher-priority rule. |
| **D3** | **Exclusive (`|Roots| = 1`) vs. Shared (`|Roots| >= 2`) Helpers** | Exclusive helpers belong to their single public owner's contiguous unit; shared helpers belong to a lower abstraction layer and must sit below all of their public callers (at the end). |
| **D4** | **2-Tier Visibility in Rust & Python** | Exported (`pub`, `pub(...)`) vs. Private (bare `fn`) in Rust; Public & dunder (`name`, `__dunder__`) vs. Private (`_name`, `__mangled`) in Python. |
| **D5** | **Complete 7-Rule `Topic::DECLARATION_ORDER` Suite** | 1. `field-after-method` (Python)<br>2. `associated-item-after-method` (Rust)<br>3. `constructor-after-method` (Python, Rust)<br>4. `uncolocated-helper` (Python, Rust)<br>5. `private-before-public-function` (Python, Rust)<br>6. `callee-before-caller` (Python, Rust)<br>7. `statement-after-main-guard` (Python) |
| **D6** | **Self-Dogfooding on Omni `src/`** | Colocate `SuppressionTracker::parse_comment_text` immediately below `from_file` and `InterceptedCommand::parse_single` immediately below `parse_all`, and reorder the remaining module/impl helper clusters in `src/` to pass `test_self_dogfooding_code_lint` with 0 violations. |
