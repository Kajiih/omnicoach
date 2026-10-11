# Trivial override docstring: 02 References

> [!IMPORTANT]
> **Status: PENDING** lead validation.

## 1. Existing tools

None of them flags a pointer, copied or incomplete override docstring. They matter because they decide what happens once the docstring is removed, and whether overrides are marked at all.

| Tool | Behaviour on overrides | Source |
| :--- | :--- | :--- |
| Ruff `D102` (undocumented-public-method) | "This rule exempts methods decorated with `@typing.override`, since it is a common practice to document a method on a superclass but not on an overriding method in a subclass." | https://docs.astral.sh/ruff/rules/undocumented-public-method/ |
| Ruff (any rule) | No rule requires `@override` on an implicit override (lead probe). | lead probe, `explicit_override.py` |
| Ruff DOC rules (`DOC201`, `DOC202`, `DOC402`, `DOC403`, `DOC501`, `DOC502`, `DOC102`, preview) | Check sections against the body; nothing compares an override with its base. | lead's gate (`batch_d_status.md`) |
| pydoclint | No code targets duplicated, pointer or incomplete override docstrings (categories DOC0xx–DOC6xx are parsing, args, returns, class/constructor, yields, raises, class attributes). With the default `--skip-checking-short-docstrings = True`, a one-line `"""See base class."""` or `"""Creates a session."""` is skipped entirely, so pydoclint stays silent on exactly the pointer and the summary-only override. | `pydoclint_violation_codes.md`, `pydoclint_config_options.md` §6 |
| Pylint `C0116` (missing-function-docstring) | Does not report a missing docstring on a method that overrides an ancestor's method (astroid inference, cross-file) or that is decorated with `typing.override` / `typing_extensions.override`. Nothing about redundant or incomplete docstrings. | `pylint/checkers/base/docstring_checker.py`, `visit_functiondef` (`report_missing=not overridden`) |
| Pylint `docparams` | Checks documented params against the signature; no comparison with the base. | lead's gate |
| mypy `explicit-override` (opt-in error code) | Flags an override that lacks `@override`: base-class methods, `object` dunders, `ABC` implementations, methods of explicit `Protocol` subclasses. Not `__init__`, not structural `Protocol` implementations. | `mypy/errorcodes.py`; lead probe |
| Pyright `reportImplicitOverride` (off by default) | Same coverage as mypy's in the lead probe. | `pyright-internal/src/common/configOptions.ts`; lead probe |
| ty `missing-override-decorator` | Same idea, in Astral's type checker. | `ty_python_semantic/src/types/overrides.rs` |
| Pyright hover, `inspect.getdoc`, Sphinx autodoc, griffe | The method's own docstring replaces the inherited one; with none, the first docstring by name lookup through the MRO is shown. Never merged (griffe merges only with opt-in `merge: true`). | `01_understand.md` §4 |

**Prerequisite configuration.** No external configuration replaces these rules. The rules only inspect `@override` methods, so a project needs a type checker that makes every override explicit; otherwise an undecorated override escapes them:

```toml
[tool.mypy]
enable_error_code = ["explicit-override"]

# or
[tool.pyright]
reportImplicitOverride = true
```

Optionally, Ruff `D102` (`extend-select = ["D102"]`) keeps requiring docstrings on public methods while exempting `@override` ones, which is what makes "remove the docstring" an accepted fix.

**Where omnicoach can improve on them:**

- pydoclint's short-docstring skip silences exactly the pointer and the one-line override; no tool flags them.
- No tool compares an override's docstring with the inherited one, so copies and incomplete overrides go unnoticed everywhere.
- The legacy linter's proxy flags by line count, which does not separate these cases (`01_understand.md` §3).

## 2. Style guides

Google Python Style Guide §3.8.3.1, quoted in `01_understand.md` §2: a docstring is unnecessary on an `@override` method unless the method refines the contract, in which case "a docstring with at least those differences is required". `"""See base class."""` is shown as trivial. These rules deliberately require more than "at least those differences" when a docstring is present (`01_understand.md` §5). The guide condemns name-restating docstrings only for test methods (§3.8.2.1). PEP 257 says nothing on overrides.

## 3. Corpus measurements

Two measurement runs exist:

- **Previous worker, all methods** (§3.1–§3.5 below, kept for the signal analysis): scripts `override_docstrings.py` and `sample.py` in `brain/abd9be3e-…/scratch/`. That directory no longer exists, so these figures cannot be re-run.
- **This revision, `@override` scope** (§3.0): script `override_scope.py` in `brain/de3c54d3-…/scratch/` (raw output `out_g3.json`, `out_g3_samples.jsonl`, `out_uv.json`). It parses each file with `ast`, takes methods with an `override` decorator (last segment, as the legacy linter), resolves bases by D10 and the inherited docstring by D9 (`03_design.md`), and applies the three rules' matching.

### 3.0 `@override` scope (this revision)

Corpora: a google3 subset of six directories of the previous sample (`learning/gemini/_agents`, `intelligence/magiclab`, `gdm/evals/crowdpix`, `fitbit/platform/agent`, `learning/agents/orcas`, `third_party/py/absl`) read from `/google/src/files/head`, and the uv cache. The CPython stdlib and Debian `dist-packages` have no `@override` method (previous run), so they are not re-measured.

| | google3 subset | uv cache |
| :--- | ---: | ---: |
| Files | 6,515 | 6,691 |
| `@override` methods | 1,759 | 51 |
| … with a docstring | 715 | 45 |
| … with a docstring and a documented in-file ancestor (D9) | 21 | 3 |
| … of which the ancestor's docstring has an `Args:` / `Returns:` / `Yields:` / `Raises:` section | 9 | 0 |
| `deferral-docstring` findings | 92 | 0 |
| `copied-base-docstring` findings | 12 | 0 |
| `partial-override-docstring` findings (after E15) | 3 | 0 |

Precision, every finding read:

- **`deferral-docstring`: 92/92.** All are exactly `"""See base class."""`. None has a documented base in the same file.
- **`copied-base-docstring`: 12/12.** Eight exact copies of a one-paragraph base (`get_unique_document_key` on two siblings, `get_feedback`, …), four copies of the summary paragraph of a sectioned base (`BatchableEmbedder._embed`, `OrcasStorageSessionServiceForRepl.get_session_or_raise`, …). The four are also P3 and go to this rule only (E15).
- **`partial-override-docstring`: 3/3**, all one-line docstrings under a sectioned base, in `learning/agents/orcas` and `intelligence/magiclab`:
  - `create_session`: `"""Creates a session."""` hides the base's `Args:` (five parameters, ACL constraints), `Returns:` and `Raises: ValueError`.
  - `get_session`: `"""Gets a session."""` hides that `None` means "absent **or** unreadable", the base's central warning.
  - `generate_text_iterable`: `"""See Client.generate_text_iterable, batched version."""` is a pointer plus a difference (not a deferral, E7) and hides `Args:` and `Yields:`.

Raw P3 count before E15 is 7: 3 above and the 4 summary copies. None overlaps `deferral-docstring` in this sample.

Reach: `copied-base-docstring` and `partial-override-docstring` see 21 of 715 documented `@override` methods (3%), because most bases are imported. The project-level base index (`03_design.md` §8) is what would make them broadly useful. An estimate of the reach once explicit-override adds `@override` to undecorated in-file overrides was not re-run (the previous run found 77 of 89 same-file exact copies on undecorated methods, §3.3).

### 3.1 the legacy linter's proxy on real `@override` code (previous worker, google3 sample)

Corpus: 16 directories with many `@override` (11,107 files, 94,107 methods, 2,304 `@override` methods). Of 2,304 `@override` methods, 1,397 have no docstring and 907 have one: 680 one-line (all flagged by the legacy linter) and 227 multi-line (none flagged).

| One-line docstring on `@override` | Count | Under `01_understand.md` §5 |
| :--- | ---: | :--- |
| Whole docstring is a deferral phrase (`See base class.`) | 146 | P1 |
| Equal to, or the summary of, the same-file base docstring | 16 | P2 |
| Only restates the method name | 5 | S4, not pursued here |
| Anything else | 513 | sample of 50: ~25 state a difference (`"Deletes a session in Vertex AI without in-memory fallback."`, `"See base class. Resolves dynamic subagents directly from CRACS API."`); ~25 are generic and probably restate a base docstring defined in another file (`"Validates the study config."`) |

Under the old reading of Google, the ~25 differences were false positives of the legacy linter. Under §5 they are incomplete when the base documents sections (P3), which only a cross-file base index can confirm. The legacy linter's fix for them (pad to multiple lines) is still wrong: it does not ask for the hidden contract.

For scale: code search finds `"""See base class."""` in **349** google3 Python files that also contain `@override`, and in **~15,000** files without `@override` (the pre-`@override` convention, out of scope under D8 until those files adopt explicit overrides).

### 3.2 Signal S1: deferral phrases (previous worker, all methods)

| Corpus | Whole docstring matches a deferral phrase |
| :--- | ---: |
| google3 sample | 370 (205 on methods with `@override` or a same-file base, 165 on methods whose base is in another file) |
| stdlib + dist-packages + uv | 84 |

Distinct texts in the google3 sample: `See base class.` 327, `See parent class.` 28, then named references (`See os.PathLike.` 2, `See Prompter.render().`, `See prompt_builtins.generate.` 4, …).

Precision depends entirely on matching the **whole** docstring:

- Whole-docstring match on generic phrases: every sampled match is a pure pointer (precision ~100%; 92/92 in §3.0).
- **Prefix** match is wrong for this rule: 19 google3 docstrings start with `See base class.` and then state a difference (`"See base class.\n\nAs a side effect, remembers snapshot_number …"`). They are not pure pointers; when the base documents sections they are P3 (`generate_text_iterable` in §3.0). In the other corpora, prefix matching on `Override…` / `Implement…` hits base-class hook instructions (`"Override this method to support alternative .mo formats."`), operator docs (`"Implementation of + operator - returns And"`) and verbs (`"Overrides the default Keyword chars"`).
- **Named references** are only pointers to the *inherited* docstring when the name is a base class: `FileIO.readinto: "Same as RawIOBase.readinto()."` (base) versus `Testdir.spawn: "See :meth:`Pytester.spawn`."` and `OptionGroup.destroy: "see OptionParser.destroy()."` (delegation to an unrelated class, where the pointer is the only documentation).
- Phrases a fixed grammar missed in google3: `"Implements the _EditLeaf interface."` (3), `"Overrides parent class implementation."`, `"Overrides base definition."`. The list has to be configurable.

### 3.3 Signal S2: copy of the base docstring (previous worker, all methods, base in the same file)

| Relation of override text to base text | google3 | stdlib | dist | uv | Sampled precision |
| :--- | ---: | ---: | ---: | ---: | :--- |
| exact | 89 | 65 | 115 | 39 | 36/36 true positives |
| equal after whitespace / case normalization | 0 | 0 | 3 | 2 | 3/3 |
| override is a prefix of the base (`contained_in_base`) | 18 | 7 | 39 | 1 | ~80%: mostly the base's summary paragraph alone (`"Get a proxy of the DBus signal."`); false positives are deliberate narrowings that cut a sentence (`ipaddress.IPv4Network.is_global` drops "or iana-ipv6-special-registry") |
| same summary line, different body | 20 | 14 | 60 | 20 | low: most add override-specific paragraphs (`logging.FileHandler.emit`) |
| override extends the base (base is a prefix) | 7 | 5 | 7 | 2 | 0: refinements (`ConfigParser.set`: "Set an option. Extends RawConfigParser.set by validating …") |
| token Jaccard ≥ 0.8 | 7 | 12 | 10 | 16 | 0: adapted copies that change the key word (`date` → `datetime`, `object` → `service`, port → SSL port) |

Most same-file copies (77 of 89 in google3) were on methods **without** `@override`, typically implementations of an `ABC` or `Protocol` defined above them. Under D8 they enter scope once the project enables explicit-override (§1).

### 3.4 Signal S3: duplicated docstrings across siblings (previous worker)

Groups of methods with the same name and the same non-empty docstring in one file, whose classes share a direct base (not `object` / `Protocol` / `ABC`), excluding groups S1 or S2 already catch: google3 36 groups (15 with the base in the file), stdlib 19, dist 50, uv 720 (684 with a base in another file, mostly generated code such as the Dropbox SDK's `is_<tag>` methods). Sampled: the method is often absent from the base (`mailbox.Maildir.list_folders` / `MH.list_folders`; `Mailbox` does not declare it), so "document it once in the base" means adding an abstract method, a design change rather than a docstring fix. Low precision for a docstring rule, small reach.

### 3.5 Signal S4: name-restating docstrings (previous worker)

One-line docstrings whose words, minus stop words, are a subset of the method name's words: google3 5 of 680 one-line `@override` docstrings, 109 other methods; under 200 across the other corpora. Mostly real (`"Get descendants."` on `get_descendants`), but not specific to overrides, and Google condemns it only for test methods.

## 4. Rust analogue

rustdoc shows the trait method's documentation on an undocumented `impl` method, so `/// See trait.` or a copied `///` block on a trait impl method is the same antipattern. No Clippy lint targets it (`missing_docs` does not apply to trait impl items). Recorded as a ROADMAP suggestion in `03_design.md` §8.
