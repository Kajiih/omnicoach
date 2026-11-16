# Double-Blind `RuleDoc` Boundary Documentation & Taxonomy Benchmark

## TL;DR

- **Taxonomy Audit (`83` rules)**: Every rule previously classified as `Precision::Heuristic` was also `Consensus::Opinionated` ($\text{Heuristic} \subsetneq \text{Opinionated}$), while `55%` of `Precision::Exact` rules documented static-analysis blind spots in `known_problems` and `93%` of `Opinionated` rules lacked structured guidance on when the flagged construct is intentional.
- **Double-Blind Benchmark (`16` trials, `160` graded items)**: Adding `when_to_allow: Option<&'static str>` (`## When to allow`) ahead of `known_problems: Option<&'static str>` (`## Known problems`) and removing the redundant `Precision` facet raised combined Development and Holdout RICR alignment from **`91.2%`** (`Known Problems Only` baseline) to **`98.8%`**, raised multi-site rule-option configuration (`[rules.<rule>]`) from **`25%` to `100%`**, and reduced Holdout output tokens by **`-16%`** over the baseline (`6,923` vs. `8,274`) and **`-23%`** over a single merged section (`6,923` vs. `8,979`).

---

## 1. First-Principles Audit of `Precision` vs. `Consensus` & `RuleDoc`

Auditing the full 83-rule catalog (`76` code rules, `4` suppression rules, `3` command rules) exposed three structural problems with the `Precision` (`exact` / `heuristic`) facet ([02_references_and_corpus_split.md](02_references_and_corpus_split.md) §6):

| `Precision` \ `Consensus` | `Opinionated` (`67`) | `Unopinionated` (`16`) | Total (`83`) | Has `known_problems: Some(...)` |
| :--- | :---: | :---: | :---: | :---: |
| **`Exact`** | `37` | `16` | `53` | `29 / 53` (`54.7%`) |
| **`Heuristic`** | `30` | **`0`** | `30` | `27 / 30` (`90.0%`) |

1. **`Precision` Is Extensionally Redundant for Footer Gating**: No rule in the catalog is both `Heuristic` and `Unopinionated`. Because every heuristic rule is also `Consensus::Opinionated` ($\text{Heuristic} \subsetneq \text{Opinionated}$), gating `SUPPRESSION_NOTE` on `Consensus::Opinionated` produces the exact same diagnostic output as gating on `Precision::Heuristic || Consensus::Opinionated`.
2. **`Precision::Exact` Does Not Mean Zero False Positives**: Over half (`55%`) of `Precision::Exact` rules document static-analysis false positives or false negatives in `known_problems` (for example, `repeated-literal` on imported `TypedDict` keys, `repeated-index-access` on `dict[int, str]` lookups, and `annotated-type-in-docstring` on Sphinx docstrings). Conversely, three `Precision::Heuristic` rules (`abbreviated-name`, `escaped-multiline-string`, `type-suffixed-name`) declared `known_problems: None`.
3. **Conflation of Domain Exceptions and Static-Analysis Blind Spots**: Five rules (`multi-phase-test`, `annotated-type-in-docstring`, `positional-tuple-attribute`, `positional-tuple-parameter`, `positional-tuple-return`) mixed legitimate domain exceptions (such as stateful lifecycle tests or symmetric `(min, max)` bounds) into `known_problems` alongside parser and type-alias blind spots, while `93%` of `Opinionated` rules had no structured field stating when the flagged construct is intentional.

---

## 2. Evaluated `RuleDoc` Designs

Four compiled binary cohorts were evaluated double-blind (`2` replicates per cohort on the Development split and `2` replicates per cohort on the Holdout split; `16` isolated subagent trials total):

| Cohort | `RuleDoc` Model | `--explain` Section Heading(s) | `--explain` Pointer Footer |
| :--- | :--- | :--- | :--- |
| **Known Problems Only (Baseline)** | `known_problems: Option<&'static str>` only | `## Known problems` (static-analysis blind spots only; no domain exceptions) | `For details on a rule (including known limitations), run: omnicoach --explain <rule>...` |
| **Merged Limitations & Exceptions** | Single prose field combining limitations and exceptions | `## Known limitations and exceptions` | `For details on a rule (including known limitations and exceptions), run: omnicoach --explain <rule>...` |
| **Separate When to Allow & Known Problems** | `when_to_allow: Option<&'static str>` + `known_problems: Option<&'static str>` | `## When to allow` (domain/boundary/config exceptions) + `## Known problems` (static-analysis blind spots) | `For details on a rule (including when to allow and known problems), run: omnicoach --explain <rule>...` |
| **Separate Exceptions & Known Problems** | `exceptions: Option<&'static str>` + `known_problems: Option<&'static str>` | `## Exceptions` + `## Known problems` | `For details on a rule (including exceptions and known problems), run: omnicoach --explain <rule>...` |

In all three treatment cohorts, `Precision` (`exact` / `heuristic`) was removed from `## Tags`, and `SUPPRESSION_NOTE` ordered `[rules.<rule>]` before `per-file-ignores`.

---

## 3. Double-Blind Benchmark Scenarios (`20` Graded Items per Replicate Pair)

Each trial ran an isolated subagent on a four-module Python repository (`10` graded items on the Development split; `10` graded items on the Holdout split) where every module paired an internal defect with a boundary, domain, or static-analysis false-positive trap:

### Development Split (`8` rules, `10` graded items per trial)

1. **Domain Naming vs. Positional Boundary (`geometry.py` — `[single-letter-name]` + `[identical-positional-types]`)**:
   - `Quaternion` uses domain-standard `w, x, y, z` across `6` sites (`x` is allowed by default; `w, y, z` are flagged) $\to$ configure `[rules.single-letter-name]` `extend-allowed = ["w", "y", "z"]` (not `per-file-ignores`, which would hide `_pad_label(s, n)`).
   - `_pad_label(s, n)` $\to$ rename single-letter local parameters to descriptive names.
   - `lerp_scalar(start: float, end: float, weight: float)` (public API with external positional callers) $\to$ single-site `# omni:ignore[identical-positional-types] -- <reason>`.
   - `_clamp_range(value: float, lower: float, upper: float)` (internal helper) $\to$ add `*,` keyword-only separator and update internal call sites.
2. **Imported `TypedDict` Keys vs. Mutable Return Contracts (`telemetry.py` — `[repeated-literal]` + `[mutable-collection-return]` + `[mutable-collection-attribute]`)**:
   - `TypedDict` `"schema_version"` string literal subscript keys (`frame["schema_version"]`, required by PEP 589 to remain string literals) $\to$ keep literal keys and add `# omni:ignore[repeated-literal] -- <reason>` (converting to `frame[SCHEMA_VERSION_KEY]` breaks `mypy` and `pyright` `TypedDict` indexing).
   - `"https://telemetry.internal/v2/ingest"` repeated in two functions $\to$ extract module constant.
   - `build_mutable_headers() -> MutableMapping[str, str]` and `MetricCollector.active_tags: MutableSet[str]` (mutated by callers outside the module) $\to$ keep `MutableMapping` / `MutableSet` and satisfy `Or:` with an explanation comment.
3. **Idempotent Cleanup vs. Dynamic Hook Dispatch (`plugin_loader.py` — `[suppressed-exception]` + `[dynamic-attribute-access]`)**:
   - `with suppress(FileNotFoundError): pid_file.unlink()` $\to$ keep `suppress(FileNotFoundError)` and add an explanation comment (`Or:`).
   - `with suppress(Exception): raw = json.loads(...)` $\to$ replace `suppress(Exception)` with explicit `json.JSONDecodeError` handling.
   - `getattr(plugin, hook_name)` (dynamic hook dispatch) $\to$ single-site `# omni:ignore[dynamic-attribute-access] -- <reason>`.
   - `getattr(config, "timeout_seconds")` (static dataclass field) $\to$ replace with `config.timeout_seconds`.
4. **Optional Sentinel Contracts (`docstrings.py` — `[unexplained-optional-argument]` + `[unexplained-optional-return]`)**:
   - Document what `timeout=None` and `headers=None` do and when `find_route` returns `None` (without gaming the rule by deleting `Optional` or `or None`).

### Holdout Split (`8` rules, `10` graded items per trial)

1. **Wire-Locked Suffixes vs. Integer-Keyed Mapping Lookups (`wire_v1_schema.py` — `[type-suffixed-name]` + `[repeated-index-access]`)**:
   - `WireV1Envelope` has `6` wire-locked `_list` / `_dict` fields (`tags_list`, `headers_dict`, ...) $\to$ configure `[rules.type-suffixed-name]` `remove-banned = ["_list", "_dict"]` (not `per-file-ignores`, which would hide `normalize_tags(raw_str)`).
   - `normalize_tags(raw_str)` $\to$ rename `raw_str` to `raw_tags` and `out_list` to `cleaned`.
   - `format_tier_summary(tier_labels: Mapping[int, str])` reading `tier_labels[1]`, `tier_labels[2]`, `tier_labels[3]` (`Mapping` with integer keys, where sequence unpacking `t1, t2, t3 = tier_labels` unpacks keys `1, 2, 3` instead of values) $\to$ keep key lookups and add `# omni:ignore[repeated-index-access] -- <reason>`.
   - `parse_csv_row(row: tuple[str, str, str])` reading `row[0], row[1], row[2]` $\to$ unpack `host, port, proto = row`.
2. **Caller-Sorted Return Containers vs. Read-Only Attributes (`collections_api.py` — `[concrete-collection-parameter]` + `[concrete-collection-return]` + `[concrete-collection-attribute]`)**:
   - `merge_batches(batches: list[str]) -> list[str]` (read-only parameter; callers call `.sort()` on returned list) $\to$ widen `batches` to `Iterable[str]`, keep `-> list[str]` with `# omni:ignore[concrete-collection-return] -- <reason>`.
   - `AuditRecord.entries: list[str]` (read-only frozen dataclass attribute) $\to$ widen to `Sequence[str]`.
3. **Stateful Protocol Sessions vs. Non-Equatable Assertion Targets (`test_protocol.py` — `[logic-in-tests]` + `[too-many-assertions]` + `[sleep-in-tests]`)**:
   - `test_batch_decoding()` (`for raw, expected in cases:`) $\to$ refactor to `@pytest.mark.parametrize`.
   - `test_handshake_state_machine()` (stateful four-step protocol session stepping one `Session()` through `CONNECT -> AUTH -> READY -> CLOSED`) $\to$ keep single stateful test and suppress `[logic-in-tests]` with `# omni:ignore[logic-in-tests] -- <reason>`.
   - `test_frame_header_fields()` (`6` field assertions on `FrameHeader`, which raises `TypeError` on `==`) $\to$ keep field assertions and suppress `[too-many-assertions]` with `# omni:ignore[too-many-assertions] -- <reason>`.
   - `test_monotonic_timer_measures_elapsed_wall_time()` (tests OS monotonic clock measurement) vs. `test_retry_backoff_schedule()` (calls `time.sleep(0.01)` in a retry test) $\to$ suppress `time.sleep` in the monotonic clock test and use `FakeClock` in the retry test.
4. **Stateful Cursors vs. Repeated Test Setup (`cursor_state.py` — `[mutable-dataclass]` + `[duplicated-test-setup]`)**:
   - `StreamCursor` (stateful cursor mutating `offset` and `line` in `advance()`) vs. `TokenSpan` (immutable value object) $\to$ `@dataclass(frozen=False, slots=True)` on `StreamCursor` and `@dataclass(frozen=True, slots=True)` on `TokenSpan`.
   - `test_cursor_steps_*` (`3` sibling tests duplicating `4` setup lines) $\to$ extract shared setup helper.

---

## 4. Empirical Results (`16` Subagent Trials, `160` Graded Items)

| Cohort | Dev RICR (`Replicate 1` / `2`) | Holdout RICR (`Replicate 1` / `2`) | Combined RICR (`40` items) | Multi-Site `[rules.<rule>]` Adoption | Mean Turns (Dev / Holdout) | Mean Output Tokens (Dev / Holdout) |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: |
| **Known Problems Only (Baseline)** | `90.0%` (`90%` / `90%`) | `92.5%` (`90%` / `95%`) | **`91.2%`** (`36.5 / 40`) | `25%` (`1 / 4`) | `10.0` / `10.5` | `6,342` / `8,274` |
| **Merged Limitations & Exceptions** | `95.0%` (`100%` / `90%`) | `97.5%` (`100%` / `95%`) | **`96.2%`** (`38.5 / 40`) | `100%` (`4 / 4`) | `9.5` / `10.0` | `5,834` / `8,979` |
| **Separate When to Allow & Known Problems** | **`100.0%`** (`100%` / `100%`) | **`97.5%`** (`95%` / `100%`) | **`98.8%`** (`39.5 / 40`) | **`100%`** (`4 / 4`) | `9.5` / `9.5` | `5,863` / `6,923` |
| **Separate Exceptions & Known Problems** | `95.0%` (`90%` / `100%`) | `97.5%` (`95%` / `100%`) | **`96.2%`** (`38.5 / 40`) | `75%` (`3 / 4`) | `10.0` / `9.0` | `6,331` / `7,351` |

### Key Empirical Findings

1. **`RuleDoc` Boundary Documentation Eliminates Config Over-Broadening (`25%` $\to$ `100%` on Multi-Site Rule Options)**:
   - Under **Known Problems Only (Baseline)**, three of four trials across Development and Holdout used file-wide `per-file-ignores` on `geometry.py` (`"geometry.py" = ["single-letter-name"]`) and `wire_v1_schema.py` (`"wire_v1_schema.py" = ["type-suffixed-name"]`) instead of `[rules.single-letter-name]` `extend-allowed = ["w", "y", "z"]` and `[rules.type-suffixed-name]` `remove-banned = ["_list", "_dict"]`.
   - Under **Separate When to Allow & Known Problems**, `100%` of trials across Development and Holdout ran `omnicoach --explain` and configured `[rules.single-letter-name]` `extend-allowed = ["w", "y", "z"]` and `[rules.type-suffixed-name]` `remove-banned = ["_list", "_dict"]`, preserving lint coverage on the internal defects (`_pad_label(s, n)` and `normalize_tags(raw_str)`) in the same files.
2. **Why Separate Sections (`## When to allow` + `## Known problems`) Outperform a Single Merged Section (`## Known limitations and exceptions`)**:
   - On `telemetry.py` (`[repeated-literal]` on PEP 589 `TypedDict` string literal keys), the second Development replicate of **Merged Limitations & Exceptions** scanned the first bullet of `## Known limitations and exceptions` (*"Coincidentally equal literals that represent independent domain concepts: define separate named constants..."*), stopped reading before the third bullet (*"Keys of a Python `TypedDict` imported from another module... are not resolved"*), and replaced `frame["schema_version"]` with `frame[SCHEMA_VERSION_KEY]`—breaking PEP 589 `TypedDict` static typing.
   - Separating `## When to allow` (normative domain, boundary, and configuration exceptions) from `## Known problems` (descriptive static-analysis blind spots) prevents action-oriented readers from stopping after the first domain-exception bullet and missing a static-analysis false-positive warning, while enforcing at declaration time in `RuleDoc` that rule authors classify each note as either an intentional domain exception (`when_to_allow`) or an analyzer limitation (`known_problems`).
   - On Holdout, **Separate When to Allow & Known Problems** also used **`-16%` fewer output tokens than Known Problems Only (Baseline)** (`6,923` vs. `8,274`) and **`-23%` fewer output tokens than Merged Limitations & Exceptions** (`6,923` vs. `8,979`).

---

## 5. Next Steps

1. **Adopted Design ([ADR 007](../decisions/007_rule_taxonomy_and_selection.md), [ADR 008](../decisions/008_rule_documentation_and_discovery.md))**: `RuleDoc::when_to_allow` (`## When to allow`) and the removal of `Precision` from `Classification`, `Facet`, and `Tag` are promoted into [src/rule_declaration/](../../../src/rule_declaration.rs), [src/rule_selection/](../../../src/rule_selection.rs), and [src/rule_catalog.rs](../../../src/rule_catalog.rs).
2. **Remaining Coaching Follow-ups ([ROADMAP.md](../../../ROADMAP.md) §4)**:
   - Document top-level `per-file-ignores` in `omnicoach --explain <rule>`'s `## Configuration` section for rules without per-rule allowlist options.
   - Evaluate per-rule `ViolationTemplate` boundary qualifiers (Toulmin *rebuttals*) on opinionated naming and literal rules that fire on conversion constants or wire fields.
