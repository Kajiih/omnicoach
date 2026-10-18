# Stage C — Investigating Optimal Structural Formats for AI Coding Agents

> **Status: ALPHA / EXPERIMENTAL — Active Investigation & Design** (Formats `F1`–`F4` and findings below are experimental research artifacts and may change at any time).

## 1. Motivation & Research Grounding

In Stages A and B, we evaluated whether exporting `omnicoach`'s architectural and intra-module facts helps humans (via Cytoscape + ELK compound graphs) and AI agents (via `.outline.md`). While `.outline.md` (`Full` depth) achieved **24 / 24 (`100%`)** on structural Q&A versus **18.5 / 24 (`77.1%`)** for equal-budget `Source-Only`, we only tested **one** text format and only on **read-only structural Q&A**.

Recent empirical research and production coding agents point to three distinct paradigms for feeding repository structure to LLMs:

1. **Code Skeletons / Signature Stubs (`Aider` Repo Map, `Agentless`)**:
   - *Mechanism*: Parse files with Tree-sitter/AST, strip function bodies (`...`), and emit valid-looking language headers (`class`, `struct`, `def`, `fn` with parameter and return types).
   - *Strength*: Leverages LLM pretraining on `.pyi` stubs and Rust headers; provides exact parameter/return types needed to **call APIs or synthesize patches** without opening source files.
   - *Gap*: Listed in raw source order; omits abstraction levels (`[Lk]`), intra-module ownership (`Owner(p)` vs. `Shared(S)`), explicit dependency edges (`-> uses` / `<- used_by`), and architectural/placement diagnostics.
2. **Expanded Tree-Based Heterogeneous Graphs (`LocAgent`, `RepoGraph`, Stage A/B `Outline`)**:
   - *Mechanism*: Render containment (`Component -> Module -> Owner -> Declaration`) and typed directed edges (`call`, `containment`, `signature_contract`, `encapsulation`, `impl_behavior`) as an indented text hierarchy.
   - *Strength*: Excels at multi-hop topological reasoning, cycle (SCC) detection, and ownership partitioning.
   - *Gaps in Stage A/B `Outline`*:
     - **Unidirectional (`->`) edges only**: Answering reverse blast-radius queries (*"Who calls `X`? Which modules/functions break if `X` changes?"*) forces the agent to scan and mentally invert edges across the entire file.
     - **Missing declaration end-lines (`:start..end`)**: Without knowing where a declaration ends, an agent planning a refactoring or moving a misplaced helper to satisfy ADR 011 cannot give an exact target insertion line in 1 step.
     - **Missing type/callable signatures**: An agent cannot write a valid call to `SuppressionTracker::from_file` or `IgnoreBuilder::build_with_cwd` from `.outline.md` alone because parameter and return types are absent.
     - **Token-heavy prose boilerplate**: Repeating `Component [L4]`, `* Group [L1]`, `public callable`, `cycle=0`, and full file paths on every line inflates token count by ~35–45%.
3. **Structured Data Schemas (`YAML` / `JSON`)**:
   - *Mechanism*: Encode components, modules, declarations, and edges as structured key-value records.
   - *Trade-off*: Explicit field keys, but empirical LLM format studies show higher token overhead (`+15%–40%`) and potential syntax-escaping friction when embedding code signatures.

---

## 2. Candidate Formats Under Test (`F1`–`F4`)

All four formats are generated from the exact same `ProjectFactsFile` / `AbstractionGraph` (`Full` depth, with `F18` transitive public-to-public summary projection enabled):

| ID | Format Name | Representation Style | Edges Shown | Span Format | Signatures Included? |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **`F1`** | **`Outline` (Stage B Baseline)** | Verbose Markdown headings & bullets (`## Component`, `### Module`, `* Group`, `- [Lk] public callable ...`) | Outgoing only (`-> Target [reason]`) | Start only (`path:line:col`) | No (symbol names only) |
| **`F2`** | **`CompactBi` (Token-Dense Bidirectional Graph DSL)** | Compact hierarchical DSL; strips `cycle=0` and redundant file path prefixes; groups methods inline | **Bidirectional** (`-> uses` and `<- used_by` at both module and symbol levels) | Start & end (`:start..end`) | No (symbol names + kinds only) |
| **`F3`** | **`AnnotatedStubs` (Code-Stub + Graph Hybrid)** | Valid-style Rust / Python `.pyi` signatures & struct fields grouped by `[Lk]` layer and `Owner(p)` / `Shared(S)` section | **Bidirectional** inline (`// [Lk :start..end] -> uses | <- used_by | !finding`) | Start & end (`:start..end`) | **Yes** (`fn`/`def` params & return types, `struct`/`enum`/`class` fields/variants) |
| **`F4`** | **`StructuredYaml` (Hierarchical YAML Schema)** | Machine-oriented YAML records (`components:`, `modules:`, `groups:`, `items:`, `uses:`, `used_by:`) | **Bidirectional** (`uses:` and `used_by:` lists) | Start & end (`span: start..end`) | **Yes** (`sig:` field) |

---

## 3. Multi-Task Agent Evaluation Benchmark (`24` Tasks × `4` Formats = `96` Runs)

To test *how* agents actually use structural context during coding workflows, each format is evaluated by an isolated `research` subagent with **zero raw source file reads allowed** across three task suites (`8` tasks per suite = `24` tasks total):

### Task Suite `T1` — Reverse Blast Radius & Multi-Hop Impact (`8` Tasks)

Tests whether the agent can trace incoming (`used_by`) dependencies across modules and within modules without hallucinating or missing callers:

- **`T1.1` (`omnicoach`)**: List every module (and the exact calling function/type in each module) that references `code_lint::semantic::comments::CommentIndex` or `strip_comment_delimiters`, plus every intra-module caller of `SuppressionTracker::parse_comment_text` in `code_lint::suppression`.
- **`T1.2` (`omnicoach`)**: In `rule_selection`, which declarations (public or private) directly call or reference `Plan`, and which declarations directly call `resolve`?
- **`T1.3` (`ripgrep_ignore`)**: List every module and top-level symbol across the `ignore` crate that references `gitignore::Gitignore` or `gitignore::GitignoreBuilder`, and every intra-module caller of `GitignoreBuilder::add_line` inside `gitignore.rs`.
- **`T1.4` (`ripgrep_ignore`)**: In `dir.rs`, trace the complete reverse caller chain (`<- used_by`) starting from the private type `IgnoreInner` up to the public entrypoints of `dir.rs`.
- **`T1.5` (`cpython_zoneinfo`)**: Which functions/methods across the entire `zoneinfo` package call `zoneinfo._common.load_tzdata` vs. `zoneinfo._common.load_data`, and which functions inside `zoneinfo._tzpath` call `_reset_tzpath`?
- **`T1.6` (`cpython_zoneinfo`)**: In `zoneinfo._zoneinfo`, list every caller (method or function) of `_load_timedelta` and every caller of `_post_epoch_days_before_year`.
- **`T1.7` (`cpython_urllib`)**: Which functions/classes in `urllib.request` and `urllib.robotparser` reference `urllib.error.HTTPError` vs. `urllib.error.URLError`?
- **`T1.8` (`cpython_urllib`)**: In `urllib.parse`, list allintra-module callers of `_coerce_args` and all intra-module callers of `_splitnetloc`.

### Task Suite `T2` — 1-Step Refactoring & ADR 011 Placement Planning (`8` Tasks)

Tests whether the agent can compute the **exact source line span (`start..end`) to cut** and the **exact target line number / anchor (`after <symbol> at line <N>`) to insert at** so that all three ADR 011 rules (`private-before-public`, `uncolocated-helper`, `callee-before-caller`) are satisfied in a single edit:

- **`T2.1` (`cpython_zoneinfo`)**: In `zoneinfo/_zoneinfo.py`, `_load_timedelta` has a `[private-before-public]` finding (`related=ZoneInfo`). Give its exact current line span (`start..end`), explain why moving it immediately after `ZoneInfo` (or after all callers) is required, and give the exact line number in `_zoneinfo.py` after which `_load_timedelta` must be placed so it satisfies `private-before-public`, `uncolocated-helper`, AND `callee-before-caller` with respect to all of its callers (`ZoneInfo`, `_TZStr`, `_parse_tz_str`).
- **`T2.2` (`cpython_zoneinfo`)**: In `zoneinfo/_zoneinfo.py`, `_TZStr`, `_DayOffset`, `_CalendarOffset`, and `_post_epoch_days_before_year` have `[callee-before-caller]` findings. Give the current line span (`start..end`) of each of these 4 declarations and a valid top-to-bottom target ordering of all module-level private declarations in `_zoneinfo.py` (with exact insertion anchor lines) that resolves all 4 findings without introducing new ones.
- **`T2.3` (`cpython_zoneinfo`)**: In `zoneinfo/_zoneinfo.py`, class `ZoneInfo` has 3 methods with `[uncolocated-helper]` findings (`_new_instance`, `_find_trans`, `_get_local_timestamp`). For each of the 3 methods, state its current line span (`start..end`), its owning public method (`Roots(m)`), and the exact target line number (either Place 1 immediately after its owning public method, or Place 2 after the last public method of `ZoneInfo`) that resolves all 3 findings.
- **`T2.4` (`cpython_zoneinfo`)**: In `zoneinfo/_zoneinfo.py`, `_CalendarOffset::_ymd2ord` has a `[private-before-public]` finding (`related=year_to_epoch`). Give its current line span (`start..end`) and the exact line number after which it must be moved.
- **`T2.5` (`omnicoach`)**: Suppose we extract a new private helper function `parse_directive_rules` in `src/code_lint/suppression.rs` that is called *only* by `SuppressionTracker::parse_comment_text` and constructs `BracketedRules`. Based on the layout and spans of `src/code_lint/suppression.rs`, what are its `Roots`, and between which exact declarations and line numbers in `src/code_lint/suppression.rs` must `parse_directive_rules` be placed to satisfy all ADR 011 rules?
- **`T2.6` (`omnicoach`)**: In `src/rule_selection.rs`, suppose a new private helper `validate_override_key` is called by both `load_config` (`[L3]`) and `parse_plan_with_overrides` (`[L3]` in `Shared(rule_status_with_overrides, parse_config_with_overrides)`). What will `Roots(validate_override_key)` be, and at what exact line number range in `src/rule_selection.rs` can `validate_override_key` be placed to satisfy `private-before-public`, `uncolocated-helper`, and `callee-before-caller`?
- **`T2.7` (`ripgrep_ignore`)**: In `gitignore.rs`, identify all declarations with ADR 011 findings (`[callee-before-caller]`, `[uncolocated-helper]`, or `[private-before-public]`), give their exact current line spans (`start..end`), and specify where each must move to resolve the findings.
- **`T2.8` (`cpython_urllib`)**: In `urllib/robotparser.py`, check whether any declaration has an ADR 011 finding, and determine the exact line number range where a new private helper `_strip_port` called by both `normalize_uri` and `RuleLine.applies_to` must be placed to satisfy all three ADR 011 rules.

### Task Suite `T3` — Zero-Read API Usage & Synthesis (`8` Tasks)

Tests whether the agent can write **syntactically and type-correct code** calling a module's public and internal APIs *without opening any source file*, graded against the actual parameter names, parameter types, and return types in the codebase:

- **`T3.1` (`omnicoach` — `code_lint::suppression`)**: Write a valid Rust function that constructs a `SuppressionTracker` from a `&ParsedFile` and `&str` source, filters a `Vec<Diagnostic>`, and runs `.audit(...)`, passing the exact required parameter types in the right order.
- **`T3.2` (`omnicoach` — `rule_selection`)**: Write a valid Rust snippet that calls `load_config`, `parse_config_with_overrides`, and `rule_status_with_overrides`, using their exact parameter types, return types (`Result<..., ...>`), and `RuleStatus` variants/fields.
- **`T3.3` (`ripgrep_ignore` — `dir` & `gitignore`)**: Write a valid Rust snippet inside the `ignore` crate that uses `IgnoreBuilder` to build an `Ignore` root, calls `Ignore::add_parents` and `Ignore::matched`, and inspects the fields/variants of `IgnoreMatch`.
- **`T3.4` (`ripgrep_ignore` — `types`)**: Write a valid Rust snippet that uses `TypesBuilder` (`new`, `add`, `add_defaults`, `select`, `negate`, `build`) to construct a `Types` matcher and call `Types::matched` with its exact parameter and return types.
- **`T3.5` (`cpython_zoneinfo` — `zoneinfo._zoneinfo` & `_tzpath`)**: Write a valid Python snippet that calls `reset_tzpath`, `find_tzfile`, `available_timezones`, and constructs `ZoneInfo` via its default constructor, `ZoneInfo.from_file`, and `ZoneInfo.no_cache`, with exact parameter names and defaults.
- **`T3.6` (`cpython_zoneinfo` — `zoneinfo._common`)**: Write a valid Python snippet that calls `load_tzdata` and `load_data` from `zoneinfo._common` with their exact parameter names and handles `ZoneInfoNotFoundError`.
- **`T3.7` (`cpython_urllib` — `urllib.robotparser`)**: Write a valid Python snippet that instantiates `RobotFileParser`, sets its URL, parses lines, checks `can_fetch`, `crawl_delay`, and `request_rate`, and accesses the fields of the returned `RequestRate` / `Entry` / `RuleLine` objects.
- **`T3.8` (`cpython_urllib` — `urllib.parse`)**: Write a valid Python snippet that calls `urlparse`, `urlsplit`, `urljoin`, `urldefrag`, and `quote_from_bytes` with their exact parameter names and default arguments.

---

## 4. Evaluation Metrics (`MC1`–`MC4`)

- **`MC1` (Comprehension & Impact Accuracy — `T1`, `/8`)**: Exact match on reverse `used_by` callers, modules, and reference counts.
- **`MC2` (1-Step Refactoring & Placement Accuracy — `T2`, `/8`)**: Exact `start..end` spans and target insertion line numbers that satisfy all three ADR 011 stages (`private-before-public`, `uncolocated-helper`, `callee-before-caller`).
- **`MC3` (Zero-Read API Synthesis Accuracy — `T3`, `/8`)**: Exact match on function/method names, parameter counts/types/names, return types, and struct/enum fields without reading raw source files.
- **`MC4` (Token & Byte Compression Ratio)**: Total bytes, lines, and estimated tokens (`bytes / 4`) across the 4 benchmark projects relative to raw source (`2,282 KB` across the 4 projects) and relative to `F1_Outline`.

---

## 5. `MC4` — Token, Line & Byte Compression Results

All four formats were exported at `ProjectDepth::Full` across the 4 benchmark projects (`omnicoach`, `ripgrep/crates/ignore`, `cpython/Lib/zoneinfo`, `cpython/Lib/urllib`):

| Project (`Full`) | Raw Source B (L) | `F1_Outline` B (L / %) | `F2_CompactBi` B (L / %) | `F3_AnnotatedStubs` B (L / %) | `F4_StructuredYaml` B (L / %) |
| :--- | ---: | ---: | ---: | ---: | ---: |
| `proj_omnicoach` | 1,857,728 (47,997) | 437,087 (6,445 / 23%) | **383,444 (3,617 / 20%)** | 479,396 (4,556 / 25%) | 663,950 (8,512 / 35%) |
| `proj_ripgrep_ignore` | 245,080 (7,042) | 57,861 (928 / 23%) | **45,287 (662 / 18%)** | 65,145 (944 / 26%) | 108,299 (1,947 / 44%) |
| `proj_cpython_zoneinfo` | 37,453 (1,174) | 12,991 (191 / 34%) | **9,392 (162 / 25%)** | **11,971 (223 / 31%)** | 21,594 (453 / 57%) |
| `proj_cpython_urllib` | 141,833 (3,985) | 66,445 (1,029 / 46%) | **51,291 (814 / 36%)** | **63,614 (1,069 / 44%)** | 109,036 (2,116 / 76%) |
| **Total Across 4 Projects** | **2,282,094 (60,198)** | **574,384 (8,593 / 25.2%)** | **489,414 (5,255 / 21.4%)** | **620,126 (6,792 / 27.2%)** | **902,879 (13,028 / 39.6%)** |
| **Relative to `F1_Outline`** | `3.97×` B (`7.01×` L) | `1.00×` B (`1.00×` L) | **`0.85×` B (`0.61×` L)** | **`1.08×` B (`0.79×` L)** | `1.57×` B (`1.52×` L) |

### Key Compression Takeaways

1. **`F2_CompactBi` is strictly smaller than `F1_Outline` (`-14.8%` bytes, `-38.8%` lines)** despite adding bidirectional incoming edges (`<- used_by`) at both module and declaration levels plus `:start..end` line spans. Stripping `cycle=0`, redundant per-item file prefixes inside modules, and verbose Markdown prose saves more tokens than bidirectional edges cost.
2. **`F3_AnnotatedStubs` adds full Rust/Python callable signatures and struct/enum/class fields at almost zero net overhead (`+8.0%` bytes, `-21.0%` lines vs. `F1_Outline`, and smaller in bytes than `F1_Outline` on both Python benchmarks)**.
3. **`F4_StructuredYaml` incurs a `+57.2%` byte penalty (`+51.6%` lines) over `F1_Outline` (`+45.6%` bytes over `F3_AnnotatedStubs`)** while encoding the exact same information as `F3_AnnotatedStubs`, due to repeated YAML keys (`kind:`, `vis:`, `level:`, `span:`, `sig:`, `roots:`, `uses:`, `used_by:`) and string quoting.

---

## 6. Blind 96-Run Agent Evaluation Results (`MC1`–`MC3`)

Four isolated `research` subagents were spawned in parallel, each restricted strictly to one format (`F1_Outline`, `F2_CompactBi`, `F3_AnnotatedStubs`, `F4_StructuredYaml`) with **zero raw source file reads allowed**:

| Format Condition | `MC1` (`T1` Reverse Impact, `/8`) | `MC2` (`T2` 1-Step Refactoring, `/8`) | `MC3` (`T3` Zero-Read API Usage, `/8`) | **Total Score (`/24`)** | Agent Steps / Tool Calls | Total Bytes (`%` of Source) |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: |
| **`F1_Outline` (Stage B Baseline)** | **8.0 / 8** (`100%`) | 4.5 / 8 (`56.3%`) | 5.50 / 8 (`68.8%`) *(25% on `omnicoach`)* | **18.00 / 24 (`75.0%`)** | 13 steps / 21 calls | 574 KB (`25.2%`) |
| **`F2_CompactBi` (Bidirectional DSL)** | **8.0 / 8** (`100%`) | **8.0 / 8 (`100%`)** | 5.25 / 8 (`65.6%`) *(25% on `omnicoach`)* | **21.25 / 24 (`88.5%`)** | **6 steps / 11 calls** | **489 KB (`21.4%`)** |
| **`F3_AnnotatedStubs` (Stubs + Bi-Graph)** | **8.0 / 8** (`100%`) | **8.0 / 8 (`100%`)** | **8.00 / 8 (`100%`)** *(100% on `omnicoach`)* | **24.00 / 24 (`100.0%`)** | 12 steps / 23 calls | 620 KB (`27.2%`) |
| **`F4_StructuredYaml` (YAML Schema)** | **8.0 / 8** (`100%`) | **8.0 / 8 (`100%`)** | **8.00 / 8 (`100%`)** *(100% on `omnicoach`)* | **24.00 / 24 (`100.0%`)** | 7 steps / 17 calls | 903 KB (`39.6%`) |

### Per-Task Grading Matrix (`T1.1`–`T3.8`)

| Task | Project | `F1_Outline` | `F2_CompactBi` | `F3_AnnotatedStubs` | `F4_StructuredYaml` | Root Cause of Any Point Loss |
| :--- | :--- | :---: | :---: | :---: | :---: | :--- |
| **`T1.1`** | `omnicoach` | 1.0 | 1.0 | 1.0 | 1.0 | `F1` had to grep-invert outgoing edges across the file; `F2`–`F4` read `<-` / `used_by` directly. |
| **`T1.2`** | `omnicoach` | 1.0 | 1.0 | 1.0 | 1.0 | — |
| **`T1.3`** | `ripgrep_ignore` | 1.0 | 1.0 | 1.0 | 1.0 | — |
| **`T1.4`** | `ripgrep_ignore` | 1.0 | 1.0 | 1.0 | 1.0 | `F3` & `F4` additionally identified private methods returning `(IgnoreInner, Option<Error>)` via signatures. |
| **`T1.5`** | `cpython_zoneinfo` | 1.0 | 1.0 | 1.0 | 1.0 | — |
| **`T1.6`** | `cpython_zoneinfo` | 1.0 | 1.0 | 1.0 | 1.0 | — |
| **`T1.7`** | `cpython_urllib` | 1.0 | 1.0 | 1.0 | 1.0 | — |
| **`T1.8`** | `cpython_urllib` | 1.0 | 1.0 | 1.0 | 1.0 | — |
| **`T2.1`** | `cpython_zoneinfo` | **0.5** | 1.0 | 1.0 | 1.0 | `F1` lacks `end_line`; had to guess end of `_parse_tz_str` (`~703`) and could not determine end of file (`779`). |
| **`T2.2`** | `cpython_zoneinfo` | **0.5** | 1.0 | 1.0 | 1.0 | `F1` guessed `_ttinfo` end as `422` (actual `419` due to comments) and `_parse_tz_delta` as `756..EOF` (actual `756..779`). |
| **`T2.3`** | `cpython_zoneinfo` | **0.5** | 1.0 | 1.0 | 1.0 | `F1` guessed `_new_instance` end as `80` (actual `79`) and `__reduce__` end as `210/211`. |
| **`T2.4`** | `cpython_zoneinfo` | **0.5** | 1.0 | 1.0 | 1.0 | `F1` guessed `_ymd2ord` span as `584..591` (actual `584..590`) and `year_to_epoch` end from next item. |
| **`T2.5`** | `omnicoach` | **0.5** | 1.0 | 1.0 | 1.0 | `F1` guessed `parse_comment_text` end as `~381` (actual `378`) and `BracketedRules` end as `~502` (actual `501`) due to doc comments. |
| **`T2.6`** | `omnicoach` | **0.5** | 1.0 | 1.0 | 1.0 | `F1` could not see where `RuleStatus` (`297..300`) vs. `impl RuleStatus` (`..347`) or `Verdict` (`501..504`) ended. |
| **`T2.7`** | `ripgrep_ignore` | 1.0 | 1.0 | 1.0 | 1.0 | All 4 formats confirmed `0` findings in `gitignore.rs`. |
| **`T2.8`** | `cpython_urllib` | **0.5** | 1.0 | 1.0 | 1.0 | `F1` could not determine the end line of `_add_entry` (`91`), `_find_entry` (`173`), or `merge_entries` (`363`, guessed `~380`). |
| **`T3.1`** | `omnicoach` | **0.25** | **0.25** | 1.0 | 1.0 | `F1` & `F2` hallucinated `tracker.audit(...)` args (`(filename, source, lang, config)` / `(path, source, config)`) instead of `(path, config, suppressible_rules, evaluated_rules)`. |
| **`T3.2`** | `omnicoach` | **0.25** | **0.25** | 1.0 | 1.0 | `F1` & `F2` hallucinated `load_config(config_path, overrides)` (2 args instead of 1) and wrong arg order on `rule_status_with_overrides`, and lacked `RuleStatus` fields. |
| **`T3.3`** | `ripgrep_ignore` | 1.0 | **0.75** | 1.0 | 1.0 | `F2` omitted enum variant payloads of `IgnoreMatchInner` (`Override`, `Gitignore`, `Types`, `Hidden`). |
| **`T3.4`** | `ripgrep_ignore` | 1.0 | 1.0 | 1.0 | 1.0 | — |
| **`T3.5`** | `cpython_zoneinfo` | **0.75** | **0.75** | 1.0 | 1.0 | `F1` & `F2` guessed parameter name `fobj` instead of positional-only `file_obj, /` on `ZoneInfo.from_file(cls, file_obj, /, key=None)`. |
| **`T3.6`** | `cpython_zoneinfo` | 1.0 | 1.0 | 1.0 | 1.0 | — |
| **`T3.7`** | `cpython_urllib` | **0.5** | **0.5** | 1.0 | 1.0 | `F1` & `F2` missed `RuleLine` attributes `fullmatch` and `matcher` (present in `F3`/`F4` class attribute summaries `{ fullmatch, matcher, path, allowance }`). |
| **`T3.8`** | `cpython_urllib` | **0.75** | **0.75** | 1.0 | 1.0 | `F1` guessed `safe=""` and `F2` guessed `safe=b"/"` on `quote_from_bytes(bs, safe='/')`. |

---

## 7. Analysis of What Each Feature Contributes to Agent Performance

1. **Bidirectional Edges (`-> uses` and `<- used_by`) cut agent search steps by ~2× on impact analysis**:
   - In `F1_Outline` (outgoing edges only), every reverse-dependency question (`T1.1`–`T1.8`) forced the subagent to run global `grep_search` calls across the entire file to invert `-> Target` edges (`13` steps / `21` tool calls).
   - In `F2_CompactBi` (`6` steps / `11` tool calls), every declaration and module explicitly lists `<- caller[reason]`, turning reverse blast-radius queries into a single local read at the target symbol.
2. **Declaration End-Lines (`:start..end`) raise 1-step refactoring accuracy (`MC2`) from `56.3%` to `100.0%`**:
   - Without `end_line` (`F1_Outline`), an agent cannot know where a function/struct/class ends because doc comments, `impl` blocks, blank lines, or EOF sit between `line(u)` and `line(next_decl)`. As a result, `F1_Outline` was off by `1–17` lines on 7 of 8 `T2` refactoring tasks (`378` vs. `~381`, `419` vs. `422`, `363` vs. `~380`, `779` vs. `EOF`).
   - Adding `:start..end` (`F2`, `F3`, `F4`) enabled all three agents to compute exact cut spans and exact target insertion line numbers (`8.0 / 8.0`) in a single step without reading a single source file.
3. **Inline Signatures & Type Field Summaries (`F3_AnnotatedStubs`) raise Zero-Read API Synthesis (`MC3`) from `65.6%–68.8%` (`25%` on non-stdlib code) to `100.0%`**:
   - On well-known standard-library/open-source crates (`cpython`, `ripgrep`), agents under `F1_Outline` and `F2_CompactBi` relied on LLM pretraining memory and still got subtle signature details wrong (`file_obj, /` on `ZoneInfo.from_file`, `safe='/'` on `quote_from_bytes`, `fullmatch`/`matcher` on `RuleLine`).
   - On **project-specific code not in LLM pretraining (`omnicoach` `T3.1` and `T3.2`)**, `F1_Outline` and `F2_CompactBi` both collapsed to **`25%` (`0.5 / 2.0`)**, hallucinating parameter counts (`load_config` with 2 args instead of 1; `SuppressionTracker::audit` missing `suppressible_rules: &HashSet<&'static str>` and `evaluated_rules: &HashSet<&'static str>`), parameter order (`rule_status_with_overrides`), and struct fields (`RuleStatus`).
   - `F3_AnnotatedStubs` (`100%` on `T3`) completely eliminated these errors while adding only `+8.0%` bytes (`-21.0%` lines) compared to `F1_Outline`.
4. **Why `F3_AnnotatedStubs` Beats `F4_StructuredYaml`**:
   - Both `F3_AnnotatedStubs` and `F4_StructuredYaml` achieved **`24.0 / 24 (`100.0%`)`** across `T1`, `T2`, and `T3`.
   - However, `F4_StructuredYaml` requires **`45.6%` more bytes and `91.8%` more lines** (`903 KB / 13,028 lines` vs. `620 KB / 6,792 lines`) than `F3_AnnotatedStubs` to represent the exact same facts.

---

## 8. Final Stage C Recommendation

1. **Primary Agent Format — `F3_AnnotatedStubs` (`.stubs.md`)**:
   - **Use when**: An AI coding agent is performing general coding tasks (comprehension, blast-radius analysis, refactoring/placement planning, or writing/editing code that calls project APIs).
   - **Why**: Achieves **100% accuracy (`24 / 24`)** across all three task suites (`T1` impact analysis, `T2` 1-step refactoring, `T3` zero-read API synthesis) at **27.2% of raw source bytes** (`21%` fewer lines than the Stage B `.outline.md` baseline).
2. **Ultra-Compact Topological Mode — `F2_CompactBi` (`.compact.md`)**:
   - **Use when**: Context window budget is tightly constrained and the agent only needs architectural navigation, impact analysis (`T1`), or placement planning (`T2`) without synthesizing code calls (`T3`), or at `ProjectDepth::ModuleOnly` / `ProjectDepth::PublicSurface`.
   - **Why**: Achieves **100% accuracy on `T1` and `T2` (`16 / 16`)** with the fewest agent steps (`6` steps / `11` tool calls) and the smallest footprint (**21.4% of raw source bytes**, `-14.8%` bytes and `-38.8%` lines vs. `F1_Outline`). Actually, `F2_CompactBi` is strictly `F3_AnnotatedStubs` with the signature line omitted (`--no-signatures`).
3. **Retire `F1_Outline` and `F4_StructuredYaml`**:
   - `F1_Outline` is strictly dominated by `F2_CompactBi` (which is smaller in both bytes and lines yet adds bidirectional edges and `:start..end` spans) and by `F3_AnnotatedStubs` (which has fewer lines and raises overall accuracy from `75.0%` to `100.0%`).
   - `F4_StructuredYaml` matches `F3_AnnotatedStubs` in accuracy (`100.0%`) but wastes `+45.6%` more tokens/bytes on repetitive YAML schema keys.
