# Greenfield Rust + Python Dogfooding Report: Building `adfontes` Under `omnicoach`

## TL;DR

- **End-to-End Scope**: Built [`adfontes`](https://github.com/Kajiih/adfontes) (`v0.1.0`: `19` Rust files / `19,628` LOC + `4` Python wheel-packaging and verification files / `452` LOC; `11`-component `define_architecture!` DAG + `Extract` submodule DAG) from scratch with `omnicoach` (`v0.4.1`) active across both interactive `PreToolUse` / `PreInvocation` lifecycle hooks and `58` CLI audit runs.
- **High-Signal Structural & Behavioral Alignment**: `adfontes` shipped with **zero `.omnicoach.toml` overrides** and **only `4` suppression directives** across `20,080` lines (`1` third-party callback signature, `2` `std::env::var` calls inside a single `from_env`-owned helper, and `1` file-level `repeated-literal` disable on a CLI/JSON-RPC wire-protocol smoke test). Across `515` distinct `(file, line, rule, message)` findings spanning `21` rules, `omnicoach` kept `Resolve` filesystem-free and `Stage` network-free, enforced `owns_external` crate encapsulation even in tests, decomposed `46` oversized or multi-phase tests into focused scenarios, and replaced `9` positional tuple signatures with named domain structs.
- **Four Concrete Rule Precision & UX Follow-Ups Surfaced**:
  1. **`primitive-duration` false positive on `_MIN` = minimum (`ASCII_VISIBLE_MIN: u8 = 0x20`)**: Lexical suffix matching treats `_min` (minimum bound paired with `_max`) as minutes (`std::time::Duration`).
  2. **`callee-before-caller` two-pass ordering friction on Rust private types (`89` findings, `17.3%`)**: Moving private structs/enums below private functions satisfies body-only usage (`RelationKind::Body`), then flips direction whenever a helper signature names that type (`fn build_tools_list_result() -> ToolsListResult`).
  3. **`repeated-literal` concentration on self-describing grammar & wire tokens (`193` findings, `37.5%`)**: Empirically confirms the open calibration item in [ROADMAP.md](../../../ROADMAP.md) §1 across HTML/XML/TeX parsers and JSON-RPC schemas (`"h1"`–`"h4"`, `"type"`, `"name"`, `"arxiv:"`, `"doi:"`, `0x20`, `4`).
  4. **`environment-variable-in-function` boundary-helper gap & `nested-function` contradiction**: Decomposing `ProxyConfig::from_env` into private module-level helper `first_non_empty_env` accounted for `2` of the `3` Rust `omni:ignore` directives in `adfontes`, and in Python directly collides with `[nested-function]` (which forbids the nested `def` exemption documented in `[environment-variable-in-function]`).

---

## 1. Repository Profile & Final Suppression Footprint

[`adfontes`](https://github.com/Kajiih/adfontes) is a standalone Rust (Edition 2024) CLI, newline-delimited JSON-RPC `2.0` stdio MCP server, and `cp310-abi3` PyO3 wheel for full-text academic and technical literature retrieval (`arXiv`, `DOI`, `PMCID`, `PMID`, IETF RFCs, HTML5, JATS/RFCXML, TeX archives, and lossless `lopdf` page slicing).

| Language | Tracked Source & Test Files | Lines of Code | Repo-Wide `.omnicoach.toml` Overrides | Inline `omni:ignore` Directives | File-Level `omni:disable-file` Directives |
| :--- | ---: | ---: | ---: | ---: | ---: |
| **Rust** (`src/`, `tests/`, `crates/adfontes-py/`) | `19` | `19,628` | `0` | `3` | `0` |
| **Python** (`crates/adfontes-py/python/`, `scripts/`) | `4` | `452` | `0` | `0` | `1` |
| **Total** | **`23`** | **`20,080`** | **`0`** | **`3`** | **`1`** |

All four surviving directives in `adfontes` `v0.1.0` correspond to genuine boundary or wire-protocol cases rather than internal workarounds:

1. `src/pdf.rs:288` — `// omni:ignore [positional-tuple-return] -- signature mandated by lopdf::FilterFunc` (external crate function-pointer type `(u32, u16)`).
2. `src/transport.rs:1243` & `1249` — `// omni:ignore [environment-variable-in-function] -- helper invoked exclusively from ProxyConfig::from_env` (two `std::env::var` reads inside `fn first_non_empty_env(primary: &str, fallback: &str) -> Option<String>`).
3. `scripts/verify_wheel.py:11` — `# omni:disable-file [repeated-literal] -- CLI arguments and JSON-RPC wire keys read clearest inline; constants would only restate them`.

---

## 2. Quantitative Diagnostic Distribution Across Greenfield Construction

Across `58` `omnicoach code` runs during the five-step greenfield build and review cycle, `omnicoach` reported **`515` distinct `(file, line, rule, message)` findings across `21` rules**, partitioned below by rule:

| Rule | Distinct Findings | Share of Total | Representative Trigger in `adfontes` |
| :--- | ---: | ---: | :--- |
| `repeated-literal` | `193` | `37.5%` | `"arxiv:"`, `"doi:"`, `"h1"`–`"h4"`, `"type"`, `"name"`, `0x20`, `4` |
| `callee-before-caller` | `89` | `17.3%` | Private structs/enums (`PinnedResolver`, `NoProxyRule`, `WalkStep`, `JsonRpcCall`) declared above callers |
| `abbreviated-name` | `49` | `9.5%` | Local Rust bindings `ch`, `err`, `num`, `ext`, `arch_source` |
| `uncolocated-helper` | `34` | `6.6%` | Private DOM helpers separated from `extract_html_to_markdown`'s cluster |
| `type-suffixed-name` | `26` | `5.0%` | `ver_str`, `safe_path_byte`, `page_dict`, `pages_map`, `pdf_str` |
| `too-many-assertions` | `24` | `4.7%` | Multi-scenario parser/protocol tests with `5`–`10` assertions |
| `multi-phase-test` | `22` | `4.3%` | Interleaved arrange-act-assert blocks in CLI dispatch and MCP tests |
| `escaped-multiline-string` | `19` | `3.7%` | Horizontal `\n`-escaped JSON-RPC frames and Markdown fixtures |
| `logic-in-tests` | `15` | `2.9%` | `for` loops and `if` branches inside `#[test]` bodies |
| `single-letter-name` | `13` | `2.5%` | `s` (`&str`), `b` (`u8`) outside closure/comprehension exemptions |
| `positional-tuple-return` | `8` | `1.6%` | `split_arxiv_version`, `split_no_proxy_rule`, `parse_fence_prefix` |
| `packed-assertion` | `6` | `1.2%` | Compound `assert!(a && b)` checks |
| `private-before-public` | `6` | `1.2%` | `clap` `CliArgs`/`CliCommand` and `SessionState` placed above `pub fn` entrypoints |
| `nullable-collection-return` | `2` | `0.4%` | `Option<Vec<String>>` (`try_pdftotext_pages`) where `None` $\neq$ `vec![]` |
| `repeated-index-access` | `2` | `0.4%` | `left[0]`, `left[1]` slice reads in `extract/tex.rs` (replaced with slice pattern matching) |
| `bare-multiline-string` | `2` | `0.4%` | Unindented raw multiline fixtures without `indoc!` |
| `primitive-duration` | `1` | `0.2%` | `const ASCII_VISIBLE_MIN: u8 = 0x20;` (false positive; see §4.1) |
| `positional-tuple-parameter` | `1` | `0.2%` | `active_fence: Option<(char, usize)>` in `extract/title.rs` |
| `unexplained-parameter` | `1` | `0.2%` | Missing `Args:` entry for `reference_binary` in `scripts/verify_wheel.py` |
| `specific-collection-parameter` | `1` | `0.2%` | `Sequence[str]` narrowed to `Iterable[str]` in `scripts/verify_wheel.py` |
| `unexplained-optional-argument` | `1` | `0.2%` | Docstring saying `"Optional"` without specifying `None` behavior |

---

## 3. High-Signal Wins Observed in Practice

1. **Compile-Time & Lint-Time Architectural Isolation (`define_architecture!` + `owns_external`)**:
   - Declaring `Resolve` without `Stage` and `Stage` without `Transport`/`Resolve` enforced clean I/O separation from the first commit—catching an early design defect during review where `fetch` validated `output_dir` confinement after network egress instead of through a pure pre-flight check.
   - `owns_external` encapsulation prevented `lopdf`, `ureq`, `scraper`, `roxmltree`, `flate2`, and `tar` types from leaking into sibling modules or unit tests (`Stage` tests construct fixtures via `crate::pdf::build_test_pdf` rather than importing `lopdf` directly).
2. **Elimination of Kitchen-Sink & Loop-Driven Tests (`too-many-assertions`, `multi-phase-test`, `logic-in-tests`, `packed-assertion`)**:
   - Coding agents naturally pack related scenarios into single `8`–`10`-assertion multi-phase tests (`dispatches_fetch_slice_pdf_and_mcp_using_injected_io`) or write `for` loops inside conformance tests (`tests/architecture_conformance.rs`).
   - Together (`82` findings across the four testing rules), `omnicoach` pushed those tests into parameterized `#[rstest]` cases, focused single-transition `#[test]` functions, and pure collection helpers asserted via a single `assert_eq!(violations, Vec::<String>::new())`—making failure output pinpoint exact cases immediately.
3. **Domain Structs Over Positional Tuples & Explicit `None` Semantics (`positional-tuple-*`, `nullable-collection-return`, `unexplained-optional-argument`)**:
   - Eight internal helper functions returning 2- or 3-tuples (`split_no_proxy_rule`, `parse_fence_prefix`, `find_largest_visible_element`) became self-documenting structs (`NoProxyHostPort`, `FencePrefix`), eliminating field-order ambiguity.
   - `nullable-collection-return`'s `Or:` explanation mechanism worked cleanly in zero retries on `try_pdftotext_pages(...) -> Option<Vec<String>>` (where `None` means `pdftotext` is unavailable/failed and triggers pure-Rust fallback, distinct from `Some(vec![])`).

---

## 4. Concrete Friction Points & Actionable Upstream Improvements

### 4.1 False Positive in `primitive-duration`: `_MIN` = Minimum Bound vs. Minutes

- **Observed Finding**: In `src/transport.rs`, `const ASCII_VISIBLE_MIN: u8 = 0x20;` (paired immediately with `const ASCII_VISIBLE_MAX: u8 = 0x7e;`) triggered:
  > `[primitive-duration] Name ASCII_VISIBLE_MIN ends with the time unit _MIN.`
  > `Suggestion: Rename ASCII_VISIBLE_MIN to ASCII_VISIBLE and type it as std::time::Duration...`
- **Root Cause**: [primitive_duration.rs](../../../src/code_lint/rules/primitive_duration.rs) matches the suffix `"_min"` case-insensitively on any non-type/non-callable binding without inspecting either (a) integer/byte/character range-bound semantics (`_MIN` / `_MAX` pairs, `u8` / `char` / `usize` bounds) or (b) stem tokens (`ASCII_VISIBLE_*`, `PAGE_*`, `PORT_*`). Unlike `"_mins"` or `"_minutes"`, bare `"_min"` is standard across Rust (`u8::MIN`, `*_MIN`) and Python (`*_MIN`) for *minimum*.
- **Candidate Fixes**:
  1. **Remove bare `"_min"` from `BANNED.default.base` (keep `"_mins"` and `"_minutes"`)**: Highest precision; `"_mins"` / `"_minutes"` unambiguously denote duration, whereas `"_min"` in `UPPER_SNAKE_CASE` constants overwhelmingly denotes a lower bound.
  2. **Scope or sibling-pair exemption**: If bare `"_min"` is retained for lowercase parameters (`timeout_min`), exempt `UPPER_SNAKE_CASE` constants paired with a sibling `*_MAX` binding in the same scope or non-duration byte/index types (`u8`, `i8`, `u16`, `usize`, `isize`, `char`).

### 4.2 Two-Pass Reordering Friction on Rust Private Types in `callee-before-caller`

- **Observed Pattern**: `callee-before-caller` ([callee_before_caller.rs](../../../src/code_lint/rules/callee_before_caller.rs), [ADR 011](../decisions/011_colocated_abstraction_ordering.md)) accounted for `89` findings (`17.3%`)—almost all on first-pass Rust modules where conventional Rust style places private `struct` and `enum` definitions (`PinnedResolver`, `NoProxyRule`, `WalkStep`, `CloseAction`, `JsonRpcCall`, `ToolCallResult`) near the top of the file above functions.
- **Why Agents Hit a Second Pass**:
  1. On pass 1, `omnicoach` flags private types placed at the top of the file that are used only inside function bodies (`Private JsonRpcCall is defined before the higher-level private handle_single_object: handle_single_object uses it only inside its body`).
  2. Moving a block of private wire/helper types down below the functions that construct them resolves body-only types, but immediately triggers the *reverse* `callee-before-caller` edge on any type named in a private helper's signature:
     > `Private build_tools_list_result is defined before the higher-level private ToolsListResult: its signature names ToolsListResult.`
- **Candidate UX Improvement**:
  - The underlying partial order in [ADR 011](../decisions/011_colocated_abstraction_ordering.md) is sound and deterministic (`caller fn` $\succ$ `signature-referenced private type` $\succ$ `helper fn naming that type` $\succ$ `body-only private type`), and `finding.anchor` already points to a valid line when moving one item. However, when multiple coupled types and helpers are out of order simultaneously, per-edge `Move X below Y` anchors shift across edits. Computing and surfacing a **cluster-level topological target order** (or grouping co-moving cluster violations into a single cluster-order hint when $\ge 3$ items in one cluster violate stepdown order) would collapse two-pass manual shuffling into a single deterministic edit.

### 4.3 `repeated-literal` Concentration on Parser Grammar & Wire-Schema Tokens

- **Observed Pattern**: `repeated-literal` ([repeated_literal.rs](../../../src/code_lint/rules/repeated_literal.rs)) produced `193` findings (`37.5%` of all diagnostics)—more than the next three rules combined. While ~`20%` caught genuine synchronization hazards (duplicated error messages such as `"RFC number must be between 1 and 99999"` and `"tar entry path is not valid UTF-8"`), ~`80%` fired on fixed external specification tokens appearing `2`–`4` times in one parser or protocol module:
  - HTML/XML/TeX grammar tags and prefixes: `"h1"`, `"h2"`, `"h3"`, `"h4"`, `"\\begin{document}"`, `"\\item"`, `"content-encoding"`.
  - Identifier schemes and wire-schema keys: `"arxiv:"`, `"doi:"`, `"10."`, `"type"`, `"name"`, `"text"`.
  - Small radix/length/ASCII bounds: `3`, `4`, `0x20`.
- **Takeaway for `ROADMAP.md` §1 (`repeated-literal` follow-ups)**: Real-world parser and protocol modules strongly support raising the default string/number occurrence floor (`min-occurrences = 3`, matching SonarSource RSPEC-1192 and `goconst`) or separating `min-string-occurrences` / `min-string-len` from magic numbers so two-site specification tokens (`match` + test or serializer + schema) do not force tautological `const TAG_H2: &str = "h2";` constants.

### 4.4 `environment-variable-in-function` Boundary-Helper Gap & `nested-function` Contradiction

- **Observed Pattern**: Two of the only three Rust `omni:ignore` directives in `adfontes` occurred inside `fn first_non_empty_env(primary: &str, fallback: &str) -> Option<String>` (`src/transport.rs:1243, 1249`), a 13-line private helper called **exclusively** by `ProxyConfig::from_env()` to check uppercase/lowercase proxy environment pairs (`HTTPS_PROXY` / `https_proxy`, `NO_PROXY` / `no_proxy`).
- **Two Compounding Issues**:
  1. **Missing intra-file `ScopeGraph` ownership propagation + cross-rule contradiction in Python**: [environment_variable_in_function.rs](../../../src/code_lint/rules/environment_variable_in_function.rs) exempts functions named `from_env`, `from_environ`, `load_env`, and top-level `main`, plus *"functions and closures nested in them"*. However:
     - In **Python**, [nested_function.rs](../../../src/code_lint/rules/nested_function.rs) (`[nested-function]`) explicitly **bans** nested `def` helpers (`Suggestion: Extract {function} to a module-level private function`), creating a direct Omni $\leftrightarrow$ Omni fix-conflict cycle ([docs/dev/rule_dependencies/03_design.md](../rule_dependencies/03_design.md)): nesting a helper inside `from_env` triggers `[nested-function]`, and extracting it to module level triggers `[environment-variable-in-function]`.
     - In **both Python and Rust**, private module-level helpers whose transitive intra-file `ScopeGraph` roots consist *exclusively* of exempt configuration-boundary functions (`from_env`, `from_environ`, `load_env`, `main`) cannot leak hidden environment reads to non-boundary callers. Exempting private helpers exclusively owned by boundary roots eliminates both the Python rule conflict and two-thirds of `adfontes`'s Rust suppressions.
  2. **Call-site span anchoring requires multiple `omni:ignore` comments in one helper**: Because `[environment-variable-in-function]` anchors diagnostics on each `std::env::var(...)` call expression rather than the enclosing `fn` header (mirroring the `[logic-in-tests]` / `[sleep-in-tests]` body-span note in [ROADMAP.md](../../../ROADMAP.md) §4), a single `// omni:ignore [environment-variable-in-function]` above `fn first_non_empty_env` triggers `[unused-suppression]` instead of covering the helper's two `std::env::var` calls.

### 4.5 Naming Friction Worth Evaluating (`abbreviated-name` & `type-suffixed-name`)

- **`abbreviated-name` (`49` findings, `9.5%`)**: Flagging `err`, `num`, `ext`, and `arch_source` was trivial to resolve (`error`, `number`, `extension`, `architecture_source`). The highest-frequency Rust friction point was `ch` (`char` loop/iterator variable in character scanners), which in Rust avoids colliding with the reserved keyword `char` (`for character in ...` works, though `ch` is universal in `rustc` and the standard library).
- **`type-suffixed-name` (`26` findings, `5.0%`)**: Flagged `_dict` and `_map` cleanly on `lopdf::Dictionary` / `BTreeMap` bindings (`page_dict` $\to$ `page`, `pages_map` $\to$ `pages`). The one recurring Rust awkwardness is `_str` when converting `path: &Path` via `let Some(path_str) = path.to_str()` in the same scope without shadowing `path: &Path` when `path` is still needed below as `&Path`.

---

## References

- [SonarSource RSPEC-1192 — *String literals should not be duplicated*](https://rules.sonarsource.com/python/RSPEC-1192/) — Establishes the default 3-occurrence and minimum-length thresholds used in production static analyzers to avoid tautological constant extraction on two-site tokens.
- [Robert C. Martin — *Clean Code*, Chapter 5 (*The Stepdown Rule*)](https://www.oreilly.com/library/view/clean-code-a/9780136083238/) — Defines top-down caller-before-callee declaration ordering enforced by `callee-before-caller` and formalized with signature-vs-body type bounds in [ADR 011](../decisions/011_colocated_abstraction_ordering.md).
- [Murphy, Notkin & Sullivan — *Software Reflexion Models: Bridging the Gap between Source and High-Level Models* (ACM SIGSOFT FSE 1995)](https://doi.org/10.1145/222124.222136) — Establishes declarative component boundary conformance verified across `adfontes`'s 11-component `define_architecture!` DAG.
- [`Kajiih/adfontes` (`v0.1.0`)](https://github.com/Kajiih/adfontes/tree/v0.1.0) — Public Rust + PyO3 workspace evaluated in this dogfooding report.
