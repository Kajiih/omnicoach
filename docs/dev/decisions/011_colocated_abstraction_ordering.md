# ADR 011: Colocated Component Units and Abstraction-Layer Ordering

## 1. Problem Statement

Traditional declaration-ordering lint rules force a false dichotomy between **public-first visibility grouping** and **vertical-slice colocation**, or invert abstraction levels:

1. **Scope-Wide Visibility Buckets Alone Break Vertical-Slice Modules (`StyleCop SA1202`, `@typescript-eslint/member-ordering`, `WPS338`, `CCE001`)**:
   - Grouping all `public` functions at the top of a scope and all `private` helpers at the bottom works well for cohesive types and small utility modules (`[new, pub_a, pub_b, _helpers]`), but in multi-subsystem modules (`src/code_lint/ast.rs`, `src/config.rs`) it rips a feature unit's dedicated helpers away from the public function they implement.
2. **Mandatory Inline Colocation Breaks Public-First Types and Utility Modules**:
   - Conversely, forcing *every* single-use helper immediately below its caller wedges multi-step private implementation details (`parse_single`, `parse_comment_text`, `find_directive_prefix`) between a type's constructor (`from_file`, `parse_all`) and its remaining public API (`is_suppressed`, `program`), burying the public contract of the type.
3. **Bottom-Up Call Ordering (`no-use-before-define`, `DefineBeforeUseRule`) Inverts Abstraction Levels**:
   - Requiring every callee to be declared before its caller places low-level leaf utilities at the top of a scope and buries high-level entrypoints at the bottom.
   - Even when restricted to private helpers below a public entrypoint (`pub_a` calls `_step_1`, which calls `_step_2`), bottom-up ordering produces `[pub_a, _step_2, _step_1]`—wedging the leaf helper `_step_2` between `pub_a` and its direct callee `_step_1` and flipping the reading direction mid-cluster.
4. **Naive Connected Components (`LCOM4`) Collapse When Helpers Are Shared**:
   - Standard cohesion metrics (`LCOM4`, Hitz & Montazeri 1995) model a scope as an undirected call graph and treat each connected component as a feature cluster.
   - As soon as two independent public entrypoints (`pub_a` and `pub_b`) both call a single low-level utility (`_shared`), undirected connected components merge `pub_a`'s cluster and `pub_b`'s cluster into one component—failing to distinguish single-owner helpers from shared helpers.

---

## 2. Theoretical Foundation & Isomorphism with ADR 006

In [ADR 006](006_architectural_dag_and_conformance.md) (`src/architecture.rs`), Omni established three structural invariants at the **crate and module level**:

1. **Self-Contained Component Units (Colocation)**: Each feature module keeps its entrypoint and its exclusive implementation details together.
2. **Sibling Isolation & Shared Lower Layer**: Sibling components never reach into each other's internals; whenever logic is shared by two or more components, it moves down to a lower shared abstraction layer (`code_lint::policy`, `code_lint::semantic`, `code_lint::ast`).
3. **Monotonic Abstraction Gradient**: Higher-level orchestrators depend downward on lower-level foundations.

Inside a single module, class, or inherent `impl` block, the **same invariants** govern how functions and methods are ordered—without requiring any annotations or macros, because **visibility modifiers and the intra-scope call graph already define the dependency DAG**:

| Architectural Concept | Inter-Module Scale (ADR 006) | Intra-Module / Intra-Class Scale (ADR 011) |
| :--- | :--- | :--- |
| **Component Definition Unit** | A declared `ArchitectureComponent` (or leaf rule module) | A **public entrypoint** (`pub fn` / public method) together with the **single-use private helpers** (`Roots(h) = {p}`) reachable only from that entrypoint |
| **Two Valid Helper Places (`uncolocated-helper`)** | Internal helpers live either inside their owning module or in a lower shared layer—never stranded in an unrelated sibling | Once a private helper `h` is placed below its public callers (`pos(h) > max(Roots(h))`), it may sit in **only two valid places**:<br>1. **Immediately after its consumer** (`Roots(h) = {p}` only): in the contiguous helper cluster directly below `p` (vertical-slice style).<br>2. **At the end of the scope** (`\|Roots(h)\| >= 1`): in the trailing private helper section after all public functions (`pos(h) > last_pub`, public-first style), provided a single-user helper does not split its owner's helpers between Place 1 and Place 2. |
| **Public Before Private (`private-before-public-function`)** | Public layer precedes internal implementation layers | Every private helper `h` must be declared **after** all public entrypoints that reach it (`pos(h) > max(Roots(h))`), and uncalled private helpers must follow all public entrypoints. |
| **Abstraction Gradient (`callee-before-caller`)** | Higher layers depend on lower layers (`Runner -> Rules -> Semantic -> Ast`) | Among **private helpers** (`Private -> Private`), higher-abstraction **callers** precede lower-abstraction **callees** (`_step_1 -> _step_2` or `_exclusive -> _shared`), while `Public -> Public` calls are unconstrained so public APIs can order either orchestrator-first or core-primitive-first. |

This aligns with empirical program-comprehension research:
- **Top-Down Plan Recognition (Brooks 1983; Soloway & Ehrlich 1984; Von Mayrhauser & Vans 1995)**: Readers orient via high-level contracts (fields, constructors, public entrypoints) before descending into implementation helpers.
- **Eye-Tracking & The Stepdown Rule (Busjahn et al. 2015; Martin 2008, *Clean Code* Ch. 5)**: Readers scan entrypoints and step downward from caller to callee.

---

## 3. Decision: The Root-Ownership Call-Cluster Model

### 3.1 Private-Subgraph Root Ownership

Within a scope (a module's top-level functions, a Python `class` body, or a Rust inherent `impl` block), every callable is classified as either a **Public Entrypoint** or a **Private Helper**:

- **Public Entrypoints**:
  - In a class or inherent `impl`: constructors and public/exported methods (`pub`, `pub(crate)`, `pub(super)`, `pub(in ...)` in Rust; non-`_` methods and `__dunder__` methods in Python).
  - In a module: exported functions (`pub` / `pub(...)` in Rust; non-`_` functions in Python), plus `fn main` in Rust.
- **Root Set of a Private Helper (`Roots(h)`)**:
  - For each private helper `h`, `Roots(h)` is the set of public entrypoints in the scope that can reach `h` **walking only through private helpers** (stopping at public boundaries so that if `pub_b` calls `pub_a` and `pub_a` calls `_a1`, `_a1` still has `Roots(_a1) = {pub_a}`).
  - Based on the number of public roots reaching `h`:
    1. **Single-User (Exclusive) Helper (`|Roots(h)| = 1`, `Roots(h) = {p}`)**: Serves a single public entrypoint `p` and may be placed either in **Place 1** (immediately after `p` in its contiguous cluster `Cluster(p)`) or in **Place 2** (in the trailing private helper section after all public functions, provided none of `p`'s helpers are in Place 1).
    2. **Multi-User (Shared) Helper (`|Roots(h)| >= 2`, `Roots(h) = {p_1, p_2, ...}`)**: Serves multiple public entrypoints and must be placed in **Place 2** (in the trailing private helper section after all public functions in the scope).

### 3.2 Canonical Scope Layouts

Both **Vertical-Slice Scopes** (using Place 1 for single-user helpers and Place 2 for shared helpers) and **Public-First Scopes** (using Place 2 for all private helpers) are first-class valid layouts, while stranding a helper in the middle of a scope between unrelated public functions (`[pub_a, pub_b, _a1, pub_c]` or `[pub_a, pub_b, _shared_ab, pub_c]`) is rejected:

```python
# Layout A: Vertical-Slice Scope (Place 1 for 1-user helpers, Place 2 for shared helpers)
def pub_a() -> None:        # Public entrypoint A
    _a_step_1()

def _a_step_1() -> None:    # Place 1: 1-user helper immediately after pub_a
    _a_step_2()

def _a_step_2() -> None:    # Place 1: sub-helper below caller _a_step_1
    _shared_1()

def pub_b() -> None:        # Public entrypoint B
    _b_step_1()

def _b_step_1() -> None:    # Place 1: 1-user helper immediately after pub_b
    _shared_1()

def _shared_1() -> None:    # Place 2: 2-user helper in trailing helper section (pos > last_pub)
    _shared_2()

def _shared_2() -> None:    # Place 2: shared leaf helper below caller _shared_1
    ...
```

```python
# Layout B: Public-First Scope (all public entrypoints first, all private helpers in Place 2)
def pub_a() -> None:        # Public entrypoint A
    _a_step_1()

def pub_b() -> None:        # Public entrypoint B
    _b_step_1()

def _a_step_1() -> None:    # Place 2: 1-user helper in trailing helper section
    _shared_1()

def _b_step_1() -> None:    # Place 2: 1-user helper in trailing helper section
    _shared_1()

def _shared_1() -> None:    # Place 2: 2-user helper below its callers _a_step_1 and _b_step_1
    ...
```

### 3.3 The Three Disjoint Call-Cluster Rules

Because visibility, helper placement, and caller-callee ordering are projections of one structural order over the call graph, they are computed by **one shared call-cluster analyzer** in [src/code_lint/ast.rs](../src/code_lint/ast.rs) and evaluated in three mutually exclusive stages (so a single misplaced function is never reported twice):

| Stage | Rule Name | Invariant Enforced |
| :--- | :--- | :--- |
| **Stage 1 (`pos < max(Roots(h))`)** | **`private-before-public-function`** | • A **single-user helper** (`Roots(h) = {p}`) must be declared **below** its owning public entrypoint `p` (`pos(h) > pos(p)`).<br>• A **multi-user helper** (`\|Roots(h)\| >= 2`) must be declared **below all** of its public callers (`pos(h) > max(Roots(h))`).<br>• An **unrooted private function** (`\|Roots(h)\| = 0`) must be declared below all public functions in the scope (`pos(h) > last_pub`). |
| **Stage 2 (`pos > max(Roots(h))`)** | **`uncolocated-helper`** | Once a rooted private helper `h` (`\|Roots(h)\| >= 1`) is below all of its public callers, it must sit in **one of two valid places**:<br>• **Place 1 (Immediately after consumer, `Roots(h) = {p}` only)**: every item between `p` and `h` also has `Roots == {p}` (or is a sibling constructor when `p` is a constructor).<br>• **Place 2 (Trailing helper section at the end of the scope, `\|Roots(h)\| >= 1`)**: `pos(h) > last_pub` (and if `Roots(h) = {p}`, none of `p`'s helpers were placed in Place 1 before `last_pub`). |
| **Stage 3 (`Private -> Private` Order)** | **`callee-before-caller`** | Among all private callables not flagged by Stage 1 or Stage 2 (`Private -> Private`), a `callee` must not be declared before its `caller` (unless they belong to the same Strongly Connected Component of mutually recursive functions, or in a scope where the caller is unrooted and the callee is rooted). |

---

## 4. Consequences

1. **Zero Contradictions Across Rules**:
   - Stage 1 (`pos < max(Roots(h))`), Stage 2 (`pos > max(Roots(h))` outside Place 1 and Place 2), and Stage 3 (`Private -> Private` `callee < caller`) partition the failure modes cleanly without overlapping diagnostics.
2. **Supports Both Public-First Types and Vertical-Slice Modules**:
   - Cohesive classes, inherent `impl` blocks, and small utility modules (`InterceptedCommand`, `SuppressionTracker`, `CommentIndex`, `test_utils.rs`, `scopes.rs`) can group all public methods/functions at the top and place all private helpers in the trailing helper section (Place 2), while multi-subsystem modules (`ast.rs`, `config.rs`, `ast/rust.rs`) can colocate 1-user helpers immediately after their public entrypoint (Place 1) and keep 2+-user helpers at the end of the file (Place 2).
3. **Complete `Topic::DECLARATION_ORDER` Suite**:
   - Together with `field-after-method` (Python), `associated-item-after-method` (Rust), `constructor-after-method` (Python, Rust), and `statement-after-main-guard` (Python), Omni enforces a coherent top-to-bottom declaration order across both types and modules.
