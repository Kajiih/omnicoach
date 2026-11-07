# Abstraction graph: 10 Stage D References

> **Status: ALPHA / EXPERIMENTAL — Active Investigation & Design** (Stage D — Phase 2; subject to change at any time). Builds on: [09_stage_d_understand.md](09_stage_d_understand.md). Next: [11_stage_d_design.md](11_stage_d_design.md).

Markers: **[V]** read in a primary source (paper, official documentation, source code) or measured on this machine; **[S]** from search summaries only; **[U]** a single secondary source or an estimate. "Vendor" marks results self-reported by a product's maker. Survey notes, measurement scripts, and raw data stay outside the repository; data derived from agent transcripts never enters it, only the aggregates below.

## 1. Summary

1. **Exploration dominates agent effort, but little of it is structural.** In this repository's agent sessions, exploration takes 59% of tool calls, 77% of tool-output bytes, and 54% of planner tokens, yet lookups of definitions, usages, and identifiers are only 9% of exploration calls. The rest reads files that are then edited, re-reads context, or reads version-control state. Answering every graph-answerable call from the graph would save at most about 7% of prompt tokens (S1). A larger gain must come from changed behavior (fewer steps, failed builds, or compactions), which only live trials can show.
2. **The literature favors small focused slices on demand over maps given up front**, and shows that agents rarely use an optional tool unless told exactly when (CPD2–CPD4).
3. **The graph's `used_by` lists cannot yet be trusted for methods:** no cross-module edge reaches a method or associated function (S2). Fix this, or tell agents, before any trial.
4. **At our scale, efficiency is measurable and success differences mostly are not** (CPD11). Tasks can be rebuilt from this repository's history in history-free clones (S4, S5); containers and other agent CLIs are not available locally (S6).

## 2. Internal References

| ID | Reference | Key idea | Relevance |
| :--- | :--- | :--- | :--- |
| RI5 | `omnicoach graph` ([graph.rs](../../../src/code_lint/graph.rs), [render.rs](../../../src/code_lint/graph/render.rs)) | Whole-scope text dumps at three depths in the F3 and F2 formats | The starting point; no slicing or query (sizes in [09](09_stage_d_understand.md) §2.2) |
| RI6 | Cross-module resolution in [project_facts.rs](../../../src/code_lint/graph/project_facts.rs) | Path references (Rust) and imports (Python) resolve to a module and its first top-level name; a reference inside an `impl` or `class` body is attributed to the type (`enclosing_symbol`) | Root cause of the method-level gaps in S2 |
| RI7 | VCS diff detection ([diff.rs](../../../src/diff.rs), `detect_vcs_diff`) | Changed lines for Jujutsu and Git, used by `omnicoach code --diff` | Input for a diff-impact query |
| RI8 | ADR 011 findings (`FindingFact` with a `PlacementAnchor`, [call_clusters.rs](../../../src/code_lint/ast/call_clusters.rs)); in-memory edit application in [virtual_edit.rs](../../../src/hook/virtual_edit.rs) | Ordering findings already name an anchor declaration and line; the hook lints a proposed edit before it lands | Basis for a placement query and for findings that carry the target line |
| RI9 | Agent hooks study (`docs/dev/agent_hooks/02_references.md`) | Offline replay of the local transcript corpus; a 7-arm live benchmark in isolated Git repositories with seeded traps, one run per arm, measuring tokens, turns, and time | Method to reuse, with repeats added (E3) |
| RI10 | [AGENTS.md](../../../AGENTS.md) verification scope | "Run the narrowest checks that cover what you changed" | A tests-for-item query serves it directly |

## 3. External References: Repository Context for Agents

### 3.1 Maps, Graphs, and Navigation Tools

| ID | Reference | Key idea | Access | Reported results | Difference from the omnicoach graph |
| :--- | :--- | :--- | :--- | :--- | :--- |
| RC1 | [Aider repo map](https://aider.chat/docs/repomap.html) ([`repomap.py`](https://github.com/Aider-AI/aider/blob/main/aider/repomap.py), v0.86.0) | Tree-sitter definition and reference tags form a file graph; PageRank personalized on files in the chat and identifiers in the request; a binary search on the number of tags fits a token budget (default `clamp(max_input/8, 1024, 4096)`) [V] | Dump, re-ranked every turn | Given up front, 56 of 200 resolved vs 66 for on-demand exploration (RC10) [V] | Name-matched file-level edges, no levels or ownership; but query-personalized ranking under a hard budget, which omnicoach lacks |
| RC2 | [Agentless](https://arxiv.org/abs/2407.01489) (2024) | Staged localization: repository tree, files, skeletons (signatures, fields), elements, edit lines | Staged dump | SWE-bench Lite: element localization from skeletons 58.3% at $0.02 vs 53.7% from full files at $0.15 [V] | No relations; validates compressed signatures (as F3) and staged zoom |
| RC3 | [repomix `--compress`](https://repomix.com/guide/code-compress), [code2prompt](https://github.com/mufeedvh/code2prompt) | Whole repository packed as signatures; code2prompt ships an Agent Skill | Dump | No task evaluation [V] | Same delivery shape as D40 (a), without graph facts |
| RC4 | [LocAgent](https://arxiv.org/abs/2503.09089) (ACL 2025) | Directory, file, class, function nodes; contain, import, invoke, inherit edges; entity search, typed traversal, retrieval; output adapts to the number of matches | Tools | SWE-bench Lite function Acc@10 77.4 vs 70.1 for OpenHands, in 7 rounds vs 13. Ablation: no entity search −18.2 points, no traversal −5.4; tree output beat JSON and DOT [V] | Python only; no levels, ownership, or architecture |
| RC5 | [RepoGraph](https://arxiv.org/abs/2410.14684) (ICLR 2025) | Line-level definition and reference graph; k-hop ego-graph around a search term | Appended to the prompt, or an agent action | +2.0 to +2.3 points on SWE-bench Lite across three frameworks; 1 hop (2.3K tokens) 29.7% beat 2 hops (10.5K tokens) 26.0% [V] | Name-level relations; size explodes with hops |
| RC6 | [CodexGraph](https://arxiv.org/abs/2408.03910) (NAACL 2025) | Code graph in a graph database; the agent's questions are translated to Cypher | Query language | Equal to AutoCodeRover on SWE-bench Lite (22.96%) at 102K vs 76K tokens; a weaker model 1.95% vs 9.34%; "the length of the query outcomes is not controllable" [V] | Free-form queries, unbounded output |
| RC7 | [CodePlan](https://arxiv.org/abs/2309.12499) (FSE 2024) | Incremental dependency graph with paired relations (calls and called-by, uses and used-by) and change may-impact analysis driving multi-file edits | Planner | 5 of 6 repositories pass validity checks vs 0 for baselines given the same context [V] | Closest analog of a diff-impact query |
| RC8 | [Repository Intelligence Graph](https://arxiv.org/abs/2601.10112) (2026) | Deterministic build and test graph with coverage edges, about 5K tokens of JSON with stable IDs | Dump before the task | Claude Code, Cursor, Codex on 8 repositories × 30 questions: accuracy +12.2%, time −53.9% [V] | Build level only; supports small authoritative facts and links to tests |
| RC9 | [CodeCompass](https://arxiv.org/abs/2602.20048) (2026) | MCP tool returning 1-hop import, inherit, and instantiate neighbors | Tool plus prompt | One FastAPI app, 258 trials: required files touched on hidden-dependency tasks 99.4% vs 76.2%; slightly worse on import-chain tasks. The tool was called in 42% of trials; a checklist at the end of the prompt raised hidden-task adoption from 30/35 to 31/31 [V] | One repository and a coverage metric; adoption is the bottleneck |
| RC10 | [CodeNib](https://arxiv.org/abs/2607.25431) (2026) | Lexical, dense, and structural views of one commit; context removed after use | Bounded tools | SWE-bench Verified, 200 tasks: candidates removed after the first source read 79 resolved, on-demand exploration 66, Aider map 56. An incrementally repaired graph matched a rebuild on 13 of 24 changes [V] | Shows the cost of retained context and the staleness risk of incremental indexes |

Also surveyed: OrcaLoca, CoSIL, KGCompass, LingmaAgent, CGM, RepoMaster, ToolTrain (localization pipelines or trained models, outside D38). KGCompass reports larger relative gains for weaker models [V].

### 3.2 Agent Interfaces and Symbol Tools

| ID | Reference | Key idea | Reported results | Relevance |
| :--- | :--- | :--- | :--- | :--- |
| RC11 | [SWE-agent](https://arxiv.org/abs/2405.15793) (NeurIPS 2024) | Capped, summarized search (at most 50 hits); 100-line file viewer; lint-gated edits | SWE-bench Lite ablations: 100-line viewer 18.0% vs whole file 12.7%; summarized search 18.0% vs iterative 12.0% vs none 15.7% [V] | Compact, capped output wins |
| RC12 | [AutoCodeRover](https://arxiv.org/abs/2404.05427) (ISSTA 2024) | AST search tools returning signatures first, bodies on request | SWE-bench Lite 19% at $0.43 [V] | Skeleton-first output |
| RC13 | [mini-SWE-agent](https://github.com/SWE-agent/mini-swe-agent) | Bash only | Over 74% on SWE-bench Verified [V] | Strong models do well without structure tools: the baseline arm |
| RC14 | [Serena](https://github.com/oraios/serena); [Claude Code LSP tool](https://code.claude.com/docs/en/tools-reference) | Language-server symbol overview, definitions, references, call hierarchy, built into or one plugin away from agents | No controlled evaluation published [V] | Precise references are becoming standard; no abstraction or architecture facts |
| RC15 | [Does a language server save tokens?](https://arxiv.org/abs/2608.13568) (2026, preliminary) | Same model and tasks: grep only, language server only, both, language server forced first, repo map; metric: tokens per success | On localization, the language server cost 6–118% more tokens than grep and went unused when optional; forcing it first dropped success from 100% to 89%; it saved tokens only for the weakest model. Location-only references failed 3 of 4 multi-file renames; inlining each reference's line closed most of the gap [V] | The closest design to ours; tells what a callers query must include |
| RC16 | Code-graph MCP servers ([CodeGraphContext](https://github.com/CodeGraphContext/CodeGraphContext), [code-graph-rag](https://github.com/vitali87/code-graph-rag), [Blarify](https://github.com/blarApp/blarify)) | Tree-sitter or SCIP into a graph database; natural language to Cypher | No task-level evaluation; vendor claims only [V] | Persistent databases can go stale |

### 3.3 Counter-Evidence and Retrieval Studies

| ID | Reference | Finding | Relevance |
| :--- | :--- | :--- | :--- |
| RC17 | Claude Code team (2025 interviews) [S]; [Anthropic, context engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents) (2025) [V] | Agentic search with glob, grep, and read replaced a vector index and "outperformed everything"; aim for "the smallest possible set of high-signal tokens", fetched just in time | Favors just-in-time slices over dumps |
| RC18 | [Cursor semantic search](https://cursor.com/blog/semsearch) (2025, vendor); [Cline](https://cline.bot/blog/why-cline-doesnt-index-your-codebase-and-why-thats-a-good-thing) (2025) | Embeddings next to grep: +12.5% offline accuracy, +2.6% code retention on repositories of 1,000 files or more [V]; indexes go stale and widen the security surface | An index helps mainly at scale, next to grep |
| RC19 | [ContextBench](https://arxiv.org/abs/2602.05892), [SWE-Explore](https://arxiv.org/abs/2606.07297) (2026) | Gold-context retrieval benchmarks: elaborate scaffolding adds little; much explored context goes unused; ranked regions under a line budget predict repair [V] | Evaluation of slices (G8) |
| RC20 | [The SWE-Bench Illusion](https://arxiv.org/abs/2506.12286) (2025) | Models name the buggy file from the issue text alone in up to 76% of SWE-bench Verified tasks vs at most 53% elsewhere [V] | Public localization gains may reflect memorization |

### 3.4 Instruction Surfaces and Context Budgets

| ID | Reference | Finding | Relevance |
| :--- | :--- | :--- | :--- |
| RC21 | [AGENTS.md](https://agents.md) | Plain Markdown, nearest file wins; read by Codex (at most 32 KiB), Cursor, Gemini CLI (configurable), the Copilot agent, Jules, Aider, goose, opencode, and Antigravity [S]; Claude Code only when no CLAUDE.md exists [V] | The tool-agnostic surface (D38) |
| RC22 | [Agent Skills](https://agentskills.io) (open standard, 2025-12) | `SKILL.md` body loaded only on activation; supported by Claude Code, Codex, Gemini CLI, Copilot, Cursor, Antigravity, goose [S] | Second surface for D40 (a) |
| RC23 | [Evaluating AGENTS.md](https://arxiv.org/abs/2602.11988) (2026); [AGENTS.md efficiency](https://arxiv.org/abs/2601.20404) (2026) | LLM-written context files: no success gain, +20–23% cost; overviews did not speed up localization; tools named in the file were used 1.6–2.5 times per task vs almost never otherwise [V]. Paired runs on 124 PRs: median runtime −28.6%, output tokens −16.6% [V] | Instructions to use tools work; overviews don't |
| RC24 | [Vercel evals](https://vercel.com/blog/agents-md-outperforms-skills-in-our-agent-evals) (2026, vendor) | Docs as a skill: never invoked in 56% of cases (53% pass); with explicit instructions 79%; an 8 KB index in AGENTS.md 100% [V] | A passive pointer beats an optional skill |
| RC25 | [Anthropic, writing tools for agents](https://www.anthropic.com/engineering/writing-tools-for-agents) (2025) | Pagination, filtering, truncation messages that say how to narrow, meaningful IDs, concise and detailed modes [V] | Design rules for query subcommands |
| RC26 | Agent tool-output limits | Claude Code shows about 30K characters inline, beyond that a file path and a 2K preview [V]; Gemini CLI truncates at 40,000 characters [V]; Codex at about 10K tokens [S] | Even the module-depth graph (117 KB) exceeds every limit |
| RC27 | [Lost in the Middle](https://arxiv.org/abs/2307.03172), [RULER](https://arxiv.org/abs/2404.06654), [NoLiMa](https://arxiv.org/abs/2502.05167), [context length alone](https://arxiv.org/abs/2510.05381), [SWE-bench](https://arxiv.org/abs/2310.06770) | Accuracy drops with context length even when retrieval is perfect; on SWE-bench, 13K → 50K tokens of retrieved code lowered resolution from 1.96% to 1.22% [V] | Long dumps hide facts |
| RC28 | [Complexity Trap](https://arxiv.org/abs/2508.21433), [SWE-Effi](https://arxiv.org/abs/2509.09853) (2025) | Hiding old tool outputs halves cost at the same solve rate; cost compounds as context accumulates [V] | Tokens re-sent each turn dominate cost |

## 4. External References: Evaluating Agent Efficiency

| ID | Reference | Key point | Use in Stage D |
| :--- | :--- | :--- | :--- |
| RE1 | [Multi-SWE-bench](https://arxiv.org/abs/2504.02605) (2025) | 239 Rust tasks in 10 repositories (clap, tokio, tracing, ripgrep, nushell, fd, bat, ...); fail-to-pass (F2P) and pass-to-pass (P2P) tests validated over three states; one Docker image per task (0.7–2.7 GB compressed) [V] | Main public Rust pool |
| RE2 | [SWE-bench Multilingual](https://www.swebench.com/multilingual.html); [Rust-SWE-bench](https://arxiv.org/abs/2602.22764) (2026) | 43 Rust tasks including ruff, a Rust linter; 500 Rust tasks in 34 repositories where agents reach at most 21.2%, failing notably on repository-wide structure [V] | Rust strata closest to our domain |
| RE3 | [SWE-bench-Live](https://arxiv.org/abs/2505.23419) | Fresh tasks every month, including Rust [V] | Least contaminated public source |
| RE4 | [SWE-bench Verified](https://openai.com/index/introducing-swe-bench-verified/) and audits ([SWE-Bench+](https://arxiv.org/abs/2410.06992), [PatchDiff](https://arxiv.org/abs/2503.15223), [UTBoost](https://arxiv.org/abs/2506.09289)) | Contaminated (RC20) and near saturation; 32.7% of passed patches had the solution leaked and 31.1% passed only through weak tests; 7.8% of "resolved" patches fail the full suite [V] | Leak and test-strength filters |
| RE5 | [SWE-bench issue #465](https://github.com/SWE-bench/SWE-bench/issues/465) | Agents ran `git log --all` and read the future commits that fixed the issue [V] | History-free trial clones |
| RE6 | [SWE-smith](https://arxiv.org/abs/2504.21798), [R2E-Gym](https://arxiv.org/abs/2504.07164) (2025) | Tasks synthesized from repositories and commits, including undoing a PR at the latest commit; naive back-translated statements come out generic [V] | Task construction |
| RE7 | Task set of [Evaluating AGENTS.md](https://arxiv.org/abs/2602.11988) | Statements standardized by an LLM under a no-leak instruction and checked by a human; every test passing on the gold patch joins the regression set [V] | Statement writing |
| RE8 | [FEA-Bench](https://arxiv.org/abs/2503.06680), [RefactorBench](https://arxiv.org/abs/2503.07832) | Feature tasks must pin the new interfaces; refactor oracles as AST-based structural tests (agents 22% vs humans 87%) [V] | Feature and refactor tasks |
| RE9 | [SWE-QA](https://arxiv.org/abs/2509.14635), [CodeQueries](https://arxiv.org/abs/2209.08372), [RepoQA](https://arxiv.org/abs/2406.06025) | Comprehension grading: anonymized LLM-judge votes; answer keys from an independent static analyzer; a deterministic Rust retrieval probe [V] | Q&A tasks with answer keys independent of the graph (E1, E6) |
| RE10 | [SWE-rebench](https://arxiv.org/abs/2505.20411), [AI Agents That Matter](https://arxiv.org/abs/2407.01502), [HAL](https://arxiv.org/abs/2510.11977) | Report runs with standard errors, tokens per problem, and the cached share (77–97%); cost and accuracy jointly; LLM-aided log audits found shortcuts [V] | Reporting and transcript audits |
| RE11 | Prompt-caching prices ([Anthropic](https://docs.claude.com/en/docs/build-with-claude/prompt-caching), [OpenAI](https://platform.openai.com/docs/guides/prompt-caching), [Gemini](https://ai.google.dev/gemini-api/docs/caching)) | Cache reads cost about 0.1× uncached input or less, writes 1.25–2×; multipliers vary by model and over time [V] | Cost-weighted tokens |
| RE12 | [Agent skills benchmark](https://arxiv.org/abs/2608.23067) (2026) | A length-matched irrelevant instruction as control; skills cost +72–394% tokens and −1.3 to −4.2% Pass@2 [V] | Placebo arm |
| RE13 | [Two-agent context ablation](https://arxiv.org/abs/2607.27250) (2026) | The task is the unit of analysis; 3 repeats; within-task permutation tests, Wilcoxon with Holm correction, equivalence testing on a task-clustered bootstrap; 15–17 tasks × 3 repeats leave a minimum detectable success difference above 30 points, and ±10 points needs about 120 tasks [V] | Statistics template |
| RE14 | [TDAD](https://arxiv.org/abs/2603.17973) (2026) | A code-to-test map given as context cut regressions from 6.08% to 1.82%; prescriptive test-first instructions alone raised them to 9.94% [V] | Inform rather than prescribe; supports links to tests |
| RE15 | [Error bars for evals](https://arxiv.org/abs/2411.00640), [small-sample intervals](https://arxiv.org/abs/2503.01747), [τ-bench](https://arxiv.org/abs/2406.12045), [nondeterminism](https://thinkingmachines.ai/blog/defeating-nondeterminism-in-llm-inference/) | Clustered standard errors and paired differences; repeats help less and less (K = 3 gives two thirds of the possible variance reduction in a worked example); Wilson intervals at small N; pass^k for reliability; temperature 0 is not deterministic [V] | Repeats, intervals, reliability |
| RE16 | [LLM-as-a-judge](https://arxiv.org/abs/2306.05685), [Agent-as-a-Judge](https://arxiv.org/abs/2410.10934) | Position, verbosity, and self-preference biases; over 80% agreement with humans [V] | Judge only residual quality, blind |

## 5. Preliminary Measurements

Preliminary measurements run on 2026-10-25 on this machine (16 cores, shared with other builds; times are upper bounds).

### 5.1 S1: Where Agents Spend Effort (G7, A1, A3–A5)

Corpus: 1,171 local agent sessions (2026-05 to 2026-10, 199.7K tool calls). This repository has 160 sessions (77 of them subagents, 9% of calls); no other `omnicoach`-linted repository appears; sessions in other repositories (89% of calls) are within 2 points of the corpus on every share below. Classes: explore (file reads, searches, listings, version-control reads), edit, verify (builds, tests, linters), and others.

| Measure (this repository / all) | Value |
| :--- | :--- |
| Explore share of tool calls, output bytes, planner tokens, active time | 59/58%, 77/76%, 54/47%, 27/24% |
| Before the first edit, per editing session (median) | 23 explore calls, 75 KB, 2.0 active minutes; 67% of the session's exploration |
| Source reads returning the whole file; median read size | 16%; 2.8 KB |
| Reads of a file edited later; re-reads of a range already in context | 62%; 23% (34% corpus-wide) |
| Structural searches (definition, usage, identifier): share of searches, of explore calls | 47%, 9% |
| Graph-answerable explore calls: lower bound (hand-checked) to upper bound | 6–9% to 34% of explore calls; at most ~25% of output bytes and ~7% of prompt tokens |
| Lint-fix edits; failing build and test runs | 4.4% of edits (at most 11%), ~7% of active time; 21% of runs |
| Planner tokens read from cache; sessions with a context compaction | 92%; 94% |
| Sessions that ran `omnicoach graph` | 2, both developing the graph |

The hand-check labeled 30 searches: 15 of the 18 classified as structural were, and 12 were answerable from the graph (each rate ±15–20 points).

### 5.2 S2: Fidelity of `uses` and `used_by` Edges (Q36, E6)

Ground truth: a rust-analyzer SCIP index of this repository, reduced to the graph's granularity (functions, types, inherent methods; test code excluded). All 1,302 graph nodes matched; 3,934 reference edges. "Type-level" counts a method edge as found when its type carries it.

| Recall | Exact | Type-level |
| :--- | ---: | ---: |
| Intra-module | 78.4% | 96.9% |
| Cross-module | 66.9% | 90.7% |
| Cross-module, by target: types / free functions / methods / associated functions | 89.8% / 85.9% / 0% / 0% | 98.8% / 94.9% / 59.1% / 100% |

- **Precision** is 91.9% exact (98.7% type-level) when `sig` edges are read in reverse. Read literally (`->` means "uses"), it falls to 84.6%, because all 264 `sig` edges point from a type to the function whose signature names it.
- **Misses (1,122):** method calls `x.f()` across modules 351; references inside method bodies attributed to the type 392; `T::f()` cut to `T` 118; method to method 116; references only inside macros 67; item to method 66; others 12.
- **Coverage:** two production rule files (`multi_phase_test.rs`, `test_calls_test.rs`) are dropped by the default `**/*_test.rs` test pattern; 61 of 62 rule entry points look unused because only their module's `RULE` constant refers to them; trait methods and trait-impl methods have no nodes.
- **Impact:** 75% of used methods and 83% of used associated functions show no `<-` edge (for example `AstNode::from_span`: 61 callers, none shown).
- **Python** (jedi on cpython `urllib` and `zoneinfo`): the same pattern.
- **Fix order:** resolve full `T::f` and `Self::f` paths and index methods (up to ~158 edges); attribute references in method bodies to the method (~392); give constants a node or a "registered by" edge; render `sig` apart from uses. Receiver-typed calls `x.f()` (576 edges) need type information: a name-only resolver is 45% precise.
- **Cost of the reference:** `rust-analyzer scip` takes 44 s warm (2 min cold) and 1.8 GB of memory.

### 5.3 S3: Latency (A2)

| Target | Release build, median |
| :--- | ---: |
| This repository, any depth | 0.44–0.61 s (debug build: 7.6–8.7 s) |
| cargo (10.6 MB) | 1.4–1.6 s |
| ripgrep `crates/ignore`, cpython `Lib/urllib` | ≤ 0.1 s |
| One file | 10–40 ms |

Peak memory is at most 26 MB; the build is single-threaded. On-demand use needs no cache or daemon.

### 5.4 S4: Tasks Rebuilt From This Repository's History (Q31)

- **Pool:** 224 of 325 commits change both code and tests; 122 of them change only inline `#[cfg(test)]` code, so tests must be split from code within files. At most 3 code files and 150 code lines: 45 commits, about 32 with a clear message; at most 5 files and 300 lines: 79.
- **Sample of 12, replayed in clones:** all 12 yield an oracle (at least one test fails without the change and passes with it; none breaks the other way). 9 fail at run time; 3 fail to compile because the tests call a new or moved function, so their statement must name it (suitable for refactor and placement tasks). 2 commits fail the repository's own lint test, so "lint clean" must mean "no new findings compared with the solution". One verification takes 70–334 s (median 161 s).
- **Leakage:** a full clone contains the solution; the ROADMAP or a design document at the parent commit describes 4 of the 12 tasks; snapshot tests show exact output; commit subjects state the fix.
- **Estimate:** about 24 behavioral tasks and 8 API-naming tasks at the strict thresholds.

### 5.5 S5: Isolating Parallel Trials (Q34)

- **Working copy:** a clone made with `git clone --ref-format=files --no-local --single-branch --no-tags` from a mirror reference at the parent commit, with its remote removed, hides the solution (setup under 2 s). Clones made with `--local`, `--shared`, or worktrees keep it one `git log --all` away. Jujutsu cannot read Git 2.56's default reftable format, hence `--ref-format=files`.
- **Builds:** a private `target/` copied from a build of the same commit gives a 20 s setup, a 0.35 s first build, 50 s for the first edit, and 13 s per edit after that, at 2.5 GB per trial. A cold build takes 134 s (243 s with two in parallel).
- **Never share `CARGO_TARGET_DIR` between clones:** cargo's fingerprints ignore the clone path and compare modification times, so a clone at another commit rebuilt nothing and ran a test that exists only in the other clone.
- Separate repositories cannot reproduce Stage A's Jujutsu divergence (N1).

### 5.6 S6: Local Environment (Q32, Q35, D38)

- No Docker or Podman; rootless Podman needs an administrator (no subordinate ID ranges). Unprivileged sandboxing with bubblewrap works; 2.6 TB of disk are free. The Docker-based harnesses of RE1–RE4 cannot run as published; Rust tasks could run natively in clones.
- No other agent CLI is installed (the `codex` on `PATH` is an unrelated tool); Node.js is present without npm. Trials can only vary the model tiers of this harness (flash-lite, flash, pro).

### 5.7 S7: Measuring Tokens and Time (Q33)

Each model call records `input_tokens` (uncached), `cache_read_tokens`, and `output_tokens`, in 82% of sessions since 2026-10-10 and on every step of those sessions; every step has a timestamp. Subagent runs are separate conversations, linked to their parent in both directions. Output tokens are about 2.9 times the visible text, so they likely include reasoning [U]; no cache-write count is recorded.

### 5.8 Assumptions and Questions Answered

| ID | Verdict | Evidence |
| :--- | :--- | :--- |
| A1 | Holds for calls, output bytes, and tokens; not for active time | S1 |
| A2 | Holds: about 0.5 s on this repository | S3 |
| A3 | Holds: no use outside the graph work | S1 |
| A4 | Cannot be checked: no other `omnicoach`-linted repository in the corpus; sessions in other repositories show the same shape | S1 |
| A5 | Reformulated: agents already read slices of about 2.8 KB, so slices help only for reads of files not edited afterwards and whole-file reads | S1 |
| Q31 | Yes: about 32 candidate tasks at the strict thresholds | S4 |
| Q32 | Public harnesses need Docker, absent here; native Rust runs are possible with effort | S6, RE1–RE4 |
| Q33 | Yes, directly from transcripts | S7 |
| Q34 | One truncated clone and one private, pre-seeded `target/` per trial | S5 |
| Q35 | Only this harness's tiers without installs; the literature expects larger relative gains for weaker models | S6, RC15, KGCompass |
| Q36 | 71.5% exact recall (93.1% at type level); none for cross-module calls to methods | S2 |

## 6. Critical Points

| ID | Point | Evidence |
| :--- | :--- | :--- |
| CPD1 | **The prize is behavioral.** Substitution caps the saving near 7% of prompt tokens. A larger gain must come from fewer steps: shorter exploration before the first edit (median 23 calls), fewer failing builds and tests (21% of runs), fewer compactions (94% of sessions). Expect gains on hidden-dependency and structural tasks, little where grep suffices; stratify tasks that way. | S1, RC4, RC9, RC15, RC19 |
| CPD2 | **Whole-graph dumps don't reach the model.** Project views of 117–723 KB exceed every agent's inline output limit; mode (a) of D40 really means "the graph in a file, read with grep and read", or runs scoped to a few paths. | RC26, RC27, [09](09_stage_d_understand.md) §2.2 |
| CPD3 | **Small slices, early.** Skeletons beat full files, 1 hop beats 2, a 100-line window beats whole files, and maps given up front lose to on-demand exploration. Since 92% of planner tokens are cache reads, every output byte is paid again on each later turn. Agents already read 2.8 KB at a time: default slices of 2–3 KB, capped well under 10K tokens. | RC2, RC5, RC10, RC11, RC28, RE11, S1 |
| CPD4 | **Adoption is the main risk.** Optional tools are skipped (graph tool in 42% of trials, language server unused on localization, skill unused in 56% of cases), while tools named in AGENTS.md get used. Use a few one-line conditional triggers naming exact commands, measure adoption, and add a forced-use arm to separate value from adoption. | S1 (A3), RC9, RC15, RC23, RC24 |
| CPD5 | **Fixed query verbs, not a query language or a map.** Free-form queries fail weaker models and return unbounded output; generic overviews cost more without helping; small, authoritative, hard-to-derive facts help. The only always-on candidates are the component DAG and the placement rules, not module edge lists. | RC6, RC8, RC23, RC24 |
| CPD6 | **`find` first.** Entity search was worth 18 points against 5 for traversal, and almost half of agents' searches look up a definition, a usage, or an identifier. | RC4, S1 |
| CPD7 | **`callers` and `impact` must be complete, or say what they miss.** Location-only reference lists broke 3 of 4 renames, and inlining each call site fixed most of them; today no cross-module edge reaches a method. Fix path calls and method attribution first, then print each call site's line, how the edge was found, and an `rg` command for the known blind spots. | RC15, S2 |
| CPD8 | **A 1-hop typed neighborhood as a tree with IDs, ranked under a budget.** Tree output beat JSON and DOT; outputs adapt to the number of matches and end with a hint on how to narrow; Aider's personalized ranking with a budget search carries over to typed edges and removes 6.7 KB edge lines. | RC1, RC4, RC11, RC25 |
| CPD9 | **Compete where language servers can't.** Symbol navigation is built into agents or one plugin away and adds little for localization. The graph's distinct facts are abstraction levels, ownership, typed abstraction edges, reflexion against the declared DAG, placement under ADR 011/012, links to tests, and ranked slices; reference completeness is the entry ticket (CPD7). | RC14, RC15, [02](02_references.md) CP1 |
| CPD10 | **Recomputing from source avoids staleness, and the speed allows it.** Indexes go stale, and an incrementally repaired graph matched a rebuild on 13 of 24 changes; `omnicoach` rebuilds this repository's graph in 0.5 s, so on-demand use needs no cache. An optional rust-analyzer backend (44 s warm) would need one. | RC10, RC17, RC18, S2, S3 |
| CPD11 | **Efficiency is measurable; success differences mostly are not.** With about 30 tasks × 3 repeats, success differences below 20–25 points are undetectable, while paired token and time ratios of 15–25% are. Make efficiency primary, success a non-inferiority guard, the task the unit of analysis, and run a pilot first. | RE13, RE15 |
| CPD12 | **Tasks from this repository's history, in history-free clones.** About 32 small commits qualify and all 12 sampled yield oracles. Controls: truncated clones, statements describing the symptom rather than the fix, hidden tests, lint graded as new findings, checks of test strength, and transcript audits for reads of the real repository, which trial agents can reach on disk. | S4, S5, RE4, RE5, RE7 |
| CPD13 | **Public benchmarks only as a small external-validity stratum.** Rust tasks exist, but their harnesses need Docker, absent here, and SWE-bench Verified is contaminated. | RE1–RE4, RC20, S6 |
| CPD14 | **One harness, several model tiers.** No other agent CLI is installed, so "any agent" (D38) can't be tested directly: keep the delivery tool-agnostic (shell and AGENTS.md) and vary this harness's tiers, where weaker models are expected to gain more. | S6, RC15, RC21 |

## 7. Suggestions and Combinations

1. **I1, placement query:** `place` returns the target module (ADR 012) and the insertion line (ADR 011) for a new item given its intended callers and callees, with its resulting roots. Every surveyed system locates existing code; none says where new code belongs. Builds on RI8.
2. **I2, impact that checks the architecture:** the may-impact set of a diff or a signature change (RC7, RI7), new edges that would break the declared DAG, and the tests to run (RE14, RI10).
3. **I3, a completeness footer in every slice:** the edge kinds that may be missing and the exact `rg` command that recovers them, combining graph precision with grep recall (RC15, S2).
4. **I4, ranking by edge type and level:** personalized PageRank over typed edges, seeded by a symbol, a file, or a diff (RC1).
5. **I5, static links to tests:** keep test code as a source of edges so that `tests <item>` answers at function level; validate against `cargo llvm-cov` coverage.
6. **I6, IDs that double as read commands:** for example `src/x.rs::T::m@120..180`, so the agent can read the span directly.
7. **I7, output sized for agent limits:** stay under the smallest inline limit by default, with `--out FILE` for full dumps (CPD2).
8. **I8, an optional cached SCIP backend** for receiver-typed method calls, keeping the default path dependency-free (S2, CPD10).
9. **I9, a checklist trigger at the end of AGENTS.md** as an instruction variant (RC9).
10. **I10, lint findings that carry the target line and a slice:** lower priority, since lint-fix loops take about 7% of active time (S1).

## 8. Questions for Phase 3

| ID | Question |
| :--- | :--- |
| Q37 | ~~Efficiency or success as the primary outcome?~~ Answered (D45). |
| Q38 | ~~Fix method-level resolution before the trials?~~ Answered (D46). |
| Q39 | ~~Which arms beyond A, B, and C?~~ Answered (D47). |
| Q40 | ~~A second, smaller model tier?~~ Answered (D48). |
| Q41 | ~~Which external-validity stratum?~~ Answered (D49). |
| Q42 | ~~Install another agent CLI?~~ Answered (D48). |
| Q43 | ~~Which trial budget?~~ Answered (D50). |
| Q44 | ~~Run the trials with the `omnicoach` hook active or disabled?~~ Answered (D51). |

## 9. Decisions

| ID | Decision | Source |
| :--- | :--- | :--- |
| D44 | Stage D documents and prototypes live in the `graph-dev` Jujutsu workspace (`../custom_lints-graph`), on top of the commit that records this investigation, never in the default workspace. | User |
| D45 | Efficiency is the primary outcome: cost-weighted tokens (cached tokens included) and wall-clock time per run. Success is a non-inferiority guard (CPD11). | User (Q37) |
| D46 | Before the trials, the prototype resolves full paths (`T::f`, `Self::f`) to methods and attributes references inside method bodies to the method (S2). Calls on a receiver (`x.f()`) stay a known blind spot that every slice states. | User (Q38) |
| D47 | Arms: A, baseline; B, `omnicoach graph` with instructions; C, query subcommands with instructions; D, C with forced use, which separates the value of the graph from its adoption (CPD4). No placebo arm. | User (Q39) |
| D48 | The main model runs every task; a smaller tier (flash) runs a subset (Q35). No other agent CLI is installed, so "any coding agent" (D38) rests on the tool-agnostic delivery rather than on cross-harness trials (CPD14). | User (Q40, Q42) |
| D49 | The external-validity stratum is a few public Rust tasks (for example from ripgrep, fd, or bat in Multi-SWE-bench, RE1) run natively in clones, without containers (CPD13). | User (Q41) |
| D50 | A pilot of about 54 runs comes first; the main run (about 270 runs) is sized from its results. | User (Q43) |
| D51 | Trial clones live in `/tmp` outside the workspace roots so `PreToolUse` file-edit hook coaching stays inactive across all arms (avoiding mid-turn confounding), while end-of-task `omnicoach code` checks remain active. | Design (Q44) |
