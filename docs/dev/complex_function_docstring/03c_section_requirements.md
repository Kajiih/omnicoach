# Docstring sections: when are Args: and Returns: required?

> **Status: VALIDATED** (lead & user, 2026-10-12; implemented as `unexplained-parameter` and `undocumented-tuple-return`).

Scope: when `undocumented-parameter` and `undocumented-return-value` ([03_design.md](03_design.md) D6, D10–D13, D16–D17) should require an `Args:` / `Returns:` / `Yields:` section, given the user's decisions so far: the nontrivial gate is `statements >= 12 OR cognitive >= 6` (D-gate1 in [03b_complexity_gate.md](03b_complexity_gate.md)), nested functions are exempt, one finding per function lists every missing parameter, and constructor arguments stored unchanged may be documented under the class `Attributes:`. Goal stated by the user: no undocumented non-obvious functions and no undocumented non-obvious arguments.

Markers: **[V]** measured here or read in a primary source; **[U]** not verified.

Google pyguide §3.8.3, the three sentences that matter: sections "can be omitted in cases where the function's name and signature are informative enough that it can be aptly described using a one-line docstring"; `Args:` entries "should include required type(s) if the code does not contain a corresponding type annotation"; for `Returns:`, "If the function only returns None, this section is not required. It may also be omitted if the docstring starts with Returns or Yields … and the opening sentence is sufficient to describe the return value."

## 1. Method

- **Corpora.** (1) google3: 2,649 non-test `.py` files from 410 top-level directories of `/google/src/files/head/depot/google3` (two seeded bounded random walks, at most 12 files per top-level directory; `third_party`, `experimental`, test files and test/testdata/examples directories excluded). (2) CPython 3.14 stdlib without tests (656 files). (3) abseil-py, flax, keras, optax from `google3/third_party/py` (877 non-test files). 39,694 functions after the 03b exclusions (test functions, `@overload`, `@override`, dunders other than `__init__`/`__new__`/`__call__`, stubs, property setters/deleters).
- **Not the 03b sample.** The 03b scripts were no longer in the previous investigator's scratch directory, so the sampler, the metrics (statements and cognitive complexity per D-gate2/D-gate3) and the docstring parser were re-implemented (`sample.py`, `collect.py`, `analyze.py`, `extra.py`, `extra2.py`, `inspect_sample.py` in this investigator's scratch directory). The sample is smaller than 03b's (2,649 vs 3,340 google3 files). Section rates agree with 03b F14 (google3 public 0.55–0.63, private 0.42–0.57 depending on size).
- **Populations.** "Args population": documented, non-nested functions other than `__init__` with at least one parameter besides `self`/`cls` and underscore-only names. "Returns population": documented, non-nested, non-property functions that return a value or yield (D11). `__init__` is left out because of the class-docstring placement (D14); see Q8.
- **Labels (proxy).** `Args`: the docstring has a Google `Args`-kind section, Sphinx `:param x:` or a NumPy `Parameters` section. `Returns`: a `Returns:`/`Yields:` section (or Sphinx/NumPy equivalent) **or** a summary starting with `Return(s)`/`Yield(s)` (D13). "Named": the parameter name appears as a word anywhere in the docstring.
- **Groups.** Public / private as in 03b (private = `_name`, member of a `_Class`, or in a `_module.py`).
- **Manual review.** 123 google3 functions in 11 buckets around the candidate rules, each classified W (documentation clearly warranted), B (borderline) or N (not needed), judged on what a caller needs to know that the name, signature and summary do not say.

**Proxy bias.** As in 03b: the label measures what reviewers accepted, and §3 shows that it mostly measures a file's or team's habit. **The stdlib does not use Google sections** (3 % of documented stdlib functions have an `Args` section), so stdlib numbers are only meaningful for the `Returns` label through the summary verb.

## 2. Findings

### Shape of real docstrings

**F1. google3 docstrings are bimodal: a one-line summary or full sections; prose without sections is rare [V].** Args population:

| group | n | one line | multi-line with Args | multi-line without Args | gate: one line | gate: multi-line without Args |
|---|---|---|---|---|---|---|
| google3 public | 5,941 | 0.44 | 0.55 | 0.01 | 0.40 | 0.01 |
| google3 private | 2,392 | 0.56 | 0.42 | 0.02 | 0.51 | 0.02 |
| oss public | 1,859 | 0.23 | 0.73 | 0.05 | 0.17 | 0.08 |
| oss private | 809 | 0.39 | 0.53 | 0.09 | 0.35 | 0.14 |
| stdlib public | 2,948 | 0.37 | 0.03 | 0.60 | 0.23 | 0.72 |

Of the gate-firing google3 functions with a docstring but no `Args` section, 97 % have a one-line docstring. In google3 the question "does this function need sections?" is therefore the question "is the one-line summary enough?", which is exactly Google's criterion. A multi-line docstring without `Args` (the literal violation of §3.8.3) affects 1.4 % (public) / 1.7 % (private) of documented google3 functions, and 98 % / 96 % of multi-line docstrings have an `Args` section.

**F2. One-line docstrings rarely name the parameters, least of all the non-obvious ones [V].** Share of parameters named anywhere in a docstring that has no `Args` section (google3 public / private): all 0.15 / 0.16; Optional or `None` default 0.07 / 0.07; bool 0.04 / 0.01; `**kwargs` 0.07 / 0.00; callable 0.11 / 0.19; unannotated 0.20 / 0.20. In the stdlib, whose docstrings are prose, 0.42–0.55 of parameters are named.

### What predicts a section

**F3. No code signal predicts an `Args` section [V].** AUC against the `Args` label (binary signals: section rate with / without the signal):

| signal | google3 public | google3 private | oss public | oss private |
|---|---|---|---|---|
| k = number of parameters | 0.57 | 0.51 | 0.57 | 0.61 |
| any unannotated parameter | 0.55 (0.62 / 0.51) | 0.61 (0.57 / 0.34) | 0.46 | 0.34 |
| any Optional / `None` default | 0.53 (0.64 / 0.53) | 0.50 (0.43 / 0.42) | 0.56 | 0.59 |
| any bool flag | 0.51 (0.64 / 0.54) | 0.50 (0.46 / 0.42) | 0.55 | 0.55 |
| any `*args` / `**kwargs` | 0.50 | 0.50 | 0.51 | 0.51 |
| any callable | 0.50 | 0.49 | 0.52 | 0.54 |
| any generic name (`data`, `x`, `obj`, `config`, `opts`, single letters, …) | 0.50 | 0.48 | 0.50 | 0.43 |
| any Optional / bool / callable / star | 0.54 | 0.50 | 0.57 | 0.61 |
| gate (D-gate1) | 0.53 (0.59 / 0.53) | 0.54 (0.47 / 0.39) | 0.52 | 0.49 |
| statements | 0.55 | 0.56 | 0.56 | 0.45 |

Section rate by k, google3 public, gate off / on: k=1 0.46 / 0.52, k=2 0.62 / 0.62, k=3 0.62 / 0.64, k=4 0.55 / 0.60, k=5 0.68 / 0.67, k≥7 0.51 / 0.65. Private: 0.38–0.45 for every k. The only consistent step is from k=1 to k≥2 (public).

**F4. No code signal predicts a `Returns` description either [V].** AUC against the `Returns` label, google3 public / private: missing annotation 0.53 / 0.59; Optional 0.50 / 0.49; tuple 0.50 / 0.50; Union 0.50 / 0.50; dict/Mapping 0.48 / 0.49; bool 0.49 / 0.53; Any/object 0.50 / 0.50; generator 0.50 / 0.50; ≥ 2 value returns 0.47 / 0.48; `None` on some path and a value on others 0.49 / 0.49; gate 0.47 / 0.50; statements 0.45 / 0.47. Literal shapes that differ between `return` statements are too rare to matter (< 0.5 %). Tuple returns are 7 % of value-returning documented functions and are described as often as the rest (0.66 vs 0.67 public).

**F5. The section habit belongs to the file and the team [V].** Using the section rate of the *other* documented functions of the same file as the predictor:

| group | Args: AUC / majority agreement | Returns: AUC / agreement | files (≥ 2 functions) all-or-nothing for Args |
|---|---|---|---|
| google3 public | 0.89 / 0.84 | 0.85 / 0.82 | 0.63 |
| google3 private | 0.87 / 0.81 | 0.81 / 0.76 | 0.61 |
| oss public | 0.86 / 0.82 | 0.86 / 0.82 | 0.56 |
| oss private | 0.80 / 0.76 | 0.76 / 0.75 | 0.50 |

Per top-level google3 directory (109 directories with ≥ 30 documented functions with parameters), the `Args` rate ranges from 0.04 to 0.98 (deciles 0.04, 0.21, 0.28, 0.36, 0.45, 0.51, 0.61, 0.69, 0.74, 0.80, 0.98). Inside mixed files only (files whose documented functions are partly sectioned), the gate does separate: section rate 0.62 with the gate vs 0.48 / 0.42 without (public / private); AUC of k 0.61 / 0.54.

Consequence: the proxy cannot calibrate *when* sections are needed. It says that Google engineers decide per file, not per function. The rule shapes below are therefore justified by the manual review (F9) and by the style guide's wording, with the proxy used only for volumes.

### Completeness of existing sections

**F6. Existing `Args` sections are complete; the current undocumented-parameter rule is quiet [V].** Share of parameters with an entry when an `Args` section exists, google3 public / private: ordinary 1.00 / 1.00; Optional 0.99 / 0.99; bool 0.99 / 0.99; callable 0.99 / 1.00; `*args` 0.95 / 1.00; `**kwargs` 0.97 / 0.50 (n = 2); unannotated 1.00 / 0.99. Functions with an incomplete section: 0.2 % / 0.3 % of documented google3 functions (oss 4.2 % / 1.4 %). Non-obvious parameters are documented as often as ordinary ones once a section exists, so a per-parameter variant ("only non-obvious parameters need entries in an existing section") would change nothing in google3. 8 sampled incomplete sections: 4 true omissions (two stale names after a rename, `weight` for `weights`, a forgotten new `debug` flag), 2 `unused_`-prefixed parameters, 2 artifacts of entries indented inconsistently (3 and 4 spaces in the same section).

**F7. Structured docstrings almost always describe the return value [V].** Value-returning functions whose docstring has an `Args`/`Raises` section but no `Returns`/`Yields` and no `Return…` summary: 0.1 % / 0.0 % of documented google3 functions (oss 5.1 % / 0.8 %). Of the 8 google3 cases, 3 are true omissions (a `Raises:`-only docstring, two `Args:`-only); 5 are malformed sections that a reader still understands (`Returns True if …` inline, `Returns` without colon, `Returns: x` on the header line).

### Cost of "always full sections for nontrivial functions"

**F8. "Gate implies full sections" flags 13–17 % of documented google3 functions; precision against the proxy is 0.47–0.63 [V].** Rule fire rate / proxy precision / findings (findings = share of the Args or Returns population that would be flagged):

| rule | google3 public | google3 private | oss public | oss private |
|---|---|---|---|---|
| Args required when gate | 0.37 / 0.59 / 15.4 % | 0.36 / 0.47 / 19.1 % | 0.28 / 0.75 / 6.9 % | 0.34 / 0.51 / 16.7 % |
| Args required when k ≥ 2 | 0.53 / 0.62 / 20.2 % | 0.53 / 0.44 / 29.8 % | 0.70 / 0.75 / 17.4 % | 0.68 / 0.59 / 27.7 % |
| Args required when k ≥ 4 | 0.16 / 0.60 / 6.5 % | 0.12 / 0.42 / 7.1 % | 0.28 / 0.79 / 5.9 % | 0.26 / 0.66 / 8.8 % |
| Args required when gate and k ≥ 2 | 0.23 / 0.63 / 8.7 % | 0.22 / 0.49 / 11.2 % | 0.23 / 0.77 / 5.4 % | 0.26 / 0.56 / 11.7 % |
| Args required when gate and k ≥ 4 | 0.09 / 0.63 / 3.4 % | 0.07 / 0.45 / 3.6 % | 0.14 / 0.83 / 2.3 % | 0.12 / 0.53 / 5.8 % |
| Args required when gate and any Optional/bool/callable/star | 0.11 / 0.64 / 4.2 % | 0.09 / 0.46 / 4.6 % | 0.19 / 0.80 / 3.7 % | 0.15 / 0.67 / 5.1 % |
| Returns required when gate | 0.34 / 0.63 / 12.6 % | 0.34 / 0.55 / 15.4 % | 0.25 / 0.72 / 7.0 % | 0.32 / 0.57 / 13.7 % |
| Returns required when gate and tuple | 0.04 / 0.70 / 1.2 % | 0.04 / 0.66 / 1.3 % | 0.03 / 0.77 / 0.8 % | 0.07 / 0.53 / 3.3 % |
| Returns required when gate and missing annotation | 0.11 / 0.69 / 3.4 % | 0.10 / 0.68 / 3.3 % | 0.18 / 0.71 / 5.3 % | 0.20 / 0.44 / 11.3 % |

Every candidate has the same proxy precision as the base rate (F3, F4), so the table is a volume table. Requiring `Returns:` on every gated function also contradicts §3.8.3's own exemption when the summary is a `Returns …` sentence that suffices: 25–32 % of one-line docstrings of value-returning google3 functions start with `Return(s)`.

### Manual review

**F9. Gate alone is a poor trigger; a non-obvious parameter named nowhere is a good one [V].** 123 google3 functions (each with a docstring):

| bucket | n | W | B | N | examples |
|---|---|---|---|---|---|
| A. gate, no Args, k = 1 | 12 | 1 | 2 | 9 | W: `poll_and_trigger(sender: TriggerSender \| None)`, `None` silently means test mode. N: `_is_command_allowed(command_line)`, a 17-branch `MapLanguageToSpellServerPort(language)`, `_extract_issue_id(output)` |
| B. gate, no Args, k = 2–3 | 12 | 3 | 5 | 4 | W: `Checkpointer.restore` (`to_restore=None` means latest step, returns a 3-tuple); `_resolve_experiment` (returns `None` if missing, `exp_map` holds `(parent, raw)` pairs); `_backfill_evalhub_runs_with_details(…, force_update)` returns a status dict. N: `_short_pdf_label(filename, max_stem_chars=18)`, `create_output_messages` |
| C. gate, no Args, k ≥ 4 | 12 | 6 | 4 | 2 | W: `analyze(df, …, direction, …)` (`direction` is `'higher'`/`'lower'`); four DataFrames of unstated columns; 4- and 3-tuple returns; `bug_val` sentinel `"BUG_ID"`. N: an RPC handler `(rpc, request, response, adaptor)`, `_run_command(command, *, check, env, max_attempts, backoff_seconds)` |
| D. no gate, no Args, k ≥ 4 | 8 | 1 | 2 | 5 | W: `cost_pq_ic(p_in, p_c, p_out, q, p, s, y, h)`, a formula with single-letter parameters. N: one-line delegations, gmon alert builders |
| E. gate, no Args, Optional/bool/callable/star parameter named nowhere | 15 | 8 | 5 | 2 | W: `find_recent_report_base_path(…, debug=False)` (`debug` switches the TTL directory); `_find_release_phase_node(…, name: str \| None = None)` (`None` matches nameless nodes, not "any"); `unpickle_signal(…, numpy=True)` selects the file format; `compute_deltas(curr, prev_state: dict \| None)` (`None` = first run). N: `_stage_backup(…, is_tarball, …, dry_run)`, `send_cl(…, ccs=None, bugs=None)` |
| F. as E, below the gate | 12 | 0 | 3 | 9 | N: `SetIsSeq(is_seq: bool)`, `get_agent(…, verbose=False)`, `_create_rpc(run_locally: bool)` |
| G. gate, value-returning, return mentioned nowhere | 15 | 3 | 4 | 8 | W: `get_invocation_details` returns `(inv_id, base_cl \| None, status \| "ERROR")`; `_resolve_review_skill_path` returns either a path or a human-readable refusal. N: `_IsEmptyCssBlockBody` ("Whether …"), `generate_all_challenge_items` ("Generates all … items") |
| H. below the gate, Optional / tuple / `None` + value, return mentioned nowhere | 10 | 3 | 6 | 1 | W: unannotated `(outs, padded_outs)`, a 5-tuple, `(peak_f, peak_a)` with units. B: `_parse_calendar_iso` returns `None` on empty or invalid input; `get_current_cl() -> int \| None` |
| I. tuple return, no Returns section, summary not "Return…" | 11 | 5 | 4 | 2 | W: `_prompt_interactive() -> tuple[str, str, int, list[str]]`, `write_combined_html(…) -> Tuple[str, str]`, `ExecuteCommand` returns `(lines, failed)`. N: `extract_x_y` ("Extracts x and y …"), `extract_scene_and_frame_ids` |
| J. existing Args section incomplete | 8 | 4 | 2 | 2 | see F6 (B: `unused_` parameters; N: parser artifacts) |
| K. structured docstring, no Returns | 8 | 3 | 0 | 5 | see F7 (N: malformed but readable sections) |

What made documentation warranted was always one of: a parameter whose *value* changes behavior in a way its name does not say (flags, `None` meaning something other than "absent", string enums, sentinels), an opaque or single-letter name, a positional result (tuple) or a result that mixes kinds (path or message, value or `None` on error), or an unstated data shape (DataFrame columns, dict keys). The number of parameters and the size of the body matter only because they raise the odds of one of these. Above the gate, a flag/optional/callable/star parameter that the docstring never names was warranted or borderline in 13 of 15 cases (E); below the gate, in 3 of 12 (F).

**F10. Unannotated parameters are not a usable trigger [V].** 36 % of documented google3 functions with parameters have an unannotated one (stdlib 94–96 %, oss 56–78 %). "Gate and any unannotated parameter named nowhere" would flag 4 % of documented google3 functions and 14–24 % in the stdlib; Google's sentence about types governs what an entry contains, not whether the section exists.

**F11. Volumes of the proposed shapes [V].** Share of all documented non-nested functions (other than `__init__`) flagged:

| rule | google3 public | google3 private | oss public | oss private | stdlib public | stdlib private |
|---|---|---|---|---|---|---|
| P1 incomplete Args section | 0.2 % | 0.3 % | 4.2 % | 1.4 % | 1.2 % | 0.6 % |
| P2 gate, no Args, non-obvious parameter named nowhere | 3.2 % | 3.7 % | 3.1 % | 4.1 % | 4.9 % | 6.9 % |
| P2 without the gate (rejected) | 6.7 % | 7.8 % | 9.0 % | 8.0 % | 18.5 % | 15.0 % |
| P3 gate, k ≥ 4, no Args | 2.7 % | 3.0 % | 2.1 % | 5.2 % | 3.3 % | 4.0 % |
| P3 with k ≥ 3 | 4.1 % | 5.7 % | 3.6 % | 7.3 % | 6.3 % | 8.6 % |
| P1 + P2 + P3 (undocumented-parameter + unexplained-parameter) | 4.4 % | 5.2 % | 7.6 % | 8.2 % | 7.6 % | 9.4 % |
| always full Args (gate, or incomplete section) | 12.4 % | 16.1 % | 10.3 % | 16.4 % | 18.9 % | 26.6 % |
| P4 structured docstring, no Returns | 0.1 % | 0.0 % | 5.1 % | 0.8 % | 0.5 % | 0.2 % |
| P5 tuple return, no Returns section, summary not "Return…" | 1.6 % | 2.0 % | 1.3 % | 5.0 % | 2.2 % | 7.0 % |
| P5 restricted to the gate | 0.8 % | 1.0 % | 0.6 % | 2.5 % | 1.1 % | 4.0 % |
| Optional return, docstring never says `None` (rejected) | 3.7 % | 3.3 % | 0.2 % | 0.6 % | 0.1 % | 0.2 % |
| P4 + P5 (undocumented-return-value) | 1.7 % | 2.1 % | 6.0 % | 5.8 % | 2.6 % | 7.3 % |
| always full Returns (gate, no Returns section or summary) | 9.0 % | 11.5 % | 5.9 % | 11.2 % | 11.4 % | 18.4 % |
| **all proposed (P1–P5)** | **5.8 %** | **6.9 %** | 12.7 % | 12.4 % | 9.5 % | 15.6 % |
| **all always-full** | **13.4 %** | **17.2 %** | 11.7 % | 17.1 % | 19.9 % | 28.0 % |

P2 parameter kinds flagged in google3 (gate): Optional / `None` default 409, bool 222, callable 23, `*args`/`**kwargs` 13.

Estimated manual precision (W, or W+B): P2 8/15 (13/15); P3 6/12 (10/12, half of the W because of the return value); P5 5/11 (9/11); P1 4/8 (6/8; 4/4 once the two changes in P1 remove the `unused_` and indentation cases); always-full Args, gate only, all k (buckets A–C pooled) 10/36 (21/36); always-full Returns (bucket G) 3/15 (7/15).

## 3. Proposals

Each rule keeps one antipattern and one message template. The D2 hand-off stays: no docstring is `undocumented-long-function`'s finding only.

- **P1. `undocumented-parameter`: an `Args` section that omits parameters, at any size (D6 unchanged), plus a missing section when the function is nontrivial and has many parameters (P3).** Keeps D10, D14, D16 and the user's `Attributes:` decision for constructors. Two additions from F6: (a) exempt `unused_`-prefixed parameters, as Google's pylintrc `ignored-argument-names` does [U, value of the option not re-read]; (b) the entry parser accepts entries at any indentation deeper than the header, with a continuation line recognized by being deeper than the previous entry, not by a fixed column (two of eight sampled findings were artifacts of 3- vs 4-space entries). Volume: 0.2–0.3 % of documented google3 functions; true omissions are renames and forgotten new parameters.
- **P2. New rule `unexplained-parameter`: a nontrivial function whose docstring has no `Args` section and never names a parameter whose value changes behavior.** "Behavior parameter" = annotated `Optional[…]` / `X | None` or defaulting to `None`; annotated `bool` or defaulting to `True`/`False`; annotated `Callable[…]`; `*args` / `**kwargs`. "Named" = the bare name (with or without stars or backticks) appears as a word anywhere in the docstring, so a summary such as "Restores … unless `dry_run` is set" is enough. Scope: D-gate1 holds (`statements >= 12 OR cognitive >= 6`), not nested, not `@override`; functions with an `Args` section belong to P1, so the two rules never fire together. One finding per function listing the parameters. Unannotated parameters are not a trigger (F10). Volume 3.2–3.7 % of documented google3 functions; manual 13/15 warranted or borderline. Below the gate the same check is mostly noise (F9 bucket F), hence the gate. Draft template:

  ```rust
  const TEMPLATE: ViolationTemplate = violation_template! {
      summary: "Docstring of `{function}` does not explain {parameters}.",
      rationale: "A flag, an optional value, a callback or a pass-through argument changes what the function does, and its name and type do not say how, such as what `None` means.",
      suggestion: "Explain {parameters} in an `Args:` section of the docstring of `{function}`, or in its summary.",
  };
  ```

  Why a separate rule and not a widening of P1: the antipattern differs (an inconsistent section vs. information missing from a one-line docstring), the fix differs (add one entry vs. decide to write sections or reword the summary), and the reasons differ (completeness vs. non-obviousness). Folding it into P1 is possible with the same template (Q1).
- **P3. Many parameters: a missing `Args` section in a nontrivial function with at least 4 parameters is an `undocumented-parameter` finding.** New option `min-parameters` (default 4, 0 disables), declared by `undocumented-parameter`. Same template as P1, listing every parameter (D16); D1 already treats a missing section and an incomplete one as the same antipattern. Counted: non-receiver parameters, `*args`/`**kwargs` included, underscore-only and `unused_` names excluded. Volume 2.7–3.0 % (k ≥ 3: 4.1–5.7 %). Evidence is weaker than for P2: no proxy support (F3), manual 10/12 warranted or borderline, but half of the W came from the return value. Alternative: drop P3 and rely on P2 (Q2).
- **P4. `undocumented-return-value`: a structured docstring that does not describe the return value, at any size (D6, D11–D13 unchanged).** A malformed but present description (`Returns True if …`, `Returns` without colon, `Returns: x` on the header line) counts as described; its form is another rule's business (F7). Volume ≈ 0.1 % in google3.
- **P5. Tuple results: `undocumented-return-value` also fires when the function returns a tuple of at least 2 positions and the docstring has no `Returns:`/`Yields:` section and its summary does not start with `Return(s)`/`Yield(s)`, at any size.** "Tuple" = return annotation `tuple[A, B, …]` / `Tuple[A, B, …]` with at least 2 items and no `...`, or, when unannotated, a `return` of a tuple display with at least 2 elements in the function's own body. Same template; the rationale already mentions what values signal. Volume 1.6–2.0 % (gate only: 0.8–1.0 %); manual 9/11 warranted or borderline, including small functions. Gate optional (Q3).
- **P6. Do not require full sections whenever the gate fires.** Compared with P1–P5: more than twice the volume in google3 (13.4–17.2 % vs 5.8–6.9 % of documented functions), proxy precision equal to the base rate (0.47–0.63, F8), manual precision 10/36 for `Args` and 3/15 for `Returns` (W only). It would also contradict §3.8.3's two explicit exemptions (a sufficient one-line docstring; a sufficient `Returns …` summary). This supersedes D-gate5 ("keep one gate") for the section rules: the gate stays a scope condition of P2, P3 and P5, not a requirement of full sections.
- **P7. Rejected triggers.** Optional return without "None" in the docstring (mostly borderline: "or `None` if missing" is the convention callers expect; 3.3–3.7 %); missing return annotation (3.3–3.4 % with the gate, no signal); dict/Mapping, Union, Any, bool, generator, several returns, mixed shapes (no signal, or < 1 % prevalence); generic or short parameter names (AUC 0.48–0.50 and very common: 11–14 % of google3 functions; single-letter formulas are a real but rare W, Q6); per-parameter entry requirements inside an existing section (F6: already 97–100 % complete).

### Rule summary after these proposals

| rule | antipattern | fires when | default volume (google3 public / private) |
|---|---|---|---|
| `undocumented-long-function` | nontrivial function without a docstring | unchanged (03b) | see 03b |
| `undocumented-parameter` | parameters left out of the docstring's parameter documentation | `Args` section exists and omits a parameter (any size); or no `Args` section, gate, and ≥ `min-parameters` (4) parameters | 2.9 % / 3.3 % |
| `unexplained-parameter` (new) | a behavior-changing parameter described nowhere | no `Args` section, gate, an Optional/bool/callable/star parameter never named | 3.2 % / 3.7 % (union with the row above 4.4 % / 5.2 %) |
| `undocumented-return-value` | a result whose meaning the docstring does not give | structured docstring without `Returns`; or tuple result without `Returns` section or `Return…` summary | 1.7 % / 2.1 % |

## 4. Open questions

- **Q1.** P2 as a separate rule `unexplained-parameter` (recommended: distinct antipattern and fix), or as a third trigger of `undocumented-parameter` with the same template? Name alternatives: `undocumented-flag-parameter`, `undocumented-optional-parameter`.
- **Q2.** Keep P3 (`min-parameters = 4`), lower it to 3 (+1.4 to +2.7 points), or drop it and rely on P2? The proxy gives no support for any k (F3); the manual review does for k ≥ 4, but partly through return values that P5 already covers for tuples.
- **Q3.** P5 at any size (recommended: small tuple-returning helpers were warranted too) or under the gate only (halves the volume)?
- **Q4.** Exempt `unused_`-prefixed parameters in `undocumented-parameter` and `unexplained-parameter`? Needs a check of Google's pylintrc `ignored-argument-names`.
- **Q5.** Should P2's "named" test also accept the name with underscores replaced by spaces ("if running locally" for `run_locally`)? Not measured; it lowers false positives on prose summaries at the cost of accidental matches on common words.
- **Q6.** Extend P2's behavior parameters with `Literal[…]`-annotated parameters (string enums) and with single-letter names in functions of ≥ 3 parameters? Both produced W examples (`direction`, `cost_pq_ic`) but were not measured.
- **Q7.** Should `unexplained-parameter` and P3 skip functions whose docstring defers to another (`"See KansasClient methods for docstring."`)? Seen once in the sample; Google's `@override` exemption does not cover it.
- **Q8.** Constructors were not measured here. Apply P2/P3 to `__init__` with the class docstring's `Args:` and `Attributes:` both counting as naming (user decision), and with the gate measured on `__init__`'s own body?
