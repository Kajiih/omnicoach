# Complexity gate for the docstring rules: calibration

> **Status: PENDING.** Decision proposals for the lead and the user. Nothing here changes the repository code.

Scope: the "nontrivial function" gate used by `undocumented-long-function`, `undocumented-parameter` and `undocumented-return-value` ([03_design.md](03_design.md) D4–D6, D17, Q1). The draft gate was "code lines >= 20". The user prefers complexity over length; this document checks the earlier proposal (cognitive complexity >= 5 OR statements >= 12) against primary sources and real code.

Markers: **[V]** verified in a primary source or measured here; **[U]** not verified.

## 1. Method

- **Sources.** SonarSource white paper "Cognitive Complexity" v1.7 (29 August 2023, PDF); SonarSource's own Python implementation (`sonar-python`, `CognitiveComplexityVisitor.java`, main branch); complexipy 8.0.1 (Rust, `ruff_python_ast`, file `crates/complexipy-core/src/cognitive_complexity.rs` and `utils.rs`); `cognitive_complexity` 1.3.0 used by flake8-cognitive-complexity 0.1.0; Ruff `mccabe/rules/function_is_too_complex.rs` and `pylint/helpers.rs::num_statements` (main branch); Google `styleguide` `pylintrc` (gh-pages); Muñoz Barón, Wyrich, Wagner, ESEM 2020 (arXiv 2007.12520).
- **Corpora.** (1) monorepo: 3,340 non-test `.py` files sampled from 409 top-level directories of `<monorepo>` (bounded random walk, at most 12 files per top-level directory; `third_party`, `experimental`, test files and test/testdata/examples directories excluded); 3,329 parsed. (2) CPython 3.14 stdlib without `test/` and `idlelib/idle_test/` (656 files). (3) Four Google-style open-source projects: abseil-py, flax, keras, optax (1,540 files). Population: every `def` (functions, methods, nested functions), minus test functions, `@overload`, `@override`, dunders and stubs (body only `pass` / `...` / `raise`). 46,641 functions.
- **Metrics per function.** Defined in §3. Computed with Python `ast` (scripts in the investigator's scratch directory: `metrics.py`, `analyze.py`, `extra.py`, `compare_cx.py`, `inspect_sample.py`; tables in `out/report.md` and `out/extra.md`).
- **Proxy label.** "Has a docstring" (`doc`) and "docstring has an `Args:` / `Returns:` / `Yields:` section or a Sphinx/NumPy equivalent" (`sec`, among functions with a parameter or a return value). Measured on **private** functions (name starts with `_`, nested in a function, member of a private class, or in a `_module.py`), where Ruff D1xx does not apply.
- **Manual review.** 39 undocumented private monorepo functions in five buckets around the thresholds, each classified "docstring clearly warranted" (W), "borderline" (B) or "not needed" (N).

**Proxy bias.** The label measures what reviewers *accepted*, not what readers *needed*. Documented functions include ones documented by habit (whole-file style, generated templates); undocumented ones include debt. The label rewards metrics that track habit, which is mostly size. Only monorepo shows a usable relation between any metric and the label (stdlib and the open-source projects document private functions at 31–34 % with almost no dependence on size or complexity, AUC 0.55–0.60), so the calibration rests on monorepo. The ESEM result (F6) is the only evidence about comprehension itself, and it is for small Java snippets.

## 2. Findings

### Primary sources

**F1. White paper increments [V].** Three kinds of increment (white paper pp. 6–8 and Appendix B):

| Rule | Constructs |
|---|---|
| B1 increment (+1) | `if`, `else if`, `else`, ternary; `switch` (once for all cases); `for`, `foreach`; `while`, `do while`; `catch` (once per clause, whatever the number of types); `goto LABEL`, `break`/`continue` to a label or number; each sequence of like binary logical operators; each method in a recursion cycle (direct or indirect) |
| B2 raises nesting | `if`, `else if`, `else`, ternary, `switch`, loops, `catch`, nested methods and method-like structures such as lambdas |
| B3 nesting increment (+nesting level) | `if`, ternary, `switch`, loops, `catch` (not `else if` / `else`: "the mental cost has already been paid when reading the if") |

Explicitly ignored: `try` and `finally` blocks, null-coalescing operators, early `return`, plain `break`/`continue`, the method itself. Lambdas and nested methods get no structural increment but raise the nesting level. Python decorator exception (Appendix A): a function whose body is only a nested function and a `return` does not raise the nesting of the nested function; the exception chains (`decorator_generator` example, total 1). Boolean sequences: `a && b && c || d || e && f` adds 3; `a && !(b && c)` adds 2; the increment applies in every context (assignments, calls, returns), not only conditions. The paper gives no threshold for this use; it says nothing about comprehensions, `match`, loop `else` or Python `with`.

**F2. Python-specific choices differ between implementations [V].**

| Construct | SonarSource Python analyzer | complexipy 8.0.1 | cognitive_complexity 1.3.0 (flake8 plugin) |
|---|---|---|---|
| `elif` | +1 | +1 | +1 + nesting, growing along the chain (each `elif` is a nested `If` child) |
| `else` of `if` | +1 | +1 | +1 |
| `else` of `for`/`while` | +1 | 0 (body at same nesting) | +1 |
| `else` of `try` | +1 | 0 | 0 |
| `except` | +1 + nesting each | +1 + nesting each | +1 + nesting each |
| `try`, `finally`, `with` | 0, no nesting | 0, no nesting | 0 (`with` body walked) |
| `match` | no increment; case bodies raise nesting | +1 + nesting once; case bodies raise nesting | not handled (0) |
| ternary | +1 + nesting; operands nested | +1 + nesting; operands nested | +1 + nesting (via nesting bump) |
| boolean operators | +1 per run of like operators, any context | +1 per run, only in some positions (assign, return, tests, positional call args, literals; not keyword args, attributes, subscripts) | +1 per `BoolOp` node in the expression |
| comprehensions | 0 | +1 + nesting per `for` clause, +1 per `if` filter | 0 |
| lambda | no nesting | nesting +1 | nesting +1 |
| nested `def` | counted in parent, nesting +1 | counted in parent, nesting +1 | counted in parent, nesting +1 |
| decorator exemption | other statements are all `return <name>` | body is exactly `[def, return]` | body is exactly `[def, return]` |
| recursion | not counted | +1 (direct call to own name, outside nested defs/lambdas) | +1 (any call to own name anywhere, including nested defs) |
| class inside a function | nesting reset to 0 | methods counted at same nesting | walked |
| default threshold | (rule S3776 default 15, [U]) | README example uses 10; default [U] | 7 |

**F3. Ruff C901 (McCabe) [V].** 1 + one per `if`, `elif`, `for`, `while`, `except` clause, `try … else`, `match` case (minus one if the last case is irrefutable), nested `def` (plus its body); class bodies are walked; `with` is transparent. Boolean operators, ternaries and comprehensions are not counted.

**F4. Ruff PLR0915 cannot be reused as is [V].** `num_statements` counts the docstring (an expression statement, +1), counts `while` but not `for` headers, does not count `return`, adds 2 for `finally`, 1 for `try` and 1 more when there are several handlers, and descends into nested function bodies. The docstring term makes the count depend on the property being checked: on monorepo private functions the AUC of the raw Ruff count against `doc` is 0.80, and 0.73 once the docstring is subtracted (F9). Default `max-statements` is 50.

**F5. Google's pylintrc [V].** `docstring-min-length=12` and `no-docstring-rgx=(__.*__|main|test.*|.*test|.*Test)$`. Google's regex replaces Pylint's default `^_`, so Google's own Pylint setup **does require docstrings on private functions** of 12 lines or more. What Pylint counts as a "line" (physical lines from `def` to the last line, blank and comment lines included) is [U]; the analogue measured here is `end_lineno - lineno >= 12`.

**F6. ESEM 2020 numbers [V].** 10 data sets, 427 snippets, about 24,000 human evaluations (mostly Java, median cognitive complexity per study 1–7.5, max 46). Random-effects pooled correlations with cognitive complexity: comprehension time r = 0.54 [0.24; 0.75] (9 studies, 327 snippets); perceived-difficulty ratings −0.29 [−0.49; −0.06] (4 studies, 203; higher rating = easier); correctness −0.13 [−0.45; 0.21] (6 studies, 269, not significant); physiological 0.00 (1 study, 12 snippets); time+correctness composites 0.40 [0.11; 0.62]. The paper also reports Scalabrino et al.: none of 121 metrics (LOC, cyclomatic complexity, documentation and developer metrics) "could accurately represent code understandability". The earlier claim "best-validated purely code-based metric" matches the paper's own conclusion ("first validated and solely code-based metric which is able to reflect at least some aspects of code understandability"). The Landman et al. claim is still [U].

### Corpus

**F7. Distributions [V].** All in-scope functions:

| metric | corpus | p25 | p50 | p75 | p90 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| code lines | monorepo | 3 | 8 | 18 | 36 | 55 | 127 |
| statements | monorepo | 2 | 5 | 11 | 21 | 31 | 65 |
| McCabe | monorepo | 1 | 2 | 4 | 7 | 10 | 19 |
| cognitive | monorepo | 0 | 1 | 4 | 10 | 18 | 44 |
| max nesting | monorepo | 0 | 1 | 2 | 3 | 3 | 5 |
| code lines | stdlib | 1 | 4 | 10 | 21 | 32 | 66 |
| statements | stdlib | 1 | 4 | 8 | 18 | 26 | 51 |
| cognitive | stdlib | 0 | 1 | 4 | 10 | 17 | 40 |
| code lines | oss | 2 | 6 | 14 | 29 | 46 | 98 |
| statements | oss | 1 | 3 | 8 | 15 | 23 | 48 |
| cognitive | oss | 0 | 1 | 3 | 8 | 14 | 33 |

Spearman correlations (monorepo): cognitive~McCabe 0.97, cognitive~statements 0.84, cognitive~code lines 0.74, statements~code lines 0.89. In this corpus cognitive complexity is close to a monotone transform of McCabe.

**F8. Our cognitive complexity agrees with complexipy [V].** Matching top-level functions and methods by name (10,531 stdlib, 23,961 monorepo, 15,929 oss): Spearman 0.98 (0.976–0.986); same side of threshold 5 for 98.4–99.0 % of functions; complexipy flags 0.3–1.4 points more, almost entirely from comprehensions. With complexipy's comprehension increments added (`cog_comp`), exact agreement is 93–95 %, Spearman ≥ 0.996, same side of 5 for ≥ 99.3 %. The remaining large differences are complexipy's position-dependent boolean counting and its nested-def handling.

**F9. Size predicts documentation better than complexity; complexity adds nothing once size is known [V].** Private functions, label `doc` (AUC = probability that a documented function scores higher than an undocumented one):

| metric | monorepo (n=7,885, base 0.69) | monorepo mixed files only (n=5,060) | oss (n=3,052, base 0.31) | stdlib (n=5,193, base 0.34) |
|---|---|---|---|---|
| code lines | 0.75 | 0.75 | 0.59 | 0.56 |
| statements | 0.74 | 0.75 | 0.57 | 0.56 |
| Ruff PLR0915 count without docstring | 0.73 | 0.74 | 0.60 | 0.55 |
| McCabe | 0.69 | 0.70 | 0.58 | 0.55 |
| cognitive | 0.68 | 0.69 | 0.57 | 0.55 |
| max nesting | 0.67 | 0.68 | 0.57 | 0.54 |

"Mixed files" keeps only files whose private functions are partly documented, which removes whole-file habits; the ranking is unchanged. Stratified AUC on monorepo: cognitive within statements bins **0.50** (no information); statements within cognitive bins 0.65; code lines within cognitive bins 0.66; cognitive within code-line bins 0.54. Doc rate by statements (rows) and cognitive (columns), monorepo private:

| stmts \ cog | 0 | 1–2 | 3–4 | 5–9 | 10+ |
|---|---|---|---|---|---|
| 0–3 | 0.48 (2125) | 0.51 (657) | 0.56 (39) | 0.78 (9) | 0.67 (3) |
| 4–7 | 0.72 (440) | 0.71 (1184) | 0.66 (494) | 0.74 (138) | 0.80 (5) |
| 8–11 | 0.83 (70) | 0.87 (304) | 0.86 (392) | 0.87 (332) | 0.96 (68) |
| 12–19 | 0.92 (24) | 0.96 (105) | 0.89 (194) | 0.89 (425) | 0.91 (207) |
| 20+ | 0.89 (9) | 1.00 (9) | 0.90 (29) | 0.95 (129) | 0.92 (494) |

Rows increase; columns are flat. Short-but-branchy functions (top right) are rare: 0.2 % of private functions have < 4 statements and cognitive >= 5.

**F10. At the operating point all gates have the same precision [V].** The AUC gap comes from tiny functions. At flag rates of 20–30 %, precision against `doc` is 0.89–0.91 on monorepo for every metric:

| gate (monorepo private) | flag rate | doc precision | recall | findings (flagged, undocumented) |
|---|---|---|---|---|
| code lines >= 20 (draft) | 0.20 | 0.90 | 0.26 | 2.0 % |
| statements >= 12 | 0.21 | 0.91 | 0.27 | 1.8 % |
| McCabe >= 5 | 0.18 | 0.90 | 0.23 | 1.8 % |
| cognitive >= 5 | 0.23 | 0.89 | 0.30 | 2.5 % |
| cognitive >= 5 OR statements >= 12 (earlier proposal) | 0.28 | 0.89 | 0.36 | 2.9 % |
| cognitive >= 6 OR statements >= 12 | 0.25 | 0.90 | 0.33 | 2.4 % |
| cognitive >= 8 OR statements >= 12 | 0.22 | 0.91 | 0.29 | 2.0 % |
| Pylint analogue (`end_lineno - lineno >= 12`) | 0.54 | 0.91 | 0.71 | 4.7 % |

F1 is maximized by near-zero thresholds on every corpus (monorepo: statements >= 3, F1 0.80), because 69 % of monorepo private functions are documented; F1 is therefore not a usable selection criterion here, and the threshold is a volume choice.

**F11. Where documentation saturates [V].** Doc rate by value, monorepo private:

| statements | 1 | 2 | 3 | 4–5 | 6–7 | 8–9 | 10–11 | 12–14 | 15–19 | 20+ |
|---|---|---|---|---|---|---|---|---|---|---|
| doc rate | 0.41 | 0.55 | 0.57 | 0.66 | 0.76 | 0.86 | 0.88 | 0.90 | 0.90 | 0.92 |

| cognitive | 0 | 1 | 2 | 3 | 4 | 5 | 6–7 | 8–9 | 10–14 | 15+ |
|---|---|---|---|---|---|---|---|---|---|---|
| doc rate | 0.53 | 0.66 | 0.73 | 0.76 | 0.78 | 0.84 | 0.87 | 0.90 | 0.91 | 0.93 |

| code lines | 1 | 2 | 3–4 | 5–7 | 8–11 | 12–15 | 16–19 | 20–29 | 30+ |
|---|---|---|---|---|---|---|---|---|---|
| doc rate | 0.37 | 0.44 | 0.53 | 0.62 | 0.77 | 0.88 | 0.86 | 0.89 | 0.91 |

Google engineers document about 90 % of private functions from statements >= 12, cognitive >= 8 or code lines >= 12. Cognitive 5 sits at 0.84.

**F12. Threshold sensitivity [V].** Flag rate / doc precision / findings for `cognitive >= a OR statements >= b`, private functions:

| a \ b | monorepo: 10 | 12 | 15 | oss: 10 | 12 | 15 | stdlib: 10 | 12 | 15 |
|---|---|---|---|---|---|---|---|---|---|
| 4 | 0.34 / 0.88 / 4.3 % | 0.32 / 0.88 / 4.0 % | 0.30 / 0.87 / 3.9 % | 0.33 / 0.41 / 19.8 % | 0.31 / 0.41 / 18.6 % | 0.30 / 0.41 / 17.9 % | 0.34 / 0.38 / 21.4 % | 0.33 / 0.38 / 20.2 % | 0.32 / 0.39 / 19.5 % |
| 5 | 0.31 / 0.89 / 3.4 % | 0.28 / 0.89 / 2.9 % | 0.25 / 0.89 / 2.7 % | 0.30 / 0.42 / 17.2 % | 0.27 / 0.42 / 15.6 % | 0.25 / 0.42 / 14.6 % | 0.31 / 0.39 / 18.9 % | 0.28 / 0.39 / 17.2 % | 0.27 / 0.40 / 16.1 % |
| 6 | 0.30 / 0.90 / 3.0 % | 0.25 / 0.90 / 2.4 % | 0.22 / 0.90 / 2.1 % | 0.27 / 0.42 / 15.5 % | 0.24 / 0.43 / 13.6 % | 0.21 / 0.42 / 12.2 % | 0.29 / 0.39 / 17.7 % | 0.26 / 0.39 / 15.6 % | 0.24 / 0.40 / 14.1 % |
| 8 | 0.28 / 0.90 / 2.7 % | 0.22 / 0.91 / 2.0 % | 0.18 / 0.91 / 1.6 % | 0.25 / 0.43 / 14.0 % | 0.20 / 0.44 / 11.6 % | 0.17 / 0.43 / 9.7 % | 0.27 / 0.40 / 16.1 % | 0.23 / 0.41 / 13.3 % | 0.19 / 0.42 / 11.0 % |

One step of either threshold moves the flag rate by 2–4 points and leaves precision unchanged (±0.01). Under `cognitive >= 5 OR statements >= 12`, 25 % (monorepo), 37 % (oss) and 26 % (stdlib) of flagged functions are flagged by cognitive complexity alone.

**F13. Nested functions are the main source of noise [V].** monorepo private functions by kind, gate `cognitive >= 5 OR statements >= 12`:

| kind | n | doc rate | flag rate | doc precision |
|---|---|---|---|---|
| module-level `_function` | 3,685 | 0.80 | 0.31 | 0.97 |
| method of any class | 3,290 | 0.69 | 0.28 | 0.87 |
| nested function (closure, decorator wrapper, callback) | 910 | 0.26 | 0.15 | 0.42 |

Nested functions are 12 % of private functions but about a third of the findings. The white paper counts a nested function's logic in its parent (with extra nesting), so the parent's score already covers it.

**F14. Sections are much rarer than docstrings, even on large functions [V].** Share of docstrings with an `Args:`/`Returns:`/`Yields:` section (or Sphinx/NumPy equivalent), functions with a parameter or a return value, by statements:

| corpus | 0–3 | 4–7 | 8–11 | 12–19 | 20+ |
|---|---|---|---|---|---|
| monorepo private | 0.39 | 0.48 | 0.46 | 0.55 | 0.57 |
| monorepo public | 0.46 | 0.55 | 0.59 | 0.63 | 0.63 |
| oss private | 0.50 | 0.51 | 0.42 | 0.44 | 0.51 |
| oss public | 0.66 | 0.68 | 0.66 | 0.71 | 0.80 |

Against the `sec` label every metric is weak (monorepo private AUC 0.63–0.67; at the gate, precision 0.47–0.51). Requiring sections whenever the gate fires would flag about 40 % of the documented nontrivial functions in monorepo, the most documented corpus here.

**F15. Manual review: neither metric finds "non-obvious logic" [V].** 39 undocumented private monorepo functions:

| bucket | W | B | N | examples |
|---|---|---|---|---|
| A. cognitive 5–6, statements < 12 (complexity only) | 3 | 1 | 4 | W: recursive DFS that mutates its arguments (`_FindOneComponent`); `while` walk returning a parent id or `None` (`_ancestor_blocked`); flag redefinition with a side effect (`_def_flag`). N: `elif` chain mapping enum to constructor (`from_property_filter`, flat dispatch); skip-blank/skip-bad-JSON generator closure (`_rows`, guard chain); polling closure; `getattr(...) or ...` chain (`_extract_from_items`, one boolean run plus nesting) |
| B. statements 12–14, cognitive < 5 (size only) | 3 | 3 | 2 | W: TensorFlow tensor pipeline with cognitive 0 (`ngram_matches`); fail-open LLM call returning `None` on error; validator appending to an `errors` argument. N: 68-line configuration builder with cognitive 0 (`_ConfigureAlerting`); message formatter |
| C. cognitive 3–4, statements 8–11 (below both) | 2 | 1 | 5 | W (false negatives): argument parser returning a padded list (`_ProcessArgs`); feature-path to SQL mapping (`GetLogsProtoPath`). N: regex fallback chain, decorator wrapper, guard chains |
| D. code lines 20–24, cognitive < 5, statements < 12 (lines only) | 2 | 2 | 3 | W: TFP generative model with cognitive 0; link-text ratio heuristic. N: `tf.io` feature-spec literal (20 lines, 3 statements), prompt builder |
| E. cognitive 5–7 and statements 12–15 (both) | 2 | 1 | 5 | W: retry decorator closure; failure-counting state update. N: seven-`if` attribute copy (`_HandleGlobals`, cognitive 7, McCabe 8), field-by-field proto-to-dict copy, prompt builders |

Above the earlier proposal's thresholds (A+B+E): 8 W, 5 B, 11 N of 24. Below (C+D): 4 W, 3 B, 8 N of 15. What made a docstring warranted was semantic: an unusual return shape or `None` meaning, mutation of arguments, hidden side effects, a domain formula, tensor shapes. Control flow does not capture these. Recurring false positives: flat dispatch (`elif` chains), guard chains (`if x: setattr`), data/config builders (long, cognitive 0), prompt/message builders. Recurring false negatives: straight-line numeric or tensor pipelines and compact parsers with surprising outputs.

## 3. Decision proposals

- **D-gate1. Two triggers, mirroring §3.8.3: `statements >= 12 OR cognitive >= 6`.** Statements stand for "nontrivial size", cognitive complexity for "non-obvious logic". Evidence: same precision as the draft (F10, 0.90 on monorepo), five more points of flag rate (25 % vs 20 % of private functions; 2.4 % vs 2.0 % findings in monorepo, 13.6 % vs 11.0 % in oss), and it catches compact branchy code (bucket A). Threshold 6 instead of 5: at 5, Google engineers document only 84 % (F11) and the extra flags are mostly flat dispatch and guard chains (bucket A); 8 would match the 90 % plateau of statements 12 but would make the complexity trigger nearly vestigial (+1 point over statements alone). The user should know the trade-off: on the documentation proxy, complexity adds no information once statements are known (F9, stratified AUC 0.50), so the second trigger is justified by the style guide's wording and by examples, not by measured accuracy.
- **D-gate2. Statements: one per statement node of the function's own body.** Recursive into `if`/`elif`/`else`, loop bodies and `else`, `try` bodies, handlers, `else`, `finally`, `with` bodies, `match` cases; each compound statement counts 1 for itself (each `elif` is one); a nested `def`/`class` counts 1 and its body is not entered; the docstring is excluded; `return`, `pass`, `break` count like any statement. Not Ruff's `num_statements` (F4). This replaces code lines (D4): formatting changes code lines (bucket D: 20-line literal with 3 statements; bucket B: 68-line builder with 13) and code lines did no better than statements (F9, F10).
- **D-gate3. Cognitive complexity: the white paper as implemented by SonarSource for Python, plus the white-paper rules that implementation omits.** Increments (n = current nesting):

  | construct | increment | nesting of the body |
  |---|---|---|
  | `if`, ternary | +1 + n | +1 (ternary operands) |
  | `elif`, `else` (of `if`, `for`, `while`, `try`) | +1 | +1 |
  | `for`, `async for`, `while` | +1 + n | +1 |
  | each `except` / `except*` clause | +1 + n | +1 |
  | `match` (once) | +1 + n | +1 for each case body; guards at case nesting |
  | `try`, `finally`, `with`, `return`, `break`, `continue`, `raise`, comprehensions | 0 | unchanged |
  | boolean operators | +1 per `BoolOp` node that is not a direct operand of a `BoolOp` with the same operator (= per run of like operators; `not` starts a new run) | – |
  | nested `def`, `lambda`, nested `class` body | 0 | +1, except the decorator exemption: when a function's body (docstring aside) is one nested `def` plus `return <name>` statements, that nested `def` keeps the current nesting (applies recursively) |
  | recursion | +1 if the function's own body (not nested `def`/`lambda`/`class`) calls its own name, or `self.<name>` / `cls.<name>` in a method | – |

  Deviations from the SonarSource Python analyzer, each from the white paper: `match` counts like `switch`; lambdas raise nesting (B2); recursion counts (B1). Deviation from complexipy: comprehensions add nothing (the SonarSource choice); this puts 0.9–1.4 % of functions on the other side of threshold 6 than complexipy does (F8). Nested functions are scored inside their parent, not separately.
- **D-gate4. Nested functions are not checked by `undocumented-long-function`.** They are 12 % of private functions, a third of monorepo findings, and only 42 % of flagged ones are documented by Google engineers (F13); their logic already counts toward the enclosing function. This is a departure from the letter of §3.8.3 ("function") to state in the docs. Methods of nested classes stay in scope.
- **D-gate5. Sections: keep one gate, but do not widen it.** `undocumented-parameter` / `undocumented-return-value` apply when the D-gate1 predicate holds or the docstring is already structured (D6). Measured cost on monorepo: about 40 % of documented nontrivial functions lack sections (F14); expect those rules to be the noisiest of the three. Alternatives for the user in Q-gate2.
- **D-gate6. Options.** Replace `min-lines` with `min-statements` (default 12) and `min-cognitive-complexity` (default 6), declared by each of the three rules (rule files cannot share consts, as in D5). 0 disables a trigger. The message names the trigger: "Function `f` has 14 statements and no docstring." / "Function `f` has cognitive complexity 7 and no docstring." Classification stays `Precision::Heuristic`.
- **D-gate7. Implementation: one `ruff_python_ast` walk per function.** A visitor with `nesting: u32`, `statements: u32`, `cognitive: u32`. `visit_stmt`: unless the statement is the docstring, `statements += 1`; match on the statement kind for D-gate3; for `StmtFunctionDef`/`StmtClassDef` inside the body, add nothing to `statements` beyond the 1, then walk the body with `nesting + 1` (or unchanged under the decorator exemption) contributing only to `cognitive`. Ruff's `StmtIf` exposes `elif_else_clauses` (a clause with `test: Some` is `elif`), so no `orelse` unwrapping is needed. `visit_expr`: `ExprBoolOp` (compare `op` with the parent's), `ExprIf`, `ExprLambda` (nesting + 1), `ExprCall` for the recursion test (`ExprName` equal to the function name, or `ExprAttribute` on `self`/`cls`). Do not descend into lambda bodies for recursion. Cost: linear in the function's size; no type information. Statement counting stops descending at nested `def`/`class`, the cognitive walk does not.

## 4. Open questions

- **Q-gate1.** Accept D-gate1's two triggers knowing that, on the documentation proxy, the cognitive trigger adds recall but no measured accuracy? Or use `statements >= 12` alone (simplest, best AUC) or `cognitive >= 5 OR statements >= 12` (earlier proposal, +3 points of flag rate, more flat-dispatch flags)?
- **Q-gate2.** Sections on private functions: same gate (D-gate5), a higher gate for private functions only (for example `statements >= 20 OR cognitive >= 10`), or sections required only when public or already structured?
- **Q-gate3.** Exempt nested functions entirely (D-gate4), or only closures passed as arguments / decorator wrappers?
- **Q-gate4.** Count comprehensions like complexipy (+1 + nesting per `for`, +1 per `if`) to stay numerically compatible with the most used Python tool on the same parser, or follow SonarSource (D-gate3)?
- **Q-gate5.** Should flat `elif` dispatch (every test compares the same name to a literal) be discounted, as the white paper does for `switch`? It is the most frequent complexity-only false positive (buckets A and E), but the paper explicitly prices `else if` chains higher than `switch`.
