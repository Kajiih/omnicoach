# Abstraction graph: 01 Understand

> **Status: ALPHA / EXPERIMENTAL — Active Investigation & Design** (revision 3; subject to change at any time). Next: [02_references.md](02_references.md).

Markers: **[V]** verified in code or a primary source; **[U]** not verified (assumption).

## 1. Problem as Currently Understood

Readers of a project benefit from a graph of its main components and how they relate. Today such graphs are drawn by hand and go stale.

**End goal (context, not this cycle's scope):** derive a multi-level view of a project's abstractions from the code and its declared components, regenerated from the source so it cannot go stale. The view separates abstractions from implementation: a reader sees components and the abstractions they expose first, and drills down into modules and helpers on demand.

**This cycle (Stage A):** start inside a single module. Show how the linter already sees the relations between the abstractions of one module, and judge whether that view is useful as documentation. Only if it works well do we generalize to a whole project (Stage B, D10).

This is an **investigation**: the output is evidence and a recommendation, not a feature (D3).

## 2. What Already Exists [V]

### 2.1 Inside a Module (Stage A)

The call-cluster analyzer in [ast.rs](../../../src/code_lint/ast.rs) builds, for every module, Python `class`, and Rust inherent `impl`:

| Knowledge | Meaning | Reference |
| :--- | :--- | :--- |
| Declarations | functions and types in source order, with kind and visibility | `ScopeItem` |
| Ownership | for each private item, the public entrypoints that reach it through private items only (`Roots(h)`); an entrypoint plus its exclusive helpers is a component inside the module | ADR 011 §3.1 |
| Abstraction ordering | who is higher-level than whom: calls, containment, `impl` behavior, encapsulation in a body, signature contracts | ADR 011 §3.4 |
| Recursion groups | items that call each other in a cycle | `compute_tarjan_scc` |
| Misplacements | findings of `private-before-public`, `uncolocated-helper`, `callee-before-caller` | `CallClusterFindings` |

Gaps relevant to an abstraction view:

- For a type, "contains a field of type `U`" and "its `impl` calls `g`" are stored in one list, so composition and behavior can't be told apart (D8 requires telling them apart).
- Methods in `impl` and `class` scopes don't record which references are in parameters versus return types.
- A module scope and the `impl` scopes inside the same file are separate graphs with no edges between them, so "a type and its methods" is not one picture yet.
- The analyzer computes ownership and ordering, uses them for findings, and discards them.

### 2.2 Across Modules (Stage B context only)

| Knowledge | Where |
| :--- | :--- |
| Declared components and allowed dependencies | `define_architecture!` in [architecture.rs](../../../src/architecture.rs) (ADR 006) |
| Which component each file belongs to | `architecture_component!(X)` in root files, inherited by children |
| Actual references between components | resolved `crate::` paths in [architecture_conformance.rs](../../../tests/architecture_conformance.rs), used for checking only |

ADR 011 §2 already frames a component across modules and an entrypoint with its helpers inside a module as the same pattern at two scales, which is why Stage A can inform Stage B.

## 3. Goals (Stage A)

| ID | Goal | Reason |
| :--- | :--- | :--- |
| G1 | Determine whether the linter's view of one module (ownership, ordering, relations) gives a reader a useful overview of its abstractions. | The core claim behind "dynamic documentation"; it gates Stage B. |
| G2 | Define, inside a module, what separates abstractions (public entrypoints and types) from implementation (private helpers), and how implementation folds into its owner. | The user's aim is to decouple abstractions from the helpers that implement them. |
| G3 | Identify which information the view must carry (relations by kind, ownership, recursion groups, positions, findings) to be faithful, and which gaps in §2.1 must close. | Merged or missing relations would make the view misleading (D8). |
| G4 | Identify how that information could be consumed by existing tools, without building a viewer. | Other tools visualize it however they want (D4). |
| G5 | Evaluate on real modules: from our crate first, then from the reference corpora (ripgrep, cargo, cpython). | Toy examples hide scale and readability problems. |
| G6 | State, from the Stage A evidence, whether and how to generalize to Stage B. | The user wants to generalize only if the module view works well. |

## 4. Non-Goals

| ID | Non-goal | Reason |
| :--- | :--- | :--- |
| NG1 | Views across modules, components, or the whole crate. | Stage B, only after Stage A succeeds (D10). |
| NG2 | Shipping a CLI command, library API, or stable schema. | Too early: investigation only (D3). |
| NG3 | Building our own renderer or viewer. | Other tools own visualization (D4). |
| NG4 | Changing lint behavior. | The ordering rules are settled; the investigation only reads their data. |
| NG5 | Deciding how `omnicoach` users would get this on their own repos. | User: not our question for now. |

## 5. Decisions

| ID | Decision | Source |
| :--- | :--- | :--- |
| D1 | The end goal is a multi-level view from components down to declarations. | User |
| D2 | At the project level, declared components are the top level, used as they are (no new component model). | User |
| D3 | Investigation only: no product code, CLI, or schema commitment. | User |
| D4 | No consumer exists; the question is whether this is a good dynamic documentation medium. | User |
| D5 | The earlier `declaration_graph_export_investigation.md` is premature and only an input for Phase 2. | Agent, after user feedback |
| D6 | Audience: people reading a codebase. Onboarding is the primary case; code review and refactoring also count. May later extend to `omnicoach` users on their own repos. | User |
| D7 | The view must show both relations (who references whom) and abstraction levels (who is higher-level than whom). | User |
| D8 | Relations must be distinguishable by kind, for example composition versus behavior for types. | User |
| D9 | "Dynamic" means regenerated from the source on demand, not live while editing. Both Rust and Python matter. | User |
| D10 | Staged scope: Stage A (this cycle) covers one module as the linter sees it. Stage B (whole project) starts only if Stage A works well. | User |
| D11 | Hierarchy for the end goal: crate → declared component → module/file → public abstraction (an entrypoint or type with its exclusive helpers) → private implementation. Stage A covers the last two levels. | User (Q8) |
| D12 | Abstraction versus implementation: at each level, the public surface is the abstraction; private items are implementation, folded into their owner (`Roots(h)`) until the reader drills down. | User (Q9) |
| D13 | "Works well" is decided against reader journeys (CUJs) with acceptance criteria, defined in Phase 3; the Stage B decision is made against them. | User (Q12) |
| D14 | Whether a type and its methods form one picture (module scope merged with its `impl` scopes) is left open: Phase 3 prototypes the options and compares them against the CUJs. | User (Q13) |

## 6. Open Questions

| ID | Question |
| :--- | :--- |
| Q10 | (Stage B) For code without declared components, should the top levels fall back to the module tree? |
| Q11 | (Stage B) Between components, show declared dependencies, actual references, or both? |
| Q12 | ~~What does "works well" mean?~~ Answered (D13). |
| Q13 | ~~A type and its methods as one picture?~~ Answered (D14). |

## 7. Related Ideas (Not Yet in Scope)

- **Layered layout from abstraction levels:** rows from entrypoints down to leaves, mirroring the reading order the rules enforce.
- **Declared versus actual:** overlay real references on `ARCHITECTURE_GRAPH` to show drift (Stage B).
- **Documentation diffs:** compare the view between two commits to show how a change reshaped a module's abstractions.
- **Agent context:** give agents a compact abstraction map of a module instead of the whole file.
- **Playground view:** the WebAssembly playground could host a viewer, if one is ever wanted.
