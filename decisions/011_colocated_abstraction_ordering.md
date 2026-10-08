# ADR 011: Colocated Component Units and Abstraction-Layer Ordering

## 1. Problem Statement

Traditional declaration-ordering lint rules force a false dichotomy between **visibility grouping** and **call-graph ordering**, breaking **Locality of Behavior (colocation)** and mixing levels of abstraction:

1. **Class-Wide or Module-Wide Visibility Buckets Break Colocation (`StyleCop SA1202`, `@typescript-eslint/member-ordering`, `WPS338`, `CCE001`)**:
   - Grouping all `public` functions or methods at the top of a scope and all `private` helpers at the bottom forces a helper used exclusively by the first public entrypoint (`from_file` -> `parse_comment_text`) to move below unrelated public methods (`is_suppressed`, `audit_suppressions`).
   - At the module level, file-wide buckets rip a feature unit's dedicated helpers away from the public function they implement, scattering a single behavior across hundreds of lines.
2. **Bottom-Up Call Ordering (`no-use-before-define`, `DefineBeforeUseRule`) Inverts Abstraction Levels**:
   - Requiring every callee to be declared before its caller places low-level leaf utilities at the top of a scope and buries high-level entrypoints at the bottom.
   - Even when restricted to private helpers below a public entrypoint (`pub_a` calls `_step_1`, which calls `_step_2`), bottom-up ordering produces `[pub_a, _step_2, _step_1]`—wedging the leaf helper `_step_2` between `pub_a` and its direct callee `_step_1` and flipping the reading direction mid-cluster.
3. **Naive Connected Components (`LCOM4`) Collapse When Helpers Are Shared**:
   - Standard cohesion metrics (`LCOM4`, Hitz & Montazeri 1995) model a scope as an undirected call graph and treat each connected component as a feature cluster.
   - As soon as two independent public entrypoints (`pub_a` and `pub_b`) both call a single low-level utility (`_shared`), undirected connected components merge `pub_a`'s cluster and `pub_b`'s cluster into one component—failing to detect when `pub_a`'s exclusive helpers are interleaved with `pub_b`'s exclusive helpers.

---

## 2. Theoretical Foundation & Isomorphism with ADR 006

In [ADR 006](006_architectural_dag_and_conformance.md) (`src/architecture.rs`), Omni established three structural invariants at the **crate and module level**:

1. **Self-Contained Component Units (Colocation)**: Each feature module keeps its entrypoint and its exclusive implementation details together.
2. **Sibling Isolation & Shared Lower Layer**: Sibling components never reach into each other's internals; whenever logic is shared by two or more components, it moves down to a lower shared abstraction layer (`code_lint::policy`, `code_lint::semantic`, `code_lint::ast`).
3. **Monotonic Abstraction Gradient**: Higher-level orchestrators depend downward on lower-level foundations.

Inside a single module, class, or inherent `impl` block, the **exact same three invariants** govern how functions and methods should be ordered—without requiring any annotations or macros, because **visibility modifiers and the intra-scope call graph already define the dependency DAG**:

| Architectural Concept | Inter-Module Scale (ADR 006) | Intra-Module / Intra-Class Scale (ADR 011) |
| :--- | :--- | :--- |
| **Component Definition Unit** | A declared `ArchitectureComponent` (or leaf rule module) | A **public entrypoint** (`pub fn` / public method) together with the **exclusive private helpers** reachable only from that entrypoint |
| **Sibling Isolation & Colocation** | Sibling rule files cannot import each other | Exclusive helpers of `pub_a` must stay contiguous with `pub_a`; another unit `pub_b` or a shared helper cannot sit between `pub_a` and its exclusive helpers (**`uncolocated-helper`**) |
| **Shared Foundation Layer** | Shared helpers move down to `policy` / `semantic` / `ast` | Private helpers called by **2 or more** public entrypoints belong to a lower abstraction layer and move to the **shared helper section at the end** below all their public callers (**`private-before-public-function`**) |
| **Abstraction Gradient** | Higher layers depend on lower layers (`Runner -> Rules -> Semantic -> Ast`) | Within every unit and within the shared layer, higher-abstraction **callers** precede lower-abstraction **callees** (`pub_a -> _step_1 -> _step_2`, enforced by **`callee-before-caller`**) |

This aligns with empirical program-comprehension research:
- **Top-Down Plan Recognition (Brooks 1983; Soloway & Ehrlich 1984; Von Mayrhauser & Vans 1995)**: Readers orient via high-level contracts (fields, constructors, public entrypoints) before descending into implementation helpers.
- **Eye-Tracking & The Stepdown Rule (Busjahn et al. 2015; Martin 2008, *Clean Code* Ch. 5)**: Readers scan entrypoints and step downward from caller to callee; keeping direct callees immediately below their caller minimizes vertical distance.

---

## 3. Decision: The Root-Ownership Call-Cluster Model

### 3.1 Private-Subgraph Root Ownership

Within a scope (a module's top-level functions, a Python `class` body, or a Rust inherent `impl` block), every callable is classified as either a **Public Entrypoint** or a **Private Helper**:

- **Public Entrypoints**:
  - In a class or inherent `impl`: constructors and public/exported methods (`pub`, `pub(crate)`, `pub(super)`, `pub(in ...)` in Rust; non-`_` methods and `__dunder__` methods in Python).
  - In a module: exported functions (`pub` / `pub(...)` in Rust; non-`_` functions in Python), plus any private function directly referenced as an entrypoint from a module-level exported item (`pub const RULE`, `pub static`), a trait `impl` block (`impl Visitor for ...`), a Python class body / decorator, or a `__main__` block. If a module contains only private functions and no such external root, functions with in-degree 0 in the module's DAG act as the scope's roots.
- **Root Set of a Private Helper (`Roots(h)`)**:
  - For each private helper `h`, `Roots(h)` is the set of public entrypoints in the scope that can reach `h` **walking only through private helpers** (stopping at public boundaries so that if `pub_b` calls `pub_a` and `pub_a` calls `_a1`, `_a1` still has `Roots(_a1) = {pub_a}`).
  - Based on the number of public roots reaching `h`:
    1. **Exclusive Helper (`|Roots(h)| = 1`, `Roots(h) = {p}`)**: Serves a single public entrypoint `p` and belongs to `p`'s **colocated unit** `Cluster(p)`.
    2. **Shared Helper (`|Roots(h)| >= 2`, `Roots(h) = {p_1, p_2, ...}`)**: Serves multiple public entrypoints and belongs to the **shared helper layer** at a lower level of abstraction than any individual public unit.

### 3.2 Canonical Scope Layout

Every scope orders its declarations along a single monotonic abstraction gradient from highest abstraction (top) to lowest abstraction (bottom):

```python
# 1. State Schema & Lifecycle Initializers
#    (field-after-method, associated-item-after-method, constructor-after-method)

# 2. Colocated Component Unit A (owner: pub_a)
def pub_a() -> None:        # Public entrypoint A (highest abstraction in Unit A)
    _a_step_1()

def _a_step_1() -> None:    # Exclusive direct helper of pub_a (colocated right below pub_a)
    _a_step_2()

def _a_step_2() -> None:    # Exclusive sub-helper of pub_a (lower abstraction -> below caller)
    _shared_1()

# 3. Colocated Component Unit B (owner: pub_b)
def pub_b() -> None:        # Public entrypoint B (allowed after _a_step_1/_a_step_2!)
    _b_step_1()

def _b_step_1() -> None:    # Exclusive helper of pub_b
    _shared_1()

# 4. Shared Helper Layer (used by both Unit A and Unit B)
def _shared_1() -> None:    # Shared helper (placed below all public units that call it)
    _shared_2()

def _shared_2() -> None:    # Shared leaf helper (lower abstraction -> below _shared_1)
    ...

# 5. Execution / Test Footer (statement-after-main-guard)
```

### 3.3 The Three Precedence-Linked Call-Cluster Rules

Because colocation, visibility, and caller-callee ordering are projections of one structural order over the call graph, they are computed by **one shared call-cluster analyzer** in `src/code_lint/ast.rs` and exposed as three rules evaluated in strict priority order (suppressing lower-priority findings on any function already flagged by a higher-priority rule):

| Priority | Rule Name | Invariant Enforced |
| :--- | :--- | :--- |
| **1 (Highest)** | **`uncolocated-helper`** | Every **exclusive private helper** (`Roots(h) = {p}`) must stay contiguous with its owning public entrypoint `p`—no function outside `Cluster(p)` (neither another public entrypoint `pub_b` nor a shared helper) may appear between `p` and `h`. |
| **2 (Middle)** | **`private-before-public-function`** | • An **exclusive helper** (`Roots(h) = {p}`) must be declared **below** its owning public entrypoint `p` (and is allowed to precede later public entrypoints `pub_b` because it belongs to `Cluster(p)` above it).<br>• A **shared helper** (`|Roots(h)| >= 2`) must be declared **below all** of its public callers (`pos(h) > max(Roots(h))`), placing it in the shared layer after the units that depend on it.<br>• An uncalled private function (`|Roots(h)| = 0`) must be declared below all public functions in the scope. |
| **3 (Lowest)** | **`callee-before-caller`** | Within the **same visibility and abstraction group**, a lower-abstraction `callee` must not be declared before its higher-abstraction `caller` (unless they belong to the same Strongly Connected Component of mutually recursive functions):<br>• **Public -> Public**: If non-constructor `pub_a` calls non-constructor `pub_b`, `pub_a` (and its unit) precedes `pub_b` (unless `pub_b` is a shared public foundation called by multiple public functions, or in standard top-down order `caller` precedes `callee`).<br>• **Exclusive Private -> Exclusive Private (`Roots = {p}`)**: Within `Cluster(p)`, `caller` precedes `callee` (`_a_step_1` before `_a_step_2`).<br>• **Shared Private -> Shared Private (`|Roots| >= 2`)**: Within the shared layer, `caller` precedes `callee` (`_shared_1` before `_shared_2`).<br>*(Never fires across `public -> private` or `exclusive -> shared`, which are governed by Priorities 1 and 2.)* |

---

## 4. Consequences

1. **Zero Contradictions Across Rules**:
   - Because Priorities 1, 2, and 3 all point along the same downward abstraction gradient (`Public Entrypoint -> Direct Exclusive Helper -> Leaf Exclusive Helper -> Shared Helper ->Shared Leaf Helper`), fixing any rule's diagnostic moves the function toward its single canonical position and never triggers another rule.
2. **True Colocation Without Annotation Boilerplate**:
   - Multi-feature classes, `impl` blocks, and modules can colocate `[pub_a, _a1, pub_b, _b1]` naturally (fixing the `SuppressionTracker::parse_comment_text` and `InterceptedCommand::parse_single` placement in Omni's own codebase) without needing manual region comments or macro markers.
3. **Complete `Topic::DECLARATION_ORDER` Suite**:
   - Together with `field-after-method` (Python), `associated-item-after-method` (Rust), `constructor-after-method` (Python, Rust), and `statement-after-main-guard` (Python), Omni enforces a coherent top-to-bottom declaration order across both types and modules.
