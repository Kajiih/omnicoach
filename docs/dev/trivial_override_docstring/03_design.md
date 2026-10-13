# Trivial override docstring: 03 Design

> [!IMPORTANT]
> **Status: VALIDATED** (lead, 2026-10-12; implemented). Verdict: **split and narrow**. The legacy linter's line-count check is replaced by three rules on what an `@override` docstring hides: `deferral-docstring`, `copied-base-docstring` and `partial-override-docstring`. Lead answers to §8: Q1 accept the double report, Q2 no entry comparison, Q3 ship Rules 2 and 3 now, Q4 default `ban` for all three, Q5 three rules, Q6 `docstrings` topic added. Implementation deviations: `04_execution_log.md`.

Inputs: `01_understand.md` (philosophy §5: no docstring, or a complete one; scope §6: `@override` only) and `02_references.md` §3 (measurements).

## 1. Signals: kept or rejected

| Signal | Decision | Why (from `02_references.md` §3) |
| :--- | :--- | :--- |
| Legacy linter: any one-line docstring on `@override` | Rejected (D1) | Line count does not separate pointer, copy, difference and complete docstring; its fix pads instead of completing |
| S1: the whole docstring is a deferral phrase | **Rule 1** `deferral-docstring` | 92/92 in `@override` scope; works without seeing the base |
| S2: equal to the inherited docstring, or to its summary paragraph | **Rule 2** `copied-base-docstring` | 12/12 in `@override` scope; 36/36 exact copies on all methods |
| S2 variants: same summary with a different body, extends the base, token overlap | Rejected (E1–E3) | Different text that adds information; complete or not, it is Rule 3's question |
| S5: omits a contract section the inherited docstring has | **Rule 3** `partial-override-docstring` | 3/3 in `@override` scope, all one-line summaries under a sectioned base |
| S3: same docstring on sibling implementations | Rejected for now, ROADMAP (§9) | Fix is often a design change (add the method to the base); small reach |
| S4: docstring restates the method name | Rejected here, ROADMAP (§9) | 5 of 680 override one-liners; not override-specific |

## 2. Decisions

### Common to the three rules

- **D1.** The legacy linter's line-count proxy is not ported (`01_understand.md` D1).
- **D4. Three rules, split by why and how.** P1 and P2 share the fix (remove) but not the harm: a pointer hides the docstring it names; a copy adds nothing and drifts. P3 has a different fix (complete it, or remove it). The configuration needs differ too: a legacy codebase may keep its pointers while banning copies, and Rule 3 is the most opinionated (D17). One rule per template (batch decision c).
- **D7. Matching normalization** (Rules 1 and 2, and E15 of Rule 3): dedent like `inspect.cleandoc`, collapse whitespace runs, case-fold, strip one trailing period (`PythonDocstring::normalized()`). For Rule 1 also strip backticks, Sphinx roles (`:meth:`, `:class:`, `:func:`, with an optional `py` domain) and a trailing `()`.
- **D8. Only `@override` methods (user decision, reverses the previous D8).** A method is in scope when it is decorated with `@override`, resolved through imports (`typing.override`, `typing_extensions.override`; `PythonFunctionSignature::has_override_decorator()`). Undecorated overrides are not inspected (E10). The rules rely on mypy `explicit-override` or Pyright `reportImplicitOverride` to make overrides explicit (`02_references.md` §1); Ruff has no such rule. Consequences: suggestions no longer say "decorate with `@override`"; `__init__` and structural `Protocol` implementations, which neither type checker asks to decorate, are out of scope unless decorated (`01_understand.md` §6); legacy `"""See base class."""` code without `@override` (~15,000 monorepo files) is not flagged.
- **D9. What "inherited docstring" means for Rules 2 and 3**: the docstring a documentation tool would show if the override's docstring were removed, as far as the file shows. Walk the in-file ancestors depth-first, left to right, without revisiting a class (this approximates the MRO). Take the first one whose body has a documented `def` of that name. A nearer ancestor that defines the method without a docstring is skipped, as `inspect.getdoc` and Pyright's `resolveMethodDocInfo` skip it (`01_understand.md` §4).
- **D10. Base-class binding follows Python's**: a base name refers to the *last* class with that name defined **before** the subclass in the file. This is runtime semantics. It also keeps the `rule_test!` doubled fail case (`{code}\n{code}`) correct, since each copy binds to its own base (`rule_batch_playbook.md` §4).
- **D11. In a base class body**, the method's docstring is the one on the last *documented* `def` with that name. `@overload` stubs and property setters are usually undocumented, so this picks the implementation or the property getter.
- **D13. Scope**: `@override` `def` / `async def` directly in a class body, in every file (`RuleTarget` unrestricted). Test fakes copying a protocol's docstrings are as stale as production code. Module-level and nested functions are out (E9).
- **D14. Diagnostic span**: the docstring literal (`PythonDocstring::node()`).
- **D15. Classification** (three rules): topic `docstrings` (new, batch-wide, owned by the lead); `Precision::Exact` (each rule claims exactly what it matches: a listed phrase, text equal to the inherited docstring, a section kind absent from the override and present in the inherited docstring; the exemptions only remove findings); `Consensus::Opinionated` (Rule 1: Google recommended `"""See base class."""` before `@override` existed; Rule 2: some projects copy docstrings so that readers of the source see them inline; Rule 3: Google §3.8.3.1 accepts a differences-only docstring); `ImpactedQuality::Maintainability`.
- **D16. Overlap between Rules 1 and 2 (lead, 2026-10-11)**: the same docstring is both a deferral phrase and equal to the inherited one only if the base docstring is itself a deferral phrase. Both rules then report it. Neither rule reads the other's option or constants, because sibling isolation (`src/architecture.rs`) forbids it. The double report is rare and acceptable: it reveals a chain, since removing the override's pointer leaves the base's pointer for `deferral-docstring` to flag. (The earlier variant where Rule 2 skipped deferral phrases stays dropped.)
- **D17. Philosophy: no docstring, or a complete one (user decision).** A method's own docstring replaces the inherited one in every documentation tool and is never merged with it (`01_understand.md` §4). So an override docstring is either absent or self-contained. This **departs from Google §3.8.3.1** ("a docstring with at least those differences"): a differences-only docstring is Google-compliant but leaves hover, `help()` and generated docs without the base contract. Every docstring these rules accept is still Google-compliant, since a complete docstring contains "at least those differences". Each rule's `what_it_does` says so in one sentence, so that a Google-style reader is not surprised by Rule 3.

### Rule 1: `deferral-docstring`

- **D5. Whole-docstring matching only.** Rule 1 fires only when the *entire* docstring is a deferral phrase. A deferral followed by more text (`"""See base class. Also invalidates the cache."""`) is not a pure pointer, and prefix matching hits hook instructions (`"Override this method to …"`) (E7). Under D17 such a docstring is incomplete when it omits sections the base has; that is Rule 3's finding (`generate_text_iterable`, `02_references.md` §3.0), with a different fix (complete it).
- **D6. Deferral phrases are a configurable list** (`banned`, `extend-banned`, `remove-banned`), because monorepo alone uses phrases a fixed grammar missed (`"Implements the _EditLeaf interface."`, `"Overrides base definition."`). Two tokens make the list cover named references:
  - `{base}`: a reference to a class in the enclosing class's bases, or to an ancestor of it defined in the file (`Base`, `pkg.Base`, `` `Base` ``, `:class:`Base``);
  - `{method}`: the method's own name, optionally with `()` and a `:meth:` role.

  A named reference to a class that is **not** a base is a delegation pointer, often the method's only documentation (`Testdir.spawn: "See :meth:`Pytester.spawn`."`), and is not flagged (E8).

### Rule 2: `copied-base-docstring`

- **D12. Rule 2 matches** when the normalized override docstring equals the normalized inherited docstring, **or** equals its normalized first paragraph (summary, `PythonDocstring::summary_paragraph()`). It does not use prefix-of-word, summary-line-only or token-overlap matches (E1–E4).

### Rule 3: `partial-override-docstring`

- **D18. Contract sections.** The section kinds Rule 3 requires are `Args`, `Returns`, `Yields` and `Raises` (`DocstringSectionKind`, parsed by `PythonDocstring::sections()` in `src/code_lint/ast/python/docstrings.rs`). `Attributes` and `Other` (`Example:`, `Note:`, `See Also:`, …) are not part of a method's call contract (E11). Kinds are compared, never header text, so `Arguments:` satisfies `Args:` (E12).
- **D19. Trigger.** An `@override` method with its own docstring, whose inherited docstring (D9) has at least one contract section kind that the override's docstring lacks. One finding per docstring, listing every missing kind, because the fix is one edit of one docstring.
- **D20. An unchanged signature still needs `Args:`.** Under D17 the override's docstring must stand alone, so when the inherited docstring documents parameters, the override documents them too, even if it changes none. The same holds for `Returns:` / `Yields:` / `Raises:`. If repeating them adds nothing, the override needs no docstring at all: that is the suggestion's second branch.
- **D21. Main target: a one-line summary under a sectioned base.** All 3 measured findings (and the 4 summary copies E15 hands to Rule 2) are one paragraph with no section (`"""Creates a session."""` over a base documenting five parameters, a return value and a `ValueError`). Rule 3 still checks multi-paragraph and sectioned overrides: a docstring that documents `Args:` but drops the base's `Raises:` hides that part of the contract in the same way.
- **D22. `Returns` and `Yields` satisfy each other** (E13). A decorated generator or an override returning an iterator documents the same result under either header; this matches `undocumented-return-value` D12 (`complex_function_docstring/03_design.md`).
- **D23. A summary starting with `Return`, `Returns`, `Yield` or `Yields` satisfies `Returns` / `Yields`** (E14), as `undocumented-return-value` D13 trusts it: Google allows "Returns …" summaries when they suffice.
- **D24. Abstract-method boilerplate.** A base `Raises:` section whose only entries are `NotImplementedError` (`DocstringEntry::bare_name()`) documents the abstract base, not the contract; Rule 3 does not require `Raises:` for it (E16).
- **D25. One finding per problem with the other two rules.**
  - With Rule 2: a docstring equal to the inherited docstring has all its sections, so it never reaches Rule 3. A docstring equal to the inherited **summary paragraph** is both a copy and incomplete; Rule 3 skips it (E15) and Rule 2 reports it, because its fix is certain (remove: the copy adds nothing). Rule 3 decides this from the two docstrings with the shared helpers `normalized()` and `summary_paragraph()`, not by reading Rule 2.
  - With Rule 1: a deferral phrase over a sectioned in-file base is also incomplete. Rule 3 cannot recognize a deferral phrase without reading Rule 1's option (sibling isolation), so both report it, as in D16. Measured: 0 of 92 deferrals had a documented in-file base. Both fixes remove the docstring, which clears both findings. Alternative in Q1.
  - With other Batch D rules: `undocumented-parameter` and `undocumented-return-value` exempt `@override` methods (their E2, E12, E23; helper H4), so no other rule checks an override docstring's completeness. Ruff `D417` checks `Args:` against the signature, not against the base; a project enabling it may see both on an override with a partial `Args:` section and no other sections.
- **D26. Placeholder `{sections}`** (new, added to `naming_and_message_style_guide.md` §3 and the vocabulary test in the implementation change): the missing section headers, canonical (`Args:`, `Returns:`, `Yields:`, `Raises:`) in that order, each in backticks, joined by " or ", rendered bare. "has no `Args:` or `Returns:` section" reads correctly for one or several kinds.

## 3. Rule 1: `deferral-docstring`

Name: the flagged thing is the docstring that defers. Alternatives considered: `see-base-class-docstring` (an identifier in the name; wrong once the list is extended), `pointer-docstring` (less clear).

```text
[deferral-docstring] Docstring of `{function}` only refers to the documentation of another definition.
  Rationale: A method's own docstring replaces the inherited one in `help()`, generated documentation and IDE hovers, so the pointer hides the very docstring it refers to.
  Suggestion: Remove the docstring so that the base docstring is inherited.
```

- Option: `ListOption`, `kind: Banned`, doc "Phrases that make up a whole deferral docstring; `{base}` stands for a base class and `{method}` for the method's name." Default (Python):
  `see base class`, `see the base class`, `see parent class`, `see superclass`, `see base class docstring`, `see base class for details`, `same as base class`, `same as parent class`, `inherited`, `docstring inherited`, `inherited from base class`, `overrides base class method`, `overrides parent class implementation`, `overrides base definition`, `see {base}`, `see {base}.{method}`, `same as {base}.{method}`, `overrides {base}.{method}`, `implements {base}.{method}`, `inherited from {base}`, `implements the {method} interface`.
- Doc example: flagged `@override def run(self): """See base class."""` → fixed: no docstring.
- `known_problems`: a phrase in a language other than English, or a wording not in the list, is not flagged; extend the list. Undecorated overrides are not inspected (D8).

## 4. Rule 2: `copied-base-docstring`

Alternatives: `duplicated-base-docstring`, `inherited-docstring-copy`. "Copied" names the pattern without implying how it happened.

```text
[copied-base-docstring] Docstring of `{function}` repeats text of the docstring it inherits from `{class}`.
  Rationale: Without it, `help()`, generated documentation and IDE hovers show the complete inherited docstring; the copy shows the same text or less, and drifts when either contract changes.
  Suggestion: Remove the docstring so that the base docstring is inherited.
```

`{class}` is the ancestor whose docstring is repeated (D9). No options: equality has nothing to tune. "Repeats text of" covers both D12 matches (whole docstring, summary paragraph).

- `known_problems`: only ancestors defined earlier in the same file are seen. A copy of a docstring from an imported base class is not flagged; in the monorepo subset, 21 of 715 documented `@override` methods have a documented in-file ancestor (`02_references.md` §3.0).

## 5. Rule 3: `partial-override-docstring`

Name: the flagged thing is an override docstring that covers only part of the inherited contract. Three words, a pattern rather than a verdict. Alternatives: `incomplete-override-docstring` (reads as a verdict), `override-docstring-missing-section` (five words, a fix-shaped phrase).

```text
[partial-override-docstring] Docstring of `{function}` has no {sections} section, unlike the `{class}` docstring it replaces.
  Rationale: A method's own docstring replaces the inherited one in `help()`, generated documentation and IDE hovers, so readers of the override no longer see that part of the contract.
  Suggestion: Add the missing sections if the docstring states how the override differs from the base; otherwise remove the docstring so that the complete one is inherited.
```

Rendered: "Docstring of `create_session` has no `Args:` or `Returns:` or `Raises:` section, …". `{class}` is the ancestor of D9. The suggestion states the criterion because the rule cannot know whether the override changes behavior (`naming_and_message_style_guide.md` §2.4).

- No options besides the enforcement mode.
- `what_it_does`: flags an `@override` docstring without an `Args:` / `Returns:` / `Yields:` / `Raises:` section that the inherited docstring has; one sentence on D17 (stricter than Google §3.8.3.1); hand-off: a copy of the inherited summary is `copied-base-docstring`'s finding.
- Doc example: base `def create(self, name: str) -> Session:` documenting `Args:` and `Returns:`; flagged override `"""Creates a session in memory."""`; fixed: the override's summary plus both sections.
- `known_problems`: only ancestors defined earlier in the same file are seen (as Rule 2). Section kinds are compared, not entries: an override `Args:` that omits one of the base's parameters is not flagged (Q2). A deferral phrase over a sectioned in-file base is also reported by `deferral-docstring` (D25).

## 6. Exemptions

Each gets one dedicated, mutation-tested `pass` case.

| ID | Rule | Not flagged | Mutation that must break it |
| :--- | :--- | :--- | :--- |
| E1 | 2 | Override docstring extends the inherited one (the base text is a strict prefix: "Set an option. Extends … by validating …") | match on "one is a prefix of the other" |
| E2 | 2 | Same summary paragraph, different further paragraphs | compare summaries only |
| E3 | 2 | Adapted copy with a changed key word (`"Return a new datetime …"` vs `"Return a new date …"`) | add a token-overlap threshold |
| E4 | 2 | Override is a word-prefix of the inherited docstring but not its whole summary paragraph (cuts a sentence: a narrowing) | match on "override is a prefix of base" |
| E5 | 2, 3 | Nearest in-file ancestor defining the method has no docstring, and no further ancestor documents it | None (lead, 2026-10-12): listed as unmutated, since no lookup edit makes a docstring appear; the skip is protected by the `undocumented_nearer_ancestor_skipped` `fail` cases and the mutation "stop at the nearest ancestor defining the method" |
| E6 | 2, 3 | Base class not resolvable in the file: imported, written as `module.Base`, or defined *after* the subclass | resolve bases by name anywhere in the file |
| E7 | 1 | Deferral phrase followed by more text (`"See base class. Also invalidates the cache."`), or a sentence that only starts like one (`"Override this method to …"`) | match the phrase as a prefix |
| E8 | 1 | Named reference to a class that is not a base (`"See :meth:`Pytester.spawn`."`) | let `{base}` match any dotted name |
| E9 | 1, 2, 3 | Functions outside a class body (module-level, nested in a function) | check every function |
| E10 | 1, 2, 3 | Method overriding an in-file base without `@override` (`"""See base class."""`, a copy, a summary over a sectioned base) | drop the `@override` check |
| E11 | 3 | Inherited docstring whose only sections are non-contract (`Attributes:`, `Example:`, `Note:`) | require every section kind of the inherited docstring |
| E12 | 3 | Override uses another header of the same kind (`Arguments:` for `Args:`, `Return:` for `Returns:`) | compare header text instead of kind |
| E13 | 3 | Override documents `Yields:` where the base has `Returns:` (and the reverse) | require the exact kind |
| E14 | 3 | Override summary starts with `Returns …` / `Yields …` and the base has `Returns:` | drop the summary check |
| E15 | 3 | Override docstring equals the inherited summary paragraph (Rule 2's finding) | drop the equality check |
| E16 | 3 | Base `Raises:` lists only `NotImplementedError` | require `Raises:` regardless of its entries |

Not exemptions but required `pass` regressions: an `@override` method with no docstring (all rules); a complete override docstring that rewrites every section of a sectioned base (Rule 3); a one-line `@override` docstring stating a difference over a base with no contract section (the legacy linter's case, accepted by all three rules: hover shows only the difference, but nothing structured is lost that the file can show). Required `fail` regression for D20: an override with the base's exact signature and a new summary, missing the base's `Args:`.

## 7. Helper API

| Helper | Owner | Status |
| :--- | :--- | :--- |
| `PythonClassInfo::methods()`: the `def` / `async def` statements directly in the class body, in source order | lead | Adopted batch-wide; replaces the planned `constructor()` (that is `methods()` filtered to a non-`@overload` `__init__`) |
| `PythonDocstring::normalized()` (D7: cleandoc dedent, whitespace collapse, case-fold, trailing-period strip), `PythonDocstring::summary_paragraph()`, `PythonDocstring::summary()` and `DocstringEntry::bare_name()` | lead (`src/code_lint/ast/python/docstrings.rs`) | Adopted. Rule 3 uses `summary()` (E14), `bare_name()` (E16), `normalized()` and `summary_paragraph()` (E15) |
| `PythonDocstring::sections()`, `DocstringSectionKind` | existing (`docstrings.rs`) | Used by Rule 3 as is |
| `PythonFunctionSignature::has_override_decorator()`: public, import-resolved, same `OVERRIDE_DECORATOR_PATHS` | this worker, Phase 4 | Implemented |
| `local_base_classes(classes, index) -> Vec<usize>` (D10): for each simple-name base, the last class of that name defined before class `index` | this worker, Phase 4 | Implemented |
| `local_ancestors(classes, index) -> Vec<usize>` (D9): the in-file ancestors, depth-first, left to right, each once | this worker, Phase 4 | Implemented (lead, 2026-10-12); also used by Rule 1's `{base}` |
| `inherited_method_docstring(classes, index, name)` (D9, D11): the first in-file ancestor documenting `name`, with that docstring | this worker, Phase 4 | Implemented. Shared by Rules 2 and 3, which live in separate files, so it goes in `src/code_lint/ast/python/classes.rs` |

The Rule 1–specific stripping (backticks, Sphinx roles, `()`) stays in the rule. `{base}` reads the raw base texts from `PythonBaseClass::name`.

## 8. Open questions for the user

- **Q1. Rules 1 and 3 on the same pointer.** A deferral phrase over a sectioned in-file base gets two findings (D25), because Rule 3 cannot recognize deferral phrases. Accept it as for D16 (recommended: 0 of 92 measured deferrals had a documented in-file base, and one fix clears both), or have Rule 1 yield whenever the in-file inherited docstring has a contract section? The second gives one finding but makes a pointer unreported when Rule 3 is disabled.
- **Q2. Entry-level completeness.** Rule 3 compares section kinds. Should it also flag an override `Args:` that omits a parameter the base documents (compared by `DocstringEntry::bare_name()`)? Recommended: not now. `undocumented-parameter` already defines entry matching against the signature; extending it to `@override` methods with a docstring would be the more coherent home.
- **Q3. Ship Rules 2 and 3 with single-file reach?** They see 3% of documented `@override` methods in the monorepo subset (imported bases dominate). Recommended: yes; they are exact, cheap, and the project index (§9) extends them later without changing their semantics.
- **Q4. Enforcement-mode default**: `RequireExplanation` like the canonical rule, or `Ban`? A pointer is never needed. A copy may be deliberate (inline docs for source readers). A partial docstring may be a deliberate Google-style difference.
- **Q5.** Three rules (D4), or fewer with parameterized messages? Rules 1 and 2 share a fix; Rule 3 does not.
- **Q6.** Add the `docstrings` topic for the batch (lead decision, shared with the other Batch D rules).

## 9. ROADMAP suggestions (not part of these rules)

- **Project index for base classes.** Resolve imported base classes across files (Pylint does it through astroid inference) so that `copied-base-docstring` and `partial-override-docstring` see cross-file bases. In the previous monorepo sample, ~250 generic one-line `@override` docstrings probably restate or truncate an imported base. Until then this is a documented gap (Rules 2 and 3 `known_problems`).
- **Duplicated docstrings across sibling implementations** (S3), narrowed to siblings whose shared in-file base declares the method **without** a docstring. Fix: move the docstring to the base declaration. This is a different fix, so a separate rule.
- **`name-restating-docstring`** (S4): a docstring whose words only repeat the function name. General rather than override-specific; Google §3.8.2.1 condemns it for tests.
- **Rust analogue**: `/// See trait.`, a trait method's doc copied onto an `impl` item, or an `impl` doc dropping the trait doc's `# Errors` / `# Panics` sections. rustdoc already shows the trait's docs on undocumented impl items. Detectable when the trait is in the same file.
- **Not pursued**: The legacy linter's "one-line docstring on `@override`" proxy (`01_understand.md` §3).
