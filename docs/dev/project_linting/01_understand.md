# Project-Aware Linting — Phase 1: Understand

> Phase 1 (Understand) of the project-aware linting cycle: the problem model, critical user
> journeys, goals, decisions and open questions. Phase 2 (references and preliminary measurements) and Phase 3
> (design) follow in this directory. Background: the `CodeLintProject` extraction and the
> roadmap items *Bridging `CodeLintProject` (`ProjectFacts`) and Declared Architecture into
> `omnicoach code`* and *Rules needing a project-wide symbol or call index*
> ([ROADMAP.md](../../../ROADMAP.md)).

## TL;DR

- **Problem.** `omnicoach code` rules see one file at a time. Stage F extracted the shared
  project-layout helpers into a `CodeLintProject` component and prototyped cross-file linting on
  top of `ProjectFacts`, shaped around a single rule, `uninherited-protocol`. Checked against the cross-file rules
  we plan next, the prototype has structural gaps: rule-specific data in the shared project
  model, no project context in the agent hook, graph-grade extraction where lint rules need much
  less, cross-file tests that cannot live in the rule file, and a project context that changes
  with the paths passed on the command line.
- **Goal of this cycle.** Design from first principles the project model that cross-file rules
  query, and validate it against three different consumers before committing to it:
  architecture conformance, cross-file `Protocol` inheritance and cross-file docstring
  inheritance.
- **Framing.** Cross-file rules need three kinds of facts: what a name refers to
  (*resolution*), which component owns a module and what it may depend on (*structure*), and
  who uses a definition (*usage*). The three consumers of this cycle need only resolution and
  structure facts, and those depend on a bounded set of files per linted file. Usage facts need
  the whole project and stay out of scope, without being designed out.
- **Decisions so far.** Stage F code may be reworked freely. Agent-hook support is
  investigated and kept only if it stays simple and fast, otherwise it becomes a roadmap item.
  The architecture model is language-agnostic, and Python declarations are prototyped.
  Three consumer families validate the design.
- **Next.** Phase 2 surveys how established tools solved these problems and answers the cost
  questions with preliminary measurements.

## 1. Context: Stage F and Why We Restart From First Principles

Stage F kept one change and prototyped the rest:

- **Kept: `CodeLintProject`** ([project.rs](../../../src/code_lint/project.rs)):
  the Rust module-path and component inheritance helpers shared between `CodeLintGraph`
  ([facts.rs](../../../src/code_lint/graph/facts.rs)) and
  [architecture_conformance.rs](../../../tests/architecture_conformance.rs).
- **Prototyped, not merged:** a `ProjectCodeRule` type whose check also received the project
  facts, with findings still reported through the per-file pipeline, so suppressions,
  explanation comments and `--diff` filtering applied unchanged; Python class and
  imported-`Protocol` facts added to `ProjectFacts`; and a cross-file version of
  `uninherited-protocol` that matched classes against `Protocol`s imported from other project
  modules. Phase 2 ([02_references.md](02_references.md)) measured and audited the prototype,
  and it was dropped. This cycle rebuilds cross-file `uninherited-protocol` on its own design.

Stage F went from "one rule needs cross-file facts" straight to an implementation, without first
asking what the next cross-file rules need. Checked against those rules, the prototype has six
gaps:

| Gap | Where | Why it matters |
| :--- | :--- | :--- |
| **Rule-specific data in the shared model.** The prototype added `python_classes`, `python_imported_protocols` and `python_resolved_bases` to `ProjectFacts`, precomputed for `uninherited-protocol`. | The prototype's `ProjectFacts` | Each new cross-file rule would add its own maps, so the model grows with the number of rules instead of answering a few general questions. |
| **No project context in the agent hook.** `omnicoach hook` lints the edited file with `lint_file`, which passes no project. | [hook/runner.rs](../../../src/hook/runner.rs) | Cross-file rules silently fall back to single-file behavior where agents edit code. (In the prototype, `omnicoach code`, including `--diff`, did build project context.) |
| **Graph-grade extraction for lint rules.** Building `ProjectFacts` computes per-scope declarations and intra-file references for every file. `omnicoach graph` needs them; the lint rules do not. | `collect_project_facts_from_sources` in [facts.rs](../../../src/code_lint/graph/facts.rs) | Unmeasured cost on large projects and in the hook. |
| **Cross-file tests outside the rule file.** `rule_test!` cases and documented `Example`s hold a single snippet, so the prototype's cross-file cases of `uninherited-protocol` lived in the runner's tests. | [test_utils.rs](../../../src/test_utils.rs) | Breaks the convention, checked by [tests/registry.rs](../../../tests/registry.rs), that a rule file owns all its cases and examples; the documented example cannot show the cross-file behavior. |
| **Heuristic Python import matching.** The prototype matched an import that did not match a discovered module path exactly by a unique suffix. | The prototype's Python resolution | Whether an import resolves depends on which files happen to be discovered. |
| **Project context depends on the command line.** Project facts are built from the paths passed to the command. | `collect_project_source_files` in [facts.rs](../../../src/code_lint/graph/facts.rs) and the prototype's runner | Linting `pkg/sub/` alone does not see `pkg/interfaces.py`, so the verdict on a file changes with the command line. |

**Interpretation.** The prototype's per-file reporting is sound and worth keeping (decision D5
below). What needs rethinking is the project model itself: what it contains, how it is built,
where its boundary comes from, and how rules and tests reach it.

## 2. Problem Model: What Cross-File Rules Need

A lint verdict on a file is a function of facts. Single-file rules only need the file's syntax.
Cross-file rules need facts about other files, of three kinds:

| Kind of fact | Question it answers | Example |
| :--- | :--- | :--- |
| **Resolution** (follows references forward) | What does this name refer to, and what does that definition look like? | `Store` in `service.py` is the `Protocol` `pkg.interfaces.Store`, with methods `load` and `save`. |
| **Structure** (project organization) | Which component owns this module? What may it depend on? What is its public surface? | `code_lint::rules::foo` belongs to `CodeLintRules`, which may not depend on `CodeLintRunner`. |
| **Usage** (follows references backward) | Who uses this definition elsewhere? | `Store.load` is never called anywhere in the project. |

The consumers we know of need:

| Consumer | Resolution | Structure | Usage |
| :--- | :--- | :--- | :--- |
| **Architecture conformance** (component dependency rules, sibling isolation, relative-path boundaries) | Module level: which module a path points to | Core | No |
| **Cross-file `Protocol` inheritance** (`uninherited-protocol`) | Class level: methods, bases, `Protocol` marker | No | No |
| **Cross-file docstring inheritance** (`copied-base-docstring`, `partial-override-docstring`, `deferral-docstring`) | Class and method level: method docstrings in ancestor order | No | No |
| Later: unused interface methods, unused exports, mutation of returned collections | Yes | Sometimes | Core |
| `omnicoach graph` | Yes, plus intra-file references | Yes | Yes |

**Analysis.** For resolution and structure facts, the verdict on a file depends only on a
bounded set of other files: the modules its references point to (and, through re-exports and
base classes, the modules those point to), the modules above it in the module tree that declare
its component, and the architecture declaration. We call this the file's **context set**: the
files whose content can change the verdict on that file. Usage facts have no such bound: any file
in the project can add a use.

**Interpretation.** The three consumers of this cycle have bounded context sets. That makes a
cheap agent-hook mode plausible, because the hook could load the context set of the edited file
instead of the whole project. Whether it is actually cheap enough is what Phase 2 measures. Rules
that need usage facts require a whole-project pass and are left for a later cycle, but the
design must leave room for them.

## 3. Critical User Journeys

| Journey | What the user does | Acceptance criteria |
| :--- | :--- | :--- |
| **Lint from the command line** | Runs `omnicoach code [PATHS...]` on a project or part of it. | Cross-file rules report on the given paths. A file's findings do not depend on which other paths were passed: the targets decide *what is reported*, the project decides *what is known*. |
| **Lint a diff** | Runs `omnicoach code --diff`. | Same findings as a full run, restricted to changed lines. |
| **Get coaching in the agent hook** | An agent proposes an edit; `omnicoach hook` evaluates the edited file in memory. | Cross-file findings on modified lines are reported, computed against the on-disk project with the edit applied, within the hook latency budget (assumption A3). If that is not achievable simply, the gap is documented and recorded on the roadmap. |
| **Write a cross-file rule and its tests** | A contributor adds a rule that needs facts from other files. | The rule queries general project facts and cannot bypass suppressions, explanation comments, target filtering or `--diff` filtering. Its multi-file test cases and documented examples live in the rule file, with the same checks as single-file cases. |
| **Declare an architecture** | A Rust project declares components in code (`define_architecture!`, `architecture_component!`); a Python project declares them in a form to be designed. | `omnicoach code` reports a forbidden dependency at the offending import or path, naming both components and the allowed direction. |
| **Query the graph** | Runs `omnicoach graph` or a query subcommand. | Graph and lint rules agree on what a name refers to, because they share one resolver. Graph output does not change unless a resolution bug is fixed. |

## 4. Goals and Non-Goals

### Goals

1. **General project questions instead of rule-specific caches.** Rules ask a small set of
   general questions (resolve a name, describe a definition, find the component of a module,
   check whether a dependency is allowed). *Reason:* three or more consumer families must share
   the model without each adding its own maps.
2. **Verdicts independent of the command line.** The project boundary comes from the project (its
   root and source roots), not from the paths being linted. *Reason:* the same file must get the
   same findings in a full run, a diff run, a path-restricted run and the hook.
3. **Pay only for what is used.** No project work when no cross-file rule is enabled for a
   language, and project facts no richer than enabled rules need. *Reason:* pre-commit and hook
   latency.
4. **A pit-of-success rule contract.** Cross-file rules plug into the same pipeline as other
   rules, and their tests and examples live in the rule file. *Reason:* Stage F moved cross-file
   tests out of the rule file, and future rules must not repeat that.
5. **Architecture conformance as native diagnostics, with a language-agnostic component model.**
   *Reason:* `tests/architecture_conformance.rs` protects only omnicoach itself, and Python
   projects currently need external tools for the same guarantees.
6. **One resolver for lint rules and the graph.** *Reason:* two resolvers drift and disagree.

### Non-Goals

1. **Type inference and receiver-type resolution** (`x.method()` on a local variable). *Reason:*
   the three consumers need names and class headers only; receiver types are a separate roadmap
   item.
2. **Caches that persist across runs, daemons and editor integration.** *Reason:* `omnicoach code`
   and `omnicoach hook` are one-shot processes. If a measured budget is exceeded, the first answer
   is a cheaper model or a loud limit, not a second caching mode.
3. **Resolution across Cargo workspace crates and into third-party packages.** *Reason:* Cargo
   and Python packaging already own those boundaries; names from outside the project stay
   opaque.
4. **Rules that need usage facts** (unused interface methods, unused exports). *Reason:* they need
   a whole-project reverse index and often type information. The design must not preclude them.
5. **Changing `omnicoach graph` output.** *Reason:* only the source of its facts may change; its
   output is verified unchanged.

## 5. Decisions

| ID | Decision | Reason |
| :--- | :--- | :--- |
| **D1: Refactor freely** | Stage F code, prototype and kept parts alike, may be reworked or dropped as far as a robust, idiomatic, pit-of-success design requires. Applied after Phase 2: the cross-file prototype was dropped; `CodeLintProject` is kept. | Stage F was shaped around one rule; keeping its shape would carry its gaps into every later rule. |
| **D2: Hook support is investigated, not required** | Evaluating cross-file rules in `omnicoach hook` is a goal only if it stays simple and within the hook latency budget. Otherwise it is documented as a known gap and recorded on the roadmap. | Agent coaching is where cross-file rules help most, but not at the cost of hook latency or a second code path. |
| **D3: Language-agnostic architecture model** | The component model (components, module ownership, allowed dependencies) does not depend on Rust. Python declarations are investigated and prototyped in this cycle. | Python projects need the same guarantees; a Rust-only model would have to be redone. |
| **D4: Three consumer families validate the design** | Prototypes must serve architecture conformance, cross-file `Protocol` inheritance and cross-file docstring inheritance with the same project model. | One consumer cannot tell a general design from one overfitted to it. |
| **D5: Per-file reporting is kept** (from the Stage F prototype) | Every cross-file finding is reported in one file at one source location, through the per-file pipeline (target filtering, suppressions, explanation comments, `--diff` filtering). | Validated by the prototype; a separate whole-project rule type would duplicate that pipeline. |

## 6. Open Questions

| ID | Question | Answered in |
| :--- | :--- | :--- |
| **Q1: Python source roots** | How does omnicoach find the directories Python imports resolve against (project root, `src/` layout, configured roots, namespace packages without `__init__.py`)? | Phase 2 references; Python import resolution measurement |
| **Q2: Project boundary** | What bounds the project context: the directory of `.omnicoach.toml`, the VCS root, or the nearest `Cargo.toml` / `pyproject.toml`? What happens without any of them? | Phase 2 references |
| **Q3: Eager versus on-demand loading** | Should a run summarize every project file up front, or load only the context sets of the linted files? Which is simpler at equal latency in the command line and in the hook? | Context set measurement |
| **Q4: What a per-file summary holds** | Which facts per module serve all three consumers (imports, re-exports, component declarations, class headers with methods and docstrings)? Can `omnicoach graph` build on the same summary? | Phase 3 prototypes |
| **Q5: Hook overlay** | Can the hook evaluate cross-file rules on the edited file, overlaid on the on-disk project, within budget? An edit can also change findings in *other* files (adding a method to a `Protocol`); is reporting only on the edited file acceptable? | Context set measurement; Phase 3 |
| **Q6: Declaring a Python architecture** | Configuration (`.omnicoach.toml`), in-file markers, or both? How are layers, independent siblings and public interfaces expressed? | Phase 2 references; Phase 3 prototype |
| **Q7: Multi-file test format** | How do `rule_test!` cases and documented `Example`s describe several files, assert where a finding is reported, and keep the "code repeated twice" check meaningful? | Phase 2 references; Phase 3 prototype |
| **Q8: Generic rules versus omnicoach-only tests** | Which checks of `tests/architecture_conformance.rs` become generic rules, and which stay omnicoach-specific tests (parser crate encapsulation, component naming, every component having a root file)? | Phase 3 |
| **Q9: Rust declarations outside omnicoach** | `define_architecture!` is a macro local to omnicoach. Should other Rust projects use it (through the roadmap's standalone crate) or declare components in configuration like Python projects? | Phase 3, with Q6 |

## 7. Assumptions to Validate

| ID | Assumption | Why it matters | How it is validated |
| :--- | :--- | :--- | :--- |
| **A1** | A per-file summary for lint rules (imports, re-exports, top-level definitions, class headers) costs much less than the graph's per-file facts. | Decides whether lint and graph should share one summary or use two levels. | Summary cost measurement on omnicoach (169 Rust files) and CPython `Lib` (1,870 Python files). |
| **A2** | The context set of a typical edited file is small: tens of modules, not the whole project. | Decides whether on-demand loading pays off, especially in the hook. | Context set measurement on the same corpora. |
| **A3** | Cross-file context may add at most a few tens of milliseconds to a hook file-edit evaluation. An earlier, not yet merged, measurement of the hook put a file edit at about 12 ms for a small file and 68 ms for an 880-line file (95th percentile about 105 ms). | Decides whether D2 can be met. | Re-measured on this branch in the context set measurement; the budget is confirmed with the user. |
| **A4** | Downstream Python projects have hundreds to about a thousand files; CPython `Lib` serves as a large-project stress case. | Sizes the latency budgets. | Checked against a real downstream project when one is available. |

## 8. Next Step: Phase 2 Plan

**References.** Three surveys, each recording key ideas, differences from omnicoach and what to
borrow:

1. **Multi-file analysis engines:** how rust-analyzer, Ruff's `ty`, Pyright, Biome, oxc,
   `eslint-plugin-import` and Pylint summarize files, resolve imports, bound the project and keep
   single-file rules cheap.
2. **Architecture conformance tools:** how import-linter, tach, dependency-cruiser,
   `eslint-plugin-boundaries`, Nx module boundaries, Sheriff, ArchUnit, Deptrac, `cargo-pup`,
   `zoning`, Bazel visibility and Go `internal` packages declare components and dependency rules,
   for which languages, and with which diagnostics.
3. **Multi-file test formats:** how rust-analyzer fixtures, `ty`'s Markdown tests and the
   fixture directories of oxc and Biome describe several files in one test.

**Preliminary measurements** (benchmark scripts, deleted in the clean-up phase):

1. **Summary cost measurement** (A1): time and allocations to build the graph's per-file facts versus a
   lint-oriented summary, per file and per project.
2. **Context set measurement** (A2, A3, Q3, Q5): sizes of context sets for real files, and the cost of
   loading only them versus the whole project, in the command-line and hook scenarios.
3. **Python import resolution measurement** (Q1): how often the current suffix matching and a
   source-root-based resolver disagree on CPython `Lib`.
