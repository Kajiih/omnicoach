# Project-Aware Linting — Phase 2: References and Preliminary Measurements

> Phase 2 (Gather Resources and Reference) of the project-aware linting cycle. It answers the
> assumptions and open questions of [01_understand.md](01_understand.md) with three surveys of
> established tools, an audit of the Stage F implementation and four preliminary measurements, and
> ends with the competing directions that Phase 3 prototypes.

## TL;DR

- **Established tools share one shape.** They keep a small per-file summary of what a module
  exposes, separate from function bodies; resolve imports on demand against the project's
  source roots, never against the files the user passed; read files through one abstraction
  that can overlay unsaved text; and report diagnostics only on the requested files.
  Incremental query databases (salsa) pay off only in long-lived processes such as editors.
- **Architecture tools declare components in configuration.** When allowed dependencies follow
  declared edges rather than import chains in the code, almost every conformance check needs
  only the linted file plus the declarations.
- **The strongest multi-file test harnesses keep each case's files inline** and check every
  file, failing on any unexpected diagnostic.
- **Measurements.** The Stage F prototype made a full run on a large Python project about five times
  slower and doubled its memory. The files a verdict actually depends on are few, and loading
  only them costs a few milliseconds. Taking the project context from the command line loses
  about half of a package's cross-module facts, and suffix matching resolves imports to the wrong
  module.
- **Audit.** The Stage F prototype and the project code it built on have fourteen issues, eight
  of them bugs, among them module paths that depend on how a path is written, merged same-named
  classes, and false `unused-suppression` reports. Dropping the prototype removes four of the
  bugs; the others sit in the kept discovery code and in the docstring rules' ancestor order.
- **Decision.** The cross-file part of Stage F was dropped and `CodeLintProject` kept (section
  8). Cross-file `uninherited-protocol` stays in scope and is rebuilt on the new design.
- **Next.** Phase 3 prototypes four competing design tracks: how the project model is loaded,
  how architectures are declared, the rule contract, and multi-file tests.

## 1. How Established Tools Analyze Several Files

Surveyed: rust-analyzer, Ruff's type checker `ty`, Pyright, Biome 2, oxc (`oxlint`),
`eslint-plugin-import`, Pylint, the Ruff linter, Go's analysis framework and TypeScript's
`isolatedDeclarations`.

| Tool | What a dependency is known by | How imports resolve | Loading | Unsaved text |
| :--- | :--- | :--- | :--- | :--- |
| rust-analyzer | `ItemTree`: a file's items without bodies | Crate graph from Cargo, then the `mod` tree | Lazy, memoized queries | Virtual file system |
| `ty` | A per-file semantic index (scopes, symbols) | Project root plus `src/` and similar first-party roots, then installed packages | Lazy, memoized queries | File-system layer serves open documents first |
| Pyright | Binder scopes and symbols, no type checking | Execution root, `extraPaths`, `src/`, installed packages, stubs | Pulls in imported files; type-checks only requested ones | Open-file contents |
| Biome 2 | Module info: imports, exports, re-exports | Configured files plus their reachable dependencies | Eager scan, bounded by reachability | Workspace API |
| oxlint | ECMAScript module record (imports, exports) | Node resolution and `tsconfig` paths | Follows imports of the linted files, in parallel | Runtime file-system trait |
| Pylint | Full inference trees | `sys.path`, `source-roots` | Lazy, cached per process | Linted file only |

**Patterns.**

1. **Summaries, not bodies.** Every design that scales knows a dependency only by a small
   summary of what it exposes. Editing a function body leaves the summary unchanged.
2. **Resolve against the project, report on the request.** Imported files are loaded even when
   not requested, but diagnostics are reported only on requested files. Pylint's
   `cyclic-import`, which aggregates over the files passed, is the counter-example: its
   findings change with the command line.
3. **Work bounded by reachability.** Lazy queries, graphs that grow outward from the linted
   files, or reachable-only indexing. Only usage facts, such as unused exports, need a full
   scan.
4. **Explicit opt-in.** Biome's project rule domain and oxlint's import plugin make the cost zero
   when no multi-file rule is enabled.
5. **The overlay is a file-system abstraction** covering reads, existence checks and directory
   listings, not only file contents.

**Interpretation for omnicoach.**

- **Borrow:**
  - a body-free module summary extracted from the parse;
  - on-demand resolution memoized for one run, with deterministic limits (files loaded,
    re-export depth) that yield "unknown", and rules that report nothing on "unknown";
  - a project boundary derived from marker files and configuration;
  - one file source with an overlay map, which serves the agent hook, tests and the command
    line alike;
  - import edges recorded with the imported names and deterministic summaries, so usage facts
    and caching remain possible later.
- **Do not borrow:**
  - salsa or any revision-based query database: each query runs once in a one-shot process, so
    a map of `OnceLock`s gives the same deduplication without pushing lifetimes and interning
    into the rule API;
  - type inference, macro expansion, build scripts or interpreter probing;
  - an eager whole-project scan in the hook, whose cost grows with the project instead of with
    the edited file;
  - aggregating results at the end of the run;
  - time-based caches, which only long-lived processes need.

## 2. How Architecture Conformance Tools Work

Surveyed: import-linter, tach, PyTestArch, Deply (Python); dependency-cruiser,
`eslint-plugin-boundaries`, Nx module boundaries, Sheriff (TypeScript); ArchUnit, Spring
Modulith, Deptrac, Konsist (JVM, PHP, Kotlin); `cargo-pup`, `rust_arkitect`, `arch_test`,
zoning (Rust and polyglot); Bazel visibility, Pants dependency rules, Go `internal/` packages,
`go-arch-lint` and depguard.

**Terminology.** In the reflexion model (Murphy, Notkin and Sullivan), a *high-level model*
lists components and allowed edges, a *source model* holds the dependencies found in code, and a
*mapping* assigns code to components. A dependency present in code but not allowed is a
*divergence*. In omnicoach, `define_architecture!` is the high-level model,
`architecture_component!` with subtree inheritance is the mapping, and every check reports
divergences.

| omnicoach check | Equivalents elsewhere | How common |
| :--- | :--- | :--- |
| Transitive reachability over declared dependencies | tach, Deptrac, Nx, Spring Modulith, `go-arch-lint` | Common. Acyclicity by declaration order is unusual; other tools detect cycles in code. |
| Sibling subtree isolation | import-linter `independence`, ArchUnit slices, dependency-cruiser groups | Common as an opt-in per group; applying it by default to every component root is unique to omnicoach. |
| Relative paths stay inside the component | Nx, zoning reach limits | Rare. |
| Single public path through private-child re-exports | Consumer-side interfaces: tach, Sheriff, Spring Modulith, Go `internal/` | Unusual: omnicoach constrains the provider. Python has no privacy, so it also needs a consumer-side "import only through the component root" rule. |
| External package encapsulation | import-linter `protected`, Nx, depguard, zoning | Common. |
| Every component has a root; unmapped files are pure routers | tach, import-linter `exhaustive`, Deptrac "uncovered" | Coverage checks are common; the pure-router constraint is unusual. |

**Analysis.** Declaration models split into configuration files (import-linter, tach,
dependency-cruiser, Deptrac, zoning, `go-arch-lint`) and in-code annotations (ArchUnit and Spring
Modulith package annotations, Deptrac's internal tag). No surveyed Rust tool declares
architecture through macros. Tools that follow import chains transitively (import-linter) need
the whole import graph and report far from the cause; Pants deliberately checks direct
dependencies only, because transitive checking "would otherwise make it impossible to use private
modules". Mature tools make stale exceptions fail (import-linter, Deptrac, zoning).

**Interpretation for omnicoach.**

- **A language-agnostic component model** fits in an ordered list of components, each with a
  name, a root module path (`crate::code_lint::ast` or `pkg.code_lint.ast`), the earlier
  components it depends on, and optionally the external packages it owns. A module belongs to
  the component whose root is its longest prefix. Language-specific parts stay small: module
  path derivation, import extraction, first-party classification and the privacy convention.
- **With closure over declared edges, the conformance checks become per-file:** each needs only
  the linted file, the declarations and module path resolution. Validating the declarations
  themselves needs to know which files exist, not their content. A real import graph is needed
  only for optional extras such as code-level cycles or unused declared dependencies.
- **Configuration as the canonical declaration** (`[architecture]` in `.omnicoach.toml`), with
  in-file markers accepted only as checked annotations, is the recommendation for Python and for
  Rust projects other than omnicoach. omnicoach's macros could become an optional crate,
  recognized by name, with both forms rejected in one project.
- **Exceptions reuse `omni:ignore`**, so `missing-suppression-reason` and `unused-suppression`
  keep them honest; no architecture-specific ignore list.

## 3. How Linters Test Multi-File Rules

Surveyed: rust-analyzer fixtures, `ty` Markdown tests, Biome spec tests and executable rule docs,
oxc's `Tester`, ESLint and typescript-eslint `RuleTester`, Pylint functional tests and message
docs, Ruff fixtures, rustc and Clippy UI tests, Semgrep, Pyright fourslash, mypy, Pyrefly, Go
`analysistest` and Error Prone.

| Approach | Examples | Checks every file | Readability |
| :--- | :--- | :--- | :--- |
| Files inline in the case | rust-analyzer, `ty`, Pyright, mypy, Pyrefly, Error Prone | Yes | Each case reads on its own |
| Shared on-disk fixture trees | oxc, typescript-eslint, `eslint-plugin-import` | Target file only | Context hidden from the reader |
| Executable multi-file docs | Biome (`file=` blocks), Pylint (`bad/` directories) | Varies | Proven for user-facing examples |

**Interpretation.** Inline per-case files with strict checking of every file are the
state of the art. Pylint's expectation key, line and rule without the file, shows why a
multi-file assertion must name the file. Three syntaxes fit `rule_test!`:

- **Inline file map per case** (recommended): a case becomes `name => { "pkg/interfaces.py" =>
  r#"..."#, "pkg/service.py" => r#"..."# => r#"MemoryStore"# }`. The macro checks the structure;
  existing single-snippet cases keep their syntax.
- **Headers inside one string** (`#- pkg/service.py flagged`): no macro change, but a second
  mini-language validated only when tests run.
- **Named shared contexts plus a target snippet**: the smallest grammar change, but a reader has
  to look up the context, and findings in context files cannot be expressed.

**Shared semantics.** All files are linted. A failing case expects exactly one diagnostic,
in the flagged file, at the expected slice; a passing case expects none in any file. The
"code repeated twice" check repeats only the flagged file's code, so it keeps catching rules
that stop at their first match. A documented `Example` gains context files, rendered as titled
code blocks by `--explain`, and is written only for rules whose behavior one file cannot show.

## 4. Audit of the Stage F Prototype and the Kept Project Code

A read-only audit traced every consumer of `ProjectFacts` and every check of
[tests/architecture_conformance.rs](../../../tests/architecture_conformance.rs), on the branch
that still contained the cross-file prototype. The issues that matter most:

| Issue | Rating | Where |
| :--- | :--- | :--- |
| **Module paths depend on how the path is written.** A relative top-level package path stops the climb above the package, so `pkg.interfaces` becomes `interfaces` and absolute imports stop resolving; an absolute Rust path loses its crate-relative prefix. | Bug | Prototype discovery and kept `normalize_relative_path` |
| **Discovery depends on the command line, asymmetrically.** File targets expand to their package, directory targets do not; a loose file expands to its whole parent directory; N files of one package are walked N times. | Bug | Prototype discovery; kept discovery walks only the given paths |
| **Two files with the same relative path** under different roots are deduplicated, and one is silently dropped. | Bug | Kept discovery |
| **Suffix matching** binds an unresolvable import, or a linted file missing from the facts, to an unrelated module. | Bug | Prototype (dropped) |
| **Same-named classes in one module are merged**, including classes nested in functions, so a test-local class can make a non-`Protocol` look like a `Protocol` to importing files. | Bug | Prototype (dropped) |
| **Ancestor order is depth-first, not Python's method resolution order**, so diamonds pick a different inherited docstring than `inspect.getdoc`. | Bug (narrow) | Kept docstring rules |
| **A cross-file rule evaluated without project facts still counts as evaluated**, so a suppression written for a cross-file finding is reported by `unused-suppression` in the hook. | Bug (user-visible) | Prototype (dropped); the redesign must not reintroduce it |
| **Lint and graph discover different file sets** (lint includes tests, the graph excludes them), so the "one resolver" goal does not hold. | Design smell | Prototype (dropped) |
| Star imports, aliases and symbols defined under `if`/`try` are not re-exported; the 4-hop limit truncates silently. | Limitation | Kept facts and prototype |
| Linear scans per lookup, a sequential build, and every linted file parsed twice. | Design smell | Kept facts (sequential build) and prototype |

**Obstacles to a shared module summary.** Class and function views borrow the syntax tree
(`PythonClassInfo<'a>`), so a summary must project them to owned data. Docstring comparison
logic lives on a type that cannot be built from owned text. Extracted classes carry no scope
information, which caused the merge bug above. ADR 006 places constraints on location: summary
builders must live in `code_lint::ast` (the only component allowed to touch parser crates), a
language-neutral summary type in its root, and a class-hierarchy resolver shared by four rules in
`code_lint::project`.

**Interpretation.** Most bugs share two causes: module identity derived from discovery instead
of from source roots, and class facts that are unordered, merged and scope-blind. A redesign
that fixes those two causes removes most of the list.

## 5. Preliminary Measurements

**Method.** Benchmark harness run in a separate change, deleted in the clean-up phase. Release build;
"sequential" is one thread, "parallel" uses all 16 hardware threads. Corpora: omnicoach `src/`
(Rust), Cargo's `src/` (Rust) and CPython `Lib/` (Python, about 1,900 files).

### 5.1 Summary Cost

| Corpus | Parse only | Lint summary | Graph-grade per-file facts | Current `ProjectFacts` build |
| :--- | :--- | :--- | :--- | :--- |
| omnicoach (Rust) | 0.21 s | 0.27 s | 0.48 s | 0.51 s |
| Cargo (Rust) | 0.56 s | 0.63 s | 1.13 s | 1.11 s |
| CPython `Lib` (Python) | 1.04 s | 1.54 s | 4.33 s | 7.80 s |

*Sequential times. The lint summary is parse plus imports and class headers with method
docstrings (Python) or the existing Rust file summary. Parallel runs are seven to nine times
faster: CPython's lint summary takes 0.2 s.*

- **Context:** whether a lint-oriented summary is much cheaper than the per-file facts
  `omnicoach graph` needs (assumption A1).
- **Analysis:** parsing is the floor, at 70–90% of a lint summary. The lint summary costs about
  half the graph-grade facts in Rust and a third in Python. The current build adds sequential
  resolution of every reference in every file, which no lint rule reads: on CPython it costs five
  times a sequential lint summary and about forty times a parallel one.
- **Interpretation:** A1 holds, with a nuance: a lighter summary helps, but parallelism and not
  resolving what no rule asks for matter more than shaving the summary further.

### 5.2 End-to-End Cost of the Stage F Prototype

| `omnicoach code` on CPython `Lib` | Wall time | Peak memory |
| :--- | :--- | :--- |
| Default rules | 9.1 s | 434 MB |
| Default rules without `uninherited-protocol` | 1.9 s | 220 MB |

- **Context:** the release binary built from the branch that contained the cross-file
  prototype, run on a large Python project.
- **Analysis:** enabling the one cross-file rule makes the run about five times slower and
  doubles memory. The sequential, graph-grade `ProjectFacts` build runs before any file is
  linted. Rust projects are unaffected because the rule is Python-only.
- **Interpretation:** the prototype was built without a performance check on a large project.
  The redesign needs a latency budget enforced by a benchmark, not only correctness tests.

### 5.3 Context Sets and On-Demand Loading

| What a verdict depends on | Modules (median / max) | Load cost on demand (median / 90th percentile) |
| :--- | :--- | :--- |
| Python: modules needed to resolve a file's imports, re-exports and base classes | 4 / 43 | 5 ms / 17 ms |
| Python: transitive import closure | about 100 / 190 | 84 ms / 99 ms |
| Rust: modules needed for a file's architecture check with in-code declarations | 10 / 67 | 16 ms / 34 ms |
| Rust: transitive reference closure | 32 / 150 | 61 ms / 131 ms |

*Reference points: a parallel whole-project lint summary takes about 40 ms for omnicoach and
200 ms for CPython `Lib`; single-file linting of a large file takes 20–35 ms in omnicoach and
30–80 ms in CPython.*

- **Context:** how many files a verdict depends on (assumption A2), and whether loading only them
  fits the hook (assumption A3, questions Q3 and Q5). Load cost is sequential parse plus summary
  of the file and its context set.
- **Analysis:** resolution context sets are tiny: a median of 4 of about 1,900 Python modules.
  Loading them costs a median 5 ms, far below even a parallel whole-project scan. Transitive
  closures are an order of magnitude larger and cost more than a large file's single-file lint.
  For Rust architecture checks with in-code declarations, on-demand loading and a parallel scan
  of an omnicoach-sized crate cost about the same.
- **Interpretation:** A2 holds, and A3 looks achievable: on-demand loading adds roughly 5–35 ms
  to a hook evaluation. Rules that need transitive closures (module cycles) or usage facts do not
  fit the hook and belong to command-line runs. With configuration-declared components (section
  2), architecture checks would need no other file at all.

### 5.4 Project Context From the Command Line, and Suffix Matching

| Package linted alone | Cross-module references kept | Resolved base classes kept |
| :--- | :--- | :--- |
| `asyncio` | about half | about four in five |
| `importlib` | about a third | two thirds |
| `json` | about a third | none to resolve |

*Compared with the same package linted inside the whole `Lib` project. The other four packages
measured fall in the same range.*

- **Context:** goal 2 of Phase 1 (verdicts independent of the command line) and question Q1
  (Python source roots).
- **Analysis:** linting one package loses between a quarter and two thirds of its cross-module
  facts, so findings change with the paths passed. Separately, about 1% of resolved imports
  resolve only through the prototype's suffix matching, and every one sampled was wrong.
  Examples: the C module `_collections` resolves to `importlib.metadata._collections`, and
  `readline` to `_pyrepl.readline`.
- **Interpretation:** the project context must come from the project's source roots, and imports
  that do not resolve under them must stay unresolved. "Unknown" is the correct answer for a name
  defined outside the project.

## 6. Status of the Phase 1 Assumptions and Open Questions

| ID | Status | Answer |
| :--- | :--- | :--- |
| **A1** summary cost | Confirmed with nuance | Section 5.1: parsing dominates; parallelism and skipping unneeded resolution matter most. |
| **A2** small context sets | Confirmed | Section 5.3. |
| **A3** hook budget | Achievable on demand | Section 5.3; budget to confirm with the user. |
| **A4** downstream project size | Open | No downstream Python project measured yet. |
| **Q1** Python source roots | Proposed | Project root plus configured roots, defaulting to the root and `src/` as Ruff and `ty` do; namespace packages resolve naturally by probing; no suffix matching. |
| **Q2** project boundary | Proposed | The nearest `.omnicoach.toml`, `pyproject.toml` or `Cargo.toml` above the file, never the command-line paths. |
| **Q3** eager or on demand | Leaning on demand | Sections 1 and 5.3; Phase 3 prototypes confirm the code complexity and the cost of parsing a file twice in full runs. |
| **Q4** summary contents | Inputs gathered | Owned and body-free: top-level bindings with import aliases, `__all__` and star imports; classes with ordered bases, decorators and method docstrings; Rust items, `mod` and `use` declarations. |
| **Q5** hook overlay | Feasible | One file source with an overlay map; findings reported on the edited file only. |
| **Q6** Python declarations | Proposed | Configuration canonical, markers as checked annotations (section 2). |
| **Q7** multi-file tests | Proposed | Inline file map per case with strict all-file checking (section 3). |
| **Q8** generic versus omnicoach-only checks | Inputs gathered | Generic: dependency reachability, sibling isolation, relative-path boundaries, external package ownership. omnicoach-specific: component naming, every enum variant having a root. |
| **Q9** Rust declarations elsewhere | Proposed | Configuration by default; the macros optional and recognized by name; omnicoach's own choice decided in Phase 3. |

## 7. Directions for Phase 3

Each track has competing options to prototype in throwaway changes and compare on accuracy,
latency (command line and hook), ergonomics for rule authors, and code complexity.

| Track | Options | Leaning from the evidence |
| :--- | :--- | :--- |
| **Project model loading** | (a) eager parallel summaries of every project file; (b) on-demand summaries memoized for the run; (c) on demand, reusing the summary of each file once it is parsed for linting | (b) or (c): section 5.3 |
| **Architecture declarations** | (a) configuration only, omnicoach migrating its macros; (b) in-code macros for Rust and configuration for Python; (c) both forms recognized, one per project | (c), with configuration as the documented default |
| **Rule contract** | (a) every rule receives a file context that includes the project; (b) one rule type whose check is either single-file or project-aware; (c) keep the separate wrapper type | (a) or (b), decided on how many rule files change and how hard misuse becomes |
| **Multi-file tests** | (a) inline file map per case; (b) headers inside one string; (c) named shared contexts | (a): section 3 |

## 8. Suggestions and Next Steps

1. **Direction validated.** Sections 6 and 7 were validated, in particular configuration as the
   canonical architecture declaration and on-demand loading, and the hook latency budget.
2. **Stage F prototype dropped.** Its cross-file commit was removed from the branch, which
   removes the regression of section 5.2 and four of the bugs of section 4; `CodeLintProject`
   stays. Cross-file `uninherited-protocol` stays in scope as one of the three consumers that
   validate the design, and is rebuilt on it.
3. **Phase 3:** write the design around the four tracks, build the competing prototypes, and
   measure them on the critical user journeys of Phase 1, including a latency benchmark on
   CPython `Lib` that would have caught the Stage F regression.
4. **Out-of-scope finding recorded on the roadmap:** non-UTF-8 source files with a PEP 263
   encoding declaration are reported as unreadable (several CPython test files).

## References

**Multi-file analysis:**

- [rust-analyzer architecture](https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md)
- [rust-analyzer `ItemTree`](https://github.com/rust-lang/rust-analyzer/blob/master/crates/hir-def/src/item_tree.rs)
- [`ty` module discovery](https://docs.astral.sh/ty/modules/)
- [`ty` semantic index](https://github.com/astral-sh/ruff/blob/0.14.0/crates/ty_python_semantic/src/semantic_index.rs)
- [Pyright import resolution](https://github.com/microsoft/pyright/blob/main/docs/import-resolution.md)
- [Biome module graph](https://github.com/biomejs/biome/blob/main/crates/biome_module_graph/src/module_graph.rs)
- [oxlint multi-file analysis](https://github.com/oxc-project/website/blob/main/src/docs/guide/usage/linter/multi-file-analysis.md)
- [Ruff `src` setting](https://docs.astral.sh/ruff/settings/#src)
- [Go analysis facts](https://pkg.go.dev/golang.org/x/tools/go/analysis#hdr-Modular_analysis_with_Facts)
- [Python MRO (C3 linearization)](https://docs.python.org/3/howto/mro.html)

**Architecture conformance:**

- [import-linter contract types](https://github.com/seddonym/import-linter/tree/main/docs/contract_types)
- [tach configuration](https://github.com/gauge-sh/tach/blob/main/docs/usage/configuration.md)
- [dependency-cruiser rules](https://github.com/sverweij/dependency-cruiser/blob/main/doc/rules-reference.md)
- [ArchUnit library API](https://github.com/TNG/ArchUnit/blob/main/docs/userguide/008_The_Library_API.adoc)
- [Deptrac configuration](https://github.com/deptrac/deptrac/blob/main/docs/configuration.md)
- [zoning](https://github.com/The-Billy-Company/zoning)
- [Pants dependency rules](https://github.com/pantsbuild/pants/blob/main/docs/docs/using-pants/validating-dependencies.mdx)
- [Bazel visibility](https://github.com/bazelbuild/bazel/blob/master/docs/concepts/visibility.mdx)

**Multi-file tests:**

- [rust-analyzer fixtures](https://github.com/rust-lang/rust-analyzer/blob/master/crates/test-utils/src/fixture.rs)
- [`ty` Markdown tests](https://github.com/astral-sh/ruff/blob/main/crates/ty_test/README.md)
- [Biome multi-file doc snippets](https://github.com/biomejs/biome/blob/main/crates/biome_analyze/CONTRIBUTING.md)
- [Pylint message documentation tests](https://github.com/pylint-dev/pylint/blob/main/doc/test_messages_documentation.py)
- [Pyright fourslash state](https://github.com/microsoft/pyright/blob/main/packages/pyright-internal/src/tests/harness/fourslash/testState.ts)
