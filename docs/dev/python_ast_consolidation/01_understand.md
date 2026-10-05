# Python AST consolidation: 01 Understand

> [!NOTE]
> **Status: REVIEW COMPLETE. Waiting for decisions D1–D9 (§8) before landing.**
> Scope: the helpers the legacy batch (commit "New Pol urles") added to [python.rs](../../../src/code_lint/ast/python.rs), and the six rules that use them. New code: lines 3066–5073, edits at 127–130 and 494–611, unit tests at 5845–6133. The file went from about 3,770 to 6,134 lines.
> Ordering constraint: this lands before the typed-CST work in [ast_robustness](../ast_robustness/01_understand.md).
>
> **Superseded in part (2026-10-05):** the layout proposed in §8 changed after landing. `quote_wrapped.rs` is gone: format-string parsing is in `ast/python/format_strings.rs` and the prose and quote heuristics are in the `quote-wrapped-placeholder` rule. `logging.rs` only recognises logger calls; the unmatched-placeholder check is in its rule. `call-before-definition` is disabled. Remaining rule-shaped items are listed in ROADMAP.md ("Rule-shaped items left in `ast/python`").

## 1. Method

- Each rule was reviewed on its own during the batch, and the 23/23 per-exemption mutation check passed. This review is cross-cutting instead.
- Three read-only reviewers each took one structural cluster: strings and logging, scopes and bindings, types and classes. They compared the new code with the helpers that already existed.
- Reviewers had no shell. The lead re-ran every behavior claim against the built `omni-code-lint`; those rows are marked *binary*. Claims checked only by reading the code are marked *inspection*.

## 2. Summary

- **11 confirmed behavior bugs** (§3): 6 false positives or wrong suggestions, 4 false negatives and 1 inconsistency. The mutation check could not catch them: none of them is a named exemption.
- **Duplicated or divergent logic in every cluster** (§4). Some duplicates disagree on the same fact (logger receivers, format-field parsing, `Final`, receivers, binding targets), and three of the bugs (S3, S5, T2) come from that.
- **One harness-driven design.** The epoch flush in `call-before-definition` exists so `rule_test!`'s repeat check passes. It causes bug C1. Now tracked as "Rule test harness" in [ROADMAP.md](../../../ROADMAP.md).
- **Documentation drift (lead's error).** The batch's `06`/`07` docs named about 25 helpers and tests that do not exist, gave wrong case counts, and stated one false lesson. Corrected (§7).

## 3. Confirmed behavior bugs

| ID | Rule | Effect | Minimal repro | Cause | Check |
| :--- | :--- | :--- | :--- | :--- | :--- |
| S1 | `quote-wrapped-placeholder` | False positive, harmful fix | `f'{{"key": "{value}", "mode": 1}}'` and `'{{"key": "{}"}}'.format(v)` are flagged. The suggested `{value!r}` emits single quotes and breaks the JSON. | `has_unclosed_structured_delimiter` skips `{{`, but in f-strings and `.format()` `{{` is the literal `{` that opens the JSON object. | binary |
| S2 | `quote-wrapped-placeholder` | False negative | `logger.warning("Update of '%s' failed", name)`, `f"Delete '{path}' first"`, `f"Missing key in '{section}'"` and `f"Cannot read config from '{path}'"` are not flagged. Each control with the keyword reworded is flagged. | `SQL_STATEMENT_KEYWORDS` and `SQL_OPERATOR_KEYWORDS` are matched with `eq_ignore_ascii_case`, so English `Update`, `Delete`, `With`, `in`, `from`, `set` read as SQL. | binary |
| S3 | `quote-wrapped-placeholder` | False positive | `("Pass --output=" "'{path}' to the tool").format(path=p)` and the logger `%s` twin are flagged. The f-string twin is not. | Only the f-string path reads earlier parts of an implicit concatenation (`preceding_concatenated_literal_text`). | binary |
| S4 | `unmatched-logger-placeholder` (`Precision::Exact`) | False positive | `logger.info("\N{BULLET} Order {} filled", order_id)` reports `{BULLET}`. | The message is the raw source text between the quotes, so `\N{...}` escapes look like fields. | binary |
| S5 | `quote-wrapped-placeholder` | Silent false negative | The same f-string finding disappears when the file starts with a blank line. | The f-string path slices `file.source_text()` with absolute byte offsets. That text is the root node's, which starts after leading whitespace (*cause: inspection*). Test sources never start with whitespace (`indoc!`). | binary |
| T1 | `inline-public-attribute-annotation` | Wrong suggestion | In a `@dataclass`, `self.total: float = ...` in `__post_init__` gets "Move `total: float` to the body of `Order`". That makes `total` a required field: `Order(2, 3.0)` raises `TypeError`. Same for attrs and pydantic. | The collector never looks at the enclosing class kind. | binary |
| T2 | `fake-without-protocol` | Inconsistent | `class FakeRepoA(metaclass=ABCMeta)` is flagged, `class FakeRepoB(ABC)` passes. Neither names the collaborator the suggestion asks for. | `is_contract_base` counts `ABC` as a contract and ignores `metaclass=`. | binary |
| C1 | `call-before-definition` | False negative | `main()` calls `show()`, and `show` is defined after two `@render.register` `def _` handlers: not flagged. Without the second `_`, it is flagged. | The second `_` flushes the epoch, so caller and callee land in different epochs. Every unrelated same-name redefinition hides forward calls across it. | binary |
| C2 | `call-before-definition` | False positive | `if any((helper := item) for item in items): return helper()` is flagged although `helper` is the local. | The binding walk stops at comprehensions. PEP 572 binds a walrus target in the enclosing scope. | binary |
| C3 | `call-before-definition` | False positive | `case [first] as helper: return helper(first)` is flagged. | The `as_pattern` capture is not recorded as a binding. | binary |
| C4 | `call-before-definition` | False negative | `def run(): def inner(v=helper()): ...` with `helper` defined later is not flagged. | The defaults of a nested `def` run in the enclosing scope, but the walk skips them. The pre-existing walkers (lines 2410, 2577) do not. | binary |

## 4. Duplicated or divergent logic

### 4.1 Strings, format placeholders and logger calls (3431–3726, 4223–5073)

- **Two logger-call matchers with different receiver lists.** Grep finds no caller outside this cluster. They disagree on real code (*binary*):

  ```python
  _logger.info("Retry {attempt} failed", attempt)     # unmatched-logger-placeholder: missed
  _logger.info("Failed to reach '%s' now", host)      # quote-wrapped-placeholder: flagged
  app.logger.info("Retry {attempt} failed", attempt)  # unmatched-logger-placeholder: flagged
  app.logger.info("Failed to reach '%s' now", host)   # quote-wrapped-placeholder: missed
  ```

  - `LOGGER_METHODS`/`LOGGER_RECEIVERS`/`LOGGER_ATTRIBUTES` (3441–3459) and `LOGGER_MESSAGE_FIRST_METHODS`/`PRINTF_LOGGER_RECEIVERS` (4244–4256) should be one table. `trace`/`success` (Loguru only, no printf) become data on that table, not a second list.
  - One rule counts `*args` as format arguments and the other drops it.
  - `error-log-in-except` keeps its deliberately narrow, configurable list.
- **Two PEP 3101 field parsers.** `parse_replacement_field` (3596) and `is_valid_str_format_field`/`find_str_format_closing_brace`/`evaluate_str_format_brace_span` (4874–4908) disagree:
  - `}}` inside a field;
  - non-ASCII names (`{café}` is missed);
  - surrounding spaces (`'{ name }'` is flagged with `{name!r}`, which silently changes the key; *binary*);
  - malformed braces (one aborts, the other continues).
- **Two printf scanners** with different mapping-key rules: the strip arm (4486–4515) and `parse_bare_printf_s_placeholder` (4972). The dropped `logger-printf-format` design planned a third.
- **String prefix, implicit concatenation and literal segments are each computed in 4–5 places:**
  - new: `extract_plain_string_node`, `extract_logger_message_literal`, `string_prefix_flags`, `outermost_string_expression`, `append_string_literal_segments`, `combined_message_literal_text`;
  - pre-existing: `literal_value`, `is_triple_quoted` (which misses the `t` prefix).

  S3 and S5 come from this spread.
- **Untested branches** (*inspection*): bracket depth, the SQL operator check, the `\n`/`\t`/`\r` boundaries, two or more trailing backslashes, the `str.format(...)` call form.

### 4.2 Types, classes and attributes (494–611, 3066–3429)

- **Base classes are classified three ways:**
  - `is_contract_base`;
  - `is_protocol_or_abc_class_raw`, which treats Protocol, ABC and ABCMeta as one family;
  - `inherits_from`, pre-existing and called only by tests.

  Two filters decide which children are bases: 605–611 skips `dictionary_splat`, `base_class_terminals_raw` does not. `unsubscripted_name` cuts the text at `[` instead of reading the tree. T2 comes from this.
- **Near-copied attribute walker.** `collect_method_inline_public_attr_annotations_rec` (3137) copies `collect_init_annotated_attrs_rec` (2065): same guard, same scope stop, same `_` test. The loop over a class body's methods is written three times (2148, 3198, 3933).
- **Two `Final` checks.** `is_bare_final_annotation` (3084) copies the loop of `has_final_annotation` (2926) but resolves qualified names differently: `my_types.Final` counts as `Final` for one and not the other (*inspection*).
- **Two receiver detectors** that re-derive the decorator rules on top of `PythonParameterKind::Receiver`:
  - `is_instance_method_with_self_receiver` (3115) skips static and class methods and requires `self`;
  - `method_receiver_name` (3993) skips only static methods and accepts `self` or `cls`.
- **Union flattening twice.** `collect_union_branches` (3305) repeats the union half of `collect_type_constructors_raw` (1088–1130). Two differences are legitimate: it tracks `None`, and it unwraps only `Annotated`.
- **Ad hoc collection vocabulary.** `ABSTRACT_AND_IMMUTABLE_COLLECTION_CONSTRUCTORS` copies `MUTABLE_COLLECTION_ABCS` word for word, `Mutable*` names included, despite "immutable" in its name. `Iterable[int] | None` and `Reversible[int] | None` are flagged, `Iterator`, `Container` and `AsyncIterable` are not (*binary*). The rule doc does not say where the line is.
- **Parse-tree quirks handled in about 10 places**: the `type`/parenthesis unwrap, and name vs `generic_type`/`subscript`. Each is one more site to port for the typed-CST migration (F2 in [ast_robustness](../ast_robustness/01_understand.md)).

### 4.3 Scopes, bindings and calls (3728–4221, edit 127–130)

- **Four walkers encode Python binding rules, each with its own policy for target leaves:**
  - `traverse_python` + `extract_from_pattern`, file-wide, pre-existing;
  - `collect_bindings_in_subtree` + `extract_local_target_names`, a near copy;
  - parameter names twice, through `parse_param_parts` and through `extract_from_pattern`.

  They disagree on attribute/subscript targets, walrus inside comprehensions (C2), `global`, and `x += 1`.
- **`extract_local_target_names` works around a pre-existing bug instead of fixing it.** `extract_from_pattern` descends into `attribute`/`subscript` targets, so `collect_bindings` records `ctx` for `self.ctx = make()`. `abbreviated-name` then flags it (*binary*), although its doc says attributes are not checked.
- **The 127–130 edit silently changed three existing rules.** Typed `*a: T` / `**k: T` parameters are now bindings for `single-letter-name`, `abbreviated-name` and `type-suffixed-name`: `def total(*a: int)` now flags `a` (*binary*). That is a fix, but none of those rules tests it.
- **Epochs serve the harness, not Python.**
  - Design D7 ([03_design_plan.md](../define_before_use/03_design_plan.md)) adds the flush "so `rule_test!` (`RepeatCheck::SameCode`) works out of the box". Its only mutation test is the repeat check itself.
  - No test contains a redefinition. The `@singledispatch` justification was added afterwards and is wrong (C1).
  - `partition_scope_epochs` (64 lines, with `mem::take` and a global order counter) is the hardest part of the cluster to read.
- **Scope boundaries differ from the existing walkers:** nested-`def` defaults (C4), and a comprehension's first iterable (evaluated in the enclosing scope, shadowed here).
- **Dead code** (*inspection*):
  - `field("lambda_parameters")` (lambdas use `parameters`);
  - the fallback at 3816, which nothing calls that way;
  - the `for_in_clause` arm;
  - pattern arms copied over that no target position can reach.

## 5. Checked, no issue

- `can_reach`, the SCC exemption, constructor handling, deduplication and span sorting are correct and proportionate.
- f-string interpolations come from the tree (`type_conversion`, `format_specifier`, `=`); `\N{}` inside f-strings is handled.
- The byte scans are UTF-8 safe and no slice can panic. Comments inside argument lists and concatenations are filtered out.
- `nullable-collection-return` mirrors its siblings: the same signature exemptions, `RequireExplanation`, `SourceOnly` and span. `unwrap_return_envelope` correctly leaves out the qualifiers that are invalid in a return type.
- Decorator detection reuses `extract_decorators_raw`/`DecoratorInfo`. Visibility is minimal: each public helper has exactly one rule caller.

## 6. Pre-existing issues found (outside the batch)

- `CommentIndex::from_file` ([comments.rs](../../../src/code_lint/semantic/comments.rs)) slices `source_text()` with absolute offsets, the same pattern as S5. Impact not verified.
- Naming rules do not report match captures (`case z:`, `case [first] as q:`), while `for z in ...` is flagged. Cause not investigated.
- `parse_param_parts` near-copies: already in [ROADMAP.md](../../../ROADMAP.md) ("Pre-existing Python collector defects").

## 7. Documentation drift (corrected)

- **What was wrong.** The lead wrote the six `06_review_and_audit.md` / `07_learn.md` from worker reports without checking them against the code:
  - about 25 helper, constant and test names that do not exist (`is_non_collaborator_base`, `collect_nullable_collection_returns`, `CONSTRUCTOR_METHODS`, `literal_fragments`, …);
  - a `naming::words` module that does not exist;
  - wrong case counts (`fake-without-protocol` 18 instead of 25, `inline-public-attribute-annotation` 20 instead of 19, `nullable-collection-return` 22 instead of 32);
  - the lesson "epochs are essential for `@singledispatch`" (C1 shows the opposite).
- **What changed.** These are fixed in place. The `04`/`05` execution logs stay as historical records.
- **Lesson for [rule_batch_playbook.md](../rule_batch_playbook.md).** Grep every backticked identifier and recount the cases before closing `06`/`07`.

## 8. Landing

### Options

| Option | What | Verdict |
| :--- | :--- | :--- |
| A. Fix in place | Fix the 11 bugs with repro tests in today's layout. Keep the duplicates. | Fast. The typed-CST migration then ports about 20 duplicated helpers twice. |
| **B. Split, then consolidate per cluster** | 1) Pure-move split of `python.rs` into submodules. 2–4) One commit per cluster: shared helpers, bug fixes with repro tests, mutation re-run. 5) Docs `02`–`07` here and ROADMAP updates. | **Recommended.** Each cluster commit touches one or two small files. Reviewer estimates: strings −250, types −150 to −200, scopes −120 to −150 lines. |
| C. Split only | Pure move now, fix later. | Ships known bugs. |

Proposed layout for B. `python.rs` declares no component, so its children inherit `CodeLintAst`. Re-exports use `mod x; pub use self::x::Item;`, as [architecture.rs](../../../src/architecture.rs) requires. Each new file copies the `omni:disable-file [repeated-literal]` header.

```text
src/code_lint/ast/python.rs                 shared primitives (paths, decorators, node utilities), re-exports
src/code_lint/ast/python/strings.rs         prefix, implicit concatenation, node-relative literal segments;
                                            PEP 3101 and printf tokenizers on plain &str
src/code_lint/ast/python/logging.rs         logger_call(): receivers, methods, printf or brace style, message, arguments
src/code_lint/ast/python/quote_wrapped.rs   prose and quote heuristics of quote-wrapped-placeholder
src/code_lint/ast/python/annotations.rs     annotation unwrapping, type constructors, unions, Final, collection vocabulary
src/code_lint/ast/python/classes.rs         classes, base classification, attribute walkers (inline and __init__)
src/code_lint/ast/python/functions.rs       method kind and receiver, direct function definitions
src/code_lint/ast/python/scopes.rs          binding sites, scope boundaries, the call-before-definition fact
```

The three ast_robustness POC changes (`proto_p1`–`proto_p3`) each edit `python.rs`, so they will need a rebase.

### Decisions

| # | Question | Recommendation |
| :--- | :--- | :--- |
| D1 | T1: what does `inline-public-attribute-annotation` do in dataclass-like classes? | Skip them: `@dataclass` (existing `PythonClassInfo::dataclass_decorator`), attrs decorators, and pydantic `BaseModel` subclasses. In those classes any class-level annotation creates a field, so no suggestion preserves behavior. |
| D2 | T2: are `ABC` and `metaclass=ABCMeta` contracts or markers for `fake-without-protocol`? | Markers. Neither names a collaborator, so both get flagged. The `inherits_abc` pass case becomes a fail case. |
| D3 | C1: how does `call-before-definition` handle redefinitions? | Replace epochs with live definitions: the last definition of a name is the callee's position. `@overload`/`@property` grouping stays. This fixes C1, removes the flush, and still passes `SameCode`. The lead traced all 10 current `fail` cases; the call graph must be keyed by name, not by definition. A future `fail` case that also calls an earlier sibling would get an extra finding in the doubled module (ROADMAP harness item). New finding: `def helper; def run: helper(); def helper` is flagged, which matches runtime behavior (Ruff F811 flags the redefinition too). Your item 3 (a public-before-helper ordering rule for both languages) may change what this rule enforces; the binding fixes (C2–C4) are needed either way. |
| D4 | Logger calls: one model for both logger rules? | Yes: one `logger_call()` with one receiver predicate (the union of today's lists, matched on the receiver's last name: `logger`, `_logger`, `log`, `self.logger`, `app.logger`, …) and a per-method `uses_printf` flag. Count `*args` in both rules. Both rules then flag more calls; update their `what_it_does`. |
| D5 | S2: how to tell SQL from English? | Match SQL keywords case-sensitively (uppercase only). Lowercase SQL becomes flaggable, which is rarer than prose starting with "Update", "Delete" or "With". |
| D6 | Collection vocabulary for `nullable-collection-return` | One shared table with the `concrete-*` rules, and the boundary stated in `what_it_does`. Add `Iterator` (empty value `iter(())`); leave the other names as today. |
| D7 | The shared binding walker stops at attribute/subscript targets. That changes the naming rules too. | Accept: `abbreviated-name`/`type-suffixed-name` stop flagging `self.ctx`, which matches their docs. Add pass/fail cases for `self.ctx` and `*a: int`. |
| D8 | Split scope | The batch's clusters plus the pre-existing helpers they merge with. The rest of `python.rs` (signatures, mutation tracking) can follow as a separate pure move if wanted. |
| D9 | Order | Split first: a pure move is trivial to verify, and each cluster commit then stays contained. The types reviewer preferred consolidating first. Either order keeps the move diff pure. |
