# Abstraction graph: 09 Stage D Understand

> **Status: ALPHA / EXPERIMENTAL — Active Investigation & Design** (Stage D — Phase 1; subject to change at any time). Builds on: [01_understand.md](01_understand.md) to [08_stage_c_agent_formats.md](08_stage_c_agent_formats.md). Next: [10_stage_d_references.md](10_stage_d_references.md).

Markers: **[V]** verified in code, by a measurement, or in a primary source; **[U]** not verified (assumption).

## 1. Problem as Currently Understood

Coding agents spend part of every task finding their way around a repository: locating the code to change, reading files to learn signatures and callers, and checking what a change can break. `omnicoach` already derives a multi-level abstraction graph of a project (components, modules, ownership groups, declarations, type members) with typed relations, abstraction levels, reflexion status, and placement findings, and renders it as text for agents (`omnicoach graph`, Stages A–C).

**This cycle (Stage D):** find out whether, and by how much, that graph makes agents more efficient on real work in a repository, and which changes to the graph and to the way agents receive it would increase the gain. Like Stages A–C, this is an investigation: the output is evidence and a recommendation (D42).

## 2. What Already Exists [V]

### 2.1 The Graph and Its Delivery

| Item | State |
| :--- | :--- |
| CLI | Alpha `omnicoach graph [PATHS...] [--depth module\|public\|full] [--no-signatures]` in [graph.rs](../../../src/code_lint/graph.rs), emitting `AnnotatedStubs` (F3, default) or `CompactBi` (F2) text. A single file at full depth gives a single-file graph; anything else gives a project graph. |
| Content | 5-level containment (D28), levels computed locally per container (D25), bidirectional edges with `:start..end` spans and signatures (D34), reflexion status on component edges (D30); see [05_decisions.md](05_decisions.md). |
| Test code | Excluded from project graphs: the directory walk skips files matching `Config::is_test_path` (`collect_directory_source_files` in [graph.rs](../../../src/code_lint/graph.rs)). |
| Agent integration | None: no mention in [AGENTS.md](../../../AGENTS.md), no skill, no hook. `omnicoach graph` appears in 12 of 1,173 local agent transcripts, all last modified on or after 2026-10-19, when this graph work started. |

### 2.2 Measured on This Repository (2026-10-25)

`src/` is 2,268,941 bytes. Token counts use the bytes / 4 estimate of [08_stage_c_agent_formats.md](08_stage_c_agent_formats.md) §4 (MC4). Times are for the debug build.

| Command | Bytes | Lines | ~Tokens | Time |
| :--- | ---: | ---: | ---: | ---: |
| `omnicoach graph --depth module .` | 117,467 | 713 | 29K | 11.1 s |
| `omnicoach graph --depth public .` | 281,007 | 2,199 | 70K | 16.0 s |
| `omnicoach graph --depth full .` | 722,534 | 6,322 | 181K | 10.5 s |
| `omnicoach graph --depth full --no-signatures .` | 585,664 | 5,020 | 146K | — |
| `omnicoach graph src/code_lint/suppression.rs` | 6,965 | 78 | 1.7K | — |

Observations:

- **Every project view is large.** The smallest (module depth) is about 29K tokens, and its longest line is 6.7 KB, because an edge list such as the one leaving `code_lint::rules` names every target on one line.
- **Cross-module method calls are missing from `used_by` lists.** `code_lint::runner::lint_file` calls `SuppressionTracker::from_file`, `filter_diagnostics`, and `audit` ([runner.rs](../../../src/code_lint/runner.rs) lines 34, 59, and 60). The full graph records `<- code_lint::runner::lint_file[xmod]` on the `SuppressionTracker` type but no incoming edge on any of the three methods, so an agent reading the graph could conclude that they have no callers outside their module. [ROADMAP.md](../../../ROADMAP.md) lists method calls on imported types as a follow-up; associated-function paths such as `SuppressionTracker::from_file(...)` are missing too, which the roadmap item does not mention.

### 2.3 Data Available for Evaluation [V]

- **Local agent transcripts:** 1,173 local agent session transcripts (`transcript.jsonl`), recording every tool call. The agent hooks investigation (`docs/dev/agent_hooks/02_references.md`, Study 1) already replayed this corpus offline to study tool-call patterns.
- **This repository's history:** conventional commits (`feat`, `fix`, `refactor`, `test`), many with tests, checked by `cargo test`, `cargo clippy --all-targets`, `omnicoach code`, and [architecture_conformance.rs](../../../tests/architecture_conformance.rs).
- **Reference corpora** from Stages A–C: ripgrep, cargo, and cpython in `target/bench-corpora/`, fetched by [fetch_corpora.sh](../../../benches/fetch_corpora.sh).

## 3. Why Stages A–C Don't Answer This Yet

| ID | Limitation of the earlier evaluations | Consequence |
| :--- | :--- | :--- |
| E1 | Questions and answer keys use the analyzer's own concepts (`Roots(h)`, ADR 011 levels, findings), which the graph spells out and the source does not. | Scores partly measure whether an agent can read the analyzer's output, not whether it works faster. |
| E2 | The arms were graph-only against source-only, with zero source reads (Stage C) or at most 12 read calls (Stage B matrix). | Real agents have the source, search tools, and the graph together; the gain of adding the graph to such an agent was never measured. |
| E3 | One run per question and condition, with one model; the docs record no repeats or variance. | A difference of one or two answers is within noise. |
| E4 | Tasks were read-only questions or edit plans; code snippets were graded by matching signatures, not compiled or tested. | No evidence on end-to-end changes verified by tests and lint. |
| E5 | Format sizes were reported, not what the agents actually read; the recommended full F3 output is about 181K tokens on this repository (§2.2). | The cost of navigating a large graph file is unknown. |
| E6 | Fidelity (M1) was checked against the analyzer's own facts, not against an independent reference finder such as rust-analyzer or pyright. | Missing edges such as those in §2.2 went unnoticed; an agent that trusts an incomplete `used_by` list can miss callers. |

## 4. Goals (Stage D)

| ID | Goal | Reason |
| :--- | :--- | :--- |
| G7 | Measure where agents spend effort today (exploring, editing, verifying, fixing lint findings) from the local transcripts. | No baseline exists; the share of effort a graph could replace bounds the possible gain (A1). |
| G8 | Establish a sound method to measure agent efficiency with the graph: realistic arms, deterministic oracles, repeated runs, blind grading. | E1–E4; the user wants to investigate how best to evaluate (D43). |
| G9 | Determine whether, and how much, the graph improves task success and quality, tokens, and wall-clock time on the target tasks. | The core question (D39, D41). |
| G10 | Compare the two delivery modes (D40) and find the minimum instructions that make agents use them. | No use without instructions so far (A3); delivery may matter as much as content. |
| G11 | Identify and prototype graph improvements driven by observed agent failures: fidelity, size, focused slices, latency, links to tests. | The user's second question; §2.2, E5, and E6 show concrete gaps. |
| G12 | Recommend what to build in an implementation cycle, with evidence. | D42. |

## 5. Non-Goals

| ID | Non-goal | Reason |
| :--- | :--- | :--- |
| NG6 | Visual viewers or any rendering for humans. | Agents consume text; Stage B found visual graphs worse for agents, and D18 excludes rendering. |
| NG7 | Hook injection, an MCP server, or a repo map injected at session start. | Not selected (D40). Each could later wrap the query subcommands as a thin adapter, so none changes the core design. |
| NG8 | Changing lint rule behavior. | The rules are settled; the investigation only reads their facts (as NG4). |
| NG9 | Embedding or vector-based semantic search. | A different paradigm; the structural graph is what `omnicoach` uniquely provides (CP1). |
| NG10 | A stable schema, a stable CLI, or production hardening. | Investigation only (D42); the graph stays alpha. |
| NG11 | Languages other than Rust and Python, and delivery tied to a proprietary monorepo toolchain. | `omnicoach` supports these two languages; delivery stays tool-agnostic (D38). |

## 6. Decisions

| ID | Decision | Source |
| :--- | :--- | :--- |
| D38 | Optimize for Antigravity agents working in this repository and in other `omnicoach`-linted Rust/Python repositories, and for any coding agent: delivery must be tool-agnostic, needing only a shell and an instructions file. | User (Q25) |
| D39 | Target tasks: end-to-end bug fixes and small features verified by tests and lint; refactors and code placement under ADR 011/012; read-only comprehension and onboarding questions, re-tested under realistic conditions (E2). | User (Q26) |
| D40 | Compare two delivery modes: the graph on demand through the CLI, with `AGENTS.md` or a skill telling agents when to run it; and query subcommands returning focused slices (a symbol's neighborhood, a diff's impact, where to place a new item, the tests covering an item). | User (Q27) |
| D41 | Efficiency means task success and quality (tests pass, lint clean, no architecture violations), tokens consumed, and wall-clock time. Tool calls, turns, and lint-fix iterations are recorded to explain differences, not scored. | User (Q28); recorded diagnostics: Agent |
| D42 | Investigation with prototypes in a separate Jujutsu workspace, ending in a recommendation; implementation is a later cycle. | User (Q29) |
| D43 | Evaluation combines an offline replay of the local transcripts with controlled live subagent trials (the same tasks with and without the graph, 3 or more runs per arm). Tasks rebuilt from this repository's history and public benchmark instances are added if feasible (Q31, Q32); finding the best method is itself part of the investigation (G8). | User (Q30) |

## 7. Assumptions to Validate

| ID | Assumption | Validated by |
| :--- | :--- | :--- |
| A1 | [U] Agents spend a large share of their tool calls and tokens exploring before they edit. | Transcript replay (G7) |
| A2 | [U] A release build runs `omnicoach graph` on this repository in one or two seconds (debug build: 10.5–16.0 s, §2.2). | Timing measurement |
| A3 | [V, partial] Agents don't use the graph unless told to: no use outside this graph work is visible (§2.1). | Transcript replay |
| A4 | [U] The local transcripts are representative of agent work in `omnicoach`-linted repositories, although many come from other repositories. | Transcript replay, filtered by repository |
| A5 | [U] Focused slices of a few KB can answer most of what agents currently read whole files for (signatures, callers, placement). | Transcript replay, then live trials |

## 8. Open Questions

| ID | Question |
| :--- | :--- |
| Q25 | ~~Which agents?~~ Answered (D38). |
| Q26 | ~~Which tasks?~~ Answered (D39). |
| Q27 | ~~Which delivery modes?~~ Answered (D40). |
| Q28 | ~~What does efficient mean?~~ Answered (D41). |
| Q29 | ~~How far does the cycle go?~~ Answered (D42). |
| Q30 | ~~How to evaluate?~~ Answered (D43). |
| Q31 | ~~Can enough tasks be rebuilt from this repository's history: small commits whose tests fail with the change removed and pass with it?~~ Answered in [10_stage_d_references.md](10_stage_d_references.md) (S4). |
| Q32 | ~~Which public benchmark instances (for example Multi-SWE-bench for Rust, SWE-bench Verified for Python) can run locally, at what setup cost, and how exposed are they to training-data contamination?~~ Answered in [10_stage_d_references.md](10_stage_d_references.md) (S6, D49). |
| Q33 | ~~Can tokens and wall-clock time be measured per subagent run from the transcripts, or is a proxy needed?~~ Answered in [10_stage_d_references.md](10_stage_d_references.md) (S7). |
| Q34 | ~~How can end-to-end trials run in isolation and in parallel without the Jujutsu divergence seen in Stage A (N1 in [04_prototypes.md](04_prototypes.md))?~~ Answered in [10_stage_d_references.md](10_stage_d_references.md) (S5, D51). |
| Q35 | ~~Which model or models run the trials, and does the graph help smaller models more?~~ Evaluated in Stage D live trials (H5, D48). |
| Q36 | ~~How complete are the `used_by` edges compared with an independent reference finder (rust-analyzer, pyright)?~~ Evaluated against rust-analyzer SCIP and jedi (H1, D52). |

## 9. Related Ideas (Not Yet in Scope)

- **Thin adapters over the query subcommands:** an MCP server, or hook injection of the slice for the files being read or edited (NG7). Hook injection would need a cached index, since hooks run on every tool call.
- **Change review:** compare the graph before and after a diff to show how a change reshaped the abstractions ("documentation diffs" in [01_understand.md](01_understand.md) §7).
- **Lint coaching with placement:** when the hook reports an ordering finding, include the graph's target insertion line.
- **Test selection in CI:** reuse links from code to tests to run only the tests a diff can affect.
