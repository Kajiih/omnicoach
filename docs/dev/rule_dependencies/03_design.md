# Rule dependencies and soundness: 03 Design Exploration

> [!IMPORTANT]
> **Status: EXPLORATORY PROTOTYPE (2026-10-06). Both design and implementation deferred (D13, §4).**
> This document records a first design attempt and its throwaway prototypes for option A (`01_understand.md` §6). **Do not treat this as a settled specification to follow blindly when resuming.** We deferred *both* the design and the rollout because today's 41 rules contain only 8 Omni ↔ Omni directed edges (5 in one rule family) and 0 Omni ↔ Omni overlaps. When we return to this with a larger rule corpus, every choice below must be challenged, re-evaluated against the new rules, and replaced if a simpler or stronger abstraction emerges.

## 1. Placement & Rule Naming (Exploratory)

### 1.1 On `Declaration`, next to `classification`

`RuleDoc` (`src/rule_declaration/documentation.rs`) is user-facing prose, with a `RuleDoc::TODO` placeholder. Relations are structural data, read by `RuleCatalog` (`--explain`), the option D harness, and future option B warnings. In this spike, they sit on `Declaration` (`src/rule_declaration.rs`) alongside `classification` and `options`, in `src/rule_declaration/relations.rs`.

### 1.2 Naming another rule without relaxing sibling isolation

A relation target names another rule. Rule files cannot import each other (`src/architecture.rs:14–15`, enforced by `tests/architecture_conformance.rs:399–420`). The registries (`CODE_RULES`, `REGISTERED_RULES`) are built from every rule, so a rule cannot refer to them either without creating a module cycle.

Relaxing sibling isolation is not RICR:
- Rules would start reusing each other's private helpers instead of extracting shared domain logic into `code_lint::policy` or `code_lint::semantic`.
- A `code_lint` rule still could not name a suppression audit or a `command_lint` rule, which live in separate architecture components.

Instead, if compile-time rule identifiers are used, they belong where `RuleName` already lives: `Diagnostic` (`src/diagnostic.rs:12`), at the bottom of the DAG, already imported by every rule and registry. A rule's `Declaration.name` and every relation target would share that compile-time identifier so typos fail to compile (`E0599`), matching how `ArchitectureComponent` (`src/architecture.rs`) and `Topic` (`src/rule_declaration/taxonomy.rs`) work. The tradeoff is maintaining a central list of rule names alongside rule modules (or keeping `RuleName("...")` strings validated by a registry test — see §5 Q2).

## 2. Prototypes & Tradeoffs

### 2.1 Omni ↔ Omni (`V1` vs. `V3` vs. `V4`)

Inventory tested: the 21 Omni rules in `01_understand.md` §3.3, forming 8 symmetric partitions and 8 directed edges (7 `Chain` / `Triggers`, 1 `Delegates`). All three prototypes were verified to derive the exact same canonical graph and `--explain` output, with no `Triggers` cycle:

```rust
// --- V1: Shared `Partition` constants + active-voice directed edges on the source rule ---
Decl {
    name: ids::CONCRETE_COLLECTION_PARAMETER,
    partitions: &[
        Partition::PARAMETER_ANNOTATION_LEVEL,
        Partition::CONCRETE_BY_POSITION,
    ],
    edges: &[
        Edge::triggers(ids::MUTABLE_COLLECTION_PARAMETER)
            .when("when the `Mutable` counterpart is chosen but the parameter is never mutated"),
        Edge::triggers(ids::SPECIFIC_COLLECTION_PARAMETER)
            .when("when the parameter is only iterated"),
    ],
}

// --- V3: Shared `Concept` intermediates; rules only state `flags` / `suggests` / `delegates` ---
Decl {
    name: ids::CONCRETE_COLLECTION_PARAMETER,
    flags: &[&Concept::PARAMETER_CONCRETE], // parents: PARAMETER_ANNOTATION, CONCRETE_ANNOTATION
    suggests: &[
        suggests(&Concept::PARAMETER_READ_ONLY).when("when the parameter is only iterated"),
        suggests(&Concept::PARAMETER_MUTABLE)
            .when("when the `Mutable` counterpart is chosen but the parameter is never mutated"),
    ],
    delegates: &[],
}

// --- V4: V1 with ranked partition memberships (lower rank triggers every higher rank) ---
Decl {
    name: ids::CONCRETE_COLLECTION_PARAMETER,
    memberships: &[
        ranked(Partition::PARAMETER_ANNOTATION_LEVEL, 0), // 0 -> 1 (mutable), 0 -> 2 (specific)
        member(Partition::CONCRETE_BY_POSITION),
    ],
    edges: &[],
}
```

| Variant | Shared constants | Entries in rule files | Mentions of another rule | Edges keeping their condition |
| :--- | ---: | ---: | ---: | ---: |
| **V1**: `Partition` constants, directed edges on the source rule | 8 partitions | 29 | 8 | 7 / 8 |
| **V3**: shared `Concept`s; rules list what they flag and suggest; relations derived | 32 concepts | 34 | 0 | 7 / 8 |
| **V4**: V1 with ranked partition memberships (a lower rank triggers every higher rank) | 8 partitions | 24 | 3 | 2 / 8 |

What the spike showed:

1. **N-ary symmetric groups as shared `Partition` constants (`V1`).** A `Partition { concern, condition }` constant, like `Topic`, acts as an intermediate node. Members reference the partition constant, never each other: 8 partitions require 0 references to sibling rules. Symmetry and transitivity hold by construction, and the shared concern and condition (`when both banned lists match`) are written once. A single rule can belong to orthogonal partition axes (`PARAMETER_ANNOTATION_LEVEL` and `CONCRETE_BY_POSITION`).
2. **Active-voice directed edges on the source rule (`V1`), and its hidden coupling.** Declaring a directed edge once on its source rule (`triggers`, `delegates_to`, `relies_on`, `subsumes`) and letting `--explain` derive the incoming view ("Triggered by …") avoids duplicate declarations. **Caveat:** whether `A triggers B` holds depends on *both* `A`'s suggestion and `B`'s detector. Tightening or relaxing `B`'s detector can create or remove `A triggers B` without touching `A`'s file. Only an automated cross-rule check (Option D or witness tests, §3) catches that drift.
3. **Why `V3` (`Concept` intermediates for everything) was not chosen now, and when to reconsider it.** `V3` eliminates all rule-to-rule references (`0` mentions of sibling rules) and automatically derives edges when a new rule flags an existing concept. However, on the current 21-rule inventory it needed $4\times$ more shared constants (32 `Concept`s), obscured which rules trigger which behind an indirection layer, used prose concept labels that only approximate detectors (`parameter annotated list` standing in for `list`/`dict`/`set`), and still required hand-written `flags` and `suggests` lists. **When to challenge this:** if directed edges multiply across many rule families, or if `flags`/`suggests` can be inferred from `Example` snippets rather than written by hand, a concept/capability layer may beat direct edges.
4. **Why `V4` (ranked partitions) failed.** Only 3 of the 8 partitions (collection abstraction levels) are ordered; sleep, naming, and suppression partitions are not. Worse, "lower rank triggers every higher rank" held in the collection ladder by coincidence (`list` happens to be concrete, mutable, and specific), not as a general property of ordered partitions; each rule file stated its own numeric rank (`0`, `1`, `2`), scattering the order across files; and deriving edges from ranks dropped 5 of the 7 per-edge conditions explaining *when* a chain occurs.

### 2.2 External rules (`Track A` vs. `Track B` vs. `Track C`)

```rust
// Track A: Omni-local links, using an inverse constructor for incoming external chains
Link::chained_from(ExternalRule::RUFF_SIM105)
    .when("after rewriting `try ... except: pass` to `contextlib.suppress(...)`")

// Track C: Omni-local links, subject-first active voice on `ExternalRule`
ExternalRule::RUFF_SIM105
    .triggers(ids::SUPPRESSED_EXCEPTION)
    .when("after rewriting `try ... except: pass` to `contextlib.suppress(...)`")
```

| | Track A: links in the Omni rule file, `chained_from` for incoming chains | Track B: every symmetric relation a shared class; external rules are nodes in a central table | Track C: like A, with edges written subject first (`RUFF_SIM105.triggers(SUPPRESSED_EXCEPTION)`) |
| :--- | :--- | :--- | :--- |
| All of a rule's relations in its own file (D1) | Yes | No | Yes |
| 1-to-1 external overlaps stay in the rule file | Yes | No: one shared constant plus an entry on each side | Yes |
| No inverse kind (`chained_from`) | No | Yes | Yes |

What the spike showed:

1. **Track B scatters a rule's relations.** `unstructured-task` subsumes Ruff `RUF006` and is triggered by `RUF006`'s fix. Track B writes the first in `unstructured_task.rs` and the second in a central `EXTERNAL_NODES` table. `suppressed_exception.rs` would show `Relations::NONE` while `SIM105 → suppressed-exception` sits in the central table. Every 1-to-1 external overlap (`error-log-in-except` with `TRY400` and `G201`) also becomes a global constant used by a single Omni rule.
2. **External rules as pure descriptors** (tool, code, name, URL). A `Partition` can include external rules (`mutable-module-constant` with `RUF012`), while relations involving one Omni rule stay in that rule's file.

## 3. Proving vs. Declaring Relations

A hand-declared relation goes stale when either rule's detector or suggestion changes. Any future design should minimize unverified declarations:

1. **Existential relations ($\exists x$): executable witnesses.**
   - `a triggers b` claims $\exists x \in V_a: f_a(x) \in V_b$. A witness pair $(x, f_a(x))$ — often `a`'s own `Example { flagged, fixed }` — lets `cargo test` run `a` on $x$ and `b` on $f_a(x)$ via `runner::lint_file`.
   - An Omni ↔ Omni `Overlaps` claims all three regions $V_a \cap V_b$, $V_a \setminus V_b$, and $V_b \setminus V_a$ are non-empty, which can be witnessed by three snippets. (With an external rule, only the Omni side can execute in `cargo test`.)
2. **Universal relations ($\forall x$): proof by construction.**
   - No finite set of examples proves $V_a \cap V_b = \emptyset$ (`Partitions`) or $V_b \subseteq V_a$ (`Subsumes`).
   - Instead, disjointness or inclusion must hold by construction from a single shared classifier: for instance, `sleep-in-tests` and `zero-sleep-in-tests` both branch on `has_zero_duration_argument` (`src/code_lint/rules/sleep_in_tests.rs:232`). Rules in separate files can share a classifier in `src/code_lint/policy.rs`. D6 (shared sleep deny list) and D9 (single owner per naming token) apply the same principle.

## 4. Why Both the Design and Rollout Are Deferred (D13)

Today's 41 registered rules (36 code rules, 4 suppression audits, 1 command rule) contain only 8 Omni ↔ Omni directed edges — 5 of them inside the collection family — and 0 Omni ↔ Omni overlaps. Locking in a relation schema and rolling it out across 41 files now would overfit both the types and the ergonomics to those 8 edges.

Upcoming work on the roadmap will expand the rule corpus and introduce new kinds of interactions: declaration ordering, `nested-class`, tuple-returning functions, third-party suppression hygiene, `printf-log-format`, the `command_lint` expansion, and the AST migration (`docs/dev/ast_robustness/`).

- **Independent fixes that need no relation schema (can land anytime):**
  - **D9:** give each naming token one owner (`str` in `type-suffixed-name`, `num` in `abbreviated-name`).
  - **D11:** update `repeated-index-access` doc examples to `start, end = span` / `let (start, end) = span;`.
- **How to approach this when resumed (challenge, do not just execute):**
  1. Re-inventory the expanded rule corpus first. Check what new relations (especially Omni ↔ Omni overlaps, multi-rule chains, or cross-domain `code_lint` ↔ `command_lint` / suppression relations) actually exist.
  2. Challenge the prototypes in §1–§3 against that fuller inventory:
     - Can Option D (the cross-rule example harness) and shared classifiers in `policy.rs` do most of the soundness work with a much smaller declarative surface?
     - Does `V1` (`Partition` + active-voice edges) still hold up, or do denser interactions justify `V3` (`Concept`s) or a different abstraction altogether?
     - Are central compile-time rule IDs in `src/diagnostic.rs` worth the maintenance cost compared to validated `RuleName` strings or a macro-generated enum?

## 5. Open Questions for the Next Iteration

- **Q1.** How should incoming external chains (T5: `SIM105 → suppressed-exception`, `RUF006 → unstructured-task`) be expressed — Track A's `chained_from`, Track C's subject-first edge, or something else?
- **Q2.** Should rule references use an exhaustive enum, associated constants on `RuleName`, or plain `RuleName(&'static str)` strings validated by a registry test? (An enum is exhaustive at compile time, but test-only rule names in `config.rs`, `rule_selection.rs`, `contract.rs`, and `diagnostic.rs` would need a separate test variant or type; plain strings require zero central-list churn if directed edges stay rare.)
- **Q3.** Which partition axes add real value in `--explain` ("by abstraction level" vs. "by position" on collection rules), and which are taxonomy noise best left to `Topic`?
- **Q4.** Can witness verification (§3) be unified with the Option D `Example.fixed` harness so that declaring an unconditional `triggers` edge needs no extra witness snippet beyond the rule's existing `Example`s?

