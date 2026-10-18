# Rule Batch Playbook

> [!NOTE]
> How-to for the agent (or person) who leads the creation of several rules at once with one worker agent per rule, standing in for the user during reviews. A single rule follows [adding_a_rule.md](adding_a_rule.md); this playbook only adds what changes when rules are built in parallel. Lessons come from the legacy custom lints batch (10 candidates, 6 shipped, 4 dropped).

## Roles

- **Lead**: owns scope, shared files, decisions, integration and the final review. Acts as the user for workers, but escalates to the real user any decision that changes scope, drops a rule, or touches the framework.
- **Worker**: one per rule. Runs the 7 phases (`01_understand.md` … `07_learn.md`) in `docs/dev/<slug>/` and implements the rule.
- **Reviewers**: two independent, read-only agents per batch: one for code and tests, one for user-facing text and docs. They review the batch as a whole, not rule by rule.

## Phases

### 0. Triage (lead, before spawning workers)

1. **Redundancy gate.** For each candidate, list the Ruff / Clippy / Mypy / Pyright / Pylint rules that already cover it. In the legacy custom lints batch, 4 of 10 candidates and 3 sub-checks were fully covered. A dropped candidate still gets `01_understand.md` and `02_references.md`, ending with the exact `pyproject.toml` / `Cargo.toml` configuration that replaces it, plus an entry in [not_pursued_rules.md](not_pursued_rules.md) linking that file.
2. **Language scope.** Decide per rule which languages apply, and record why the others do not ([rule_design_guide.md](rule_design_guide.md)). Workers ported Python rules and never asked; the Rust question came up only after shipping.
3. **Shared-helper inventory.** Before workers start, list the structural facts several candidates will need (logger-call detection, string literal text, format-placeholder parsing, local bindings, union flattening, collection type names) and grep `src/code_lint/ast/` and `src/code_lint/semantic/` for existing helpers. Assign each new shared fact to one owner (the lead or one worker). Without this step, parallel workers each wrote their own logger matcher, format-field parser and literal extractor in `ast/python.rs`.

### 1–3. Understand, references, design (workers, in parallel)

- Give each worker: the rule slug, the candidate source, the 7-phase list, the shared-helper inventory with owners, and the canonical file to copy conventions from (a recent rule such as `nullable_collection_return.rs`). Workers otherwise copy whichever neighbour they open, and inconsistencies spread (module doc comments, test naming).
- Review each `01`–`03` as the user would: check numbered decisions, push back on scope creep, and require every exemption to be named (`E1`, `E2`, …) so it can be mutation-tested later.
- Record the lead's validation in the document itself (status banner `VALIDATED` / `VALIDATED — DROPPED`). Workers leave banners at `PENDING`; fix them before closing the batch.

### 4. Execute (workers, integration serialized)

- Workers may write their rule file in parallel, but **shared files are integrated one rule at a time by the lead**: `ast/python.rs`, `rules.rs`, `taxonomy.rs`, `tests/snapshots/cli__list_rules.snap`, [ROADMAP.md](../../ROADMAP.md).
- After each integration run the full verification from [adding_a_rule.md](adding_a_rule.md) step 9, including self-dogfooding (`omnicoach code .`), which caught naming violations in the new helpers.
- **Mutation check per exemption.** Give each named exemption one dedicated `pass` case, add an entry for it to [scripts/exemption_mutations.py](../../scripts/exemption_mutations.py) (a source edit that disables the exemption, plus the case), and run the script. It also fails when a listed case no longer exists or a `pass` case of a covered rule is unlisted. Do not keep the harness in scratch space: the legacy batch harness was lost, and the declaration-order batch later found 7 cases that still passed with their exemption removed.
- Know the harness: `rule_test!` also runs every `fail` case as `{code}\n{code}` in one module and expects exactly the two copies' spans. The second copy redefines every top-level name, so a rule that reasons about the other definitions in a scope (declaration order, redefinitions, same-name grouping) must plan for it in Phase 3. `call-before-definition` bent its semantics to pass (epochs), and a simpler same-name merge had to be reverted. Open design problem: "Rule test harness" in [ROADMAP.md](../../ROADMAP.md).

### 5. Consolidate (lead, mandatory holistic synthesis)

Parallel workers design and implement each rule from that rule's perspective alone. Left as stitched-together worker outputs, a batch bloats the framework with narrow helpers and leaves cross-rule plumbing inside rule files. Before review, the lead must re-architect the shared `ast/`/`semantic/`/`command_lint/` layers and the rules together so the result is as cohesive, idiomatic and minimal as if all rules had been designed as one system—improving the framework's abstractions at the end of the batch rather than accreting around them:

1. **Audit both the shared layers and `rules/` across the batch.** Look beyond duplicate free functions in `ast/`: inspect the rule files for shared domain concepts implemented locally, ad-hoc `SourceSpan` join tables that stitch together separately extracted AST items because a structural link was missing, and partition predicates duplicated between sibling rules.
2. **Elevate shared domain concepts onto the core types.** Express each domain concept once on the shared AST, semantic, or command types (whether cross-language abstractions like `CallableItem` / `TypeMethod` or language-specific nodes across Rust, Python, Markdown, and shell commands) so every rule's check is a direct, readable composition of domain predicates with no local plumbing. When two workers implemented the same concept with slightly different edge cases, unify them on the stronger semantics.
3. **Refactor and simplify pre-existing abstractions.** Use the batch's broader view to replace narrow one-off flags on existing types with the underlying structural relationship from which both old and new queries derive, and split modules that grew past readability.
4. **Sync and re-run `scripts/exemption_mutations.py`.** When an exemption moves from rule files into a shared helper, point its mutation at the shared helper (covering all rules that rely on it) and re-run the script to verify that no exemption was weakened or orphaned.

### 6. Review (two reviewers, whole batch)

- Ask reviewers explicitly for cross-rule findings: duplicated helpers, inconsistent conventions, overlapping diagnostics between the new rules and existing ones.
- Split the code review by structural cluster (strings and logging, scopes and bindings, types and classes), not by rule. In the legacy custom lints batch, per-rule reviews and the 23/23 mutation check missed 11 behavior bugs that cluster reviewers then found.
- Read-only reviewers cannot run code. Run each repro they claim against the built binary before acting on it.
- Apply findings, then record each finding and its resolution in every affected `06_review_and_audit.md`.

### 7. Learn (workers draft, lead edits)

Each `07_learn.md` keeps only reusable lessons. Batch-level lessons go into this playbook. Before closing `06`/`07`, grep every backticked identifier against `src/` and recount the cases: docs drafted from worker reports named about 25 helpers and tests that did not exist, and the declaration-order batch repeated it. Cite test cases by name without the generated `case_N_` prefix, which shifts when cases are added, and do not copy the exemption-to-case table into docs: `scripts/exemption_mutations.py` is that table and checks it.

## Tooling constraints

- Repository Markdown: no `file://` links and no backticks around link text (the pre-write hook rejects both); use relative paths.
- Long batches outlive the context window. Keep one status table per batch (rule, phase, blocking question) so a resumed lead does not depend on memory.
