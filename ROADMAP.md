# Omni Toolkit Roadmap

This document serves as the single source of truth for architectural milestones, performance investigations, and planned ecosystem integrations for Omni.

Items here represent design areas and technical directions to evaluate rather than fixed implementation mandates.

---

## Architecture & Conformance

Design: [ADR 006](docs/dev/decisions/006_architectural_dag_and_conformance.md). Enforcement: `src/architecture.rs` (graph definition) and `tests/architecture_conformance.rs` (source-tree conformance).

- **Conformance CST Edge Cases (Watch List)**:
  - *Current*: `summarize_rust_file` skips `macro_definition` bodies (production macros `architecture_component!` and `rule_test!` expand either to a doc attribute or inside `#[cfg(test)]`) and assumes paths do not start with a root-anchored leading `::` (`::omni::...` or `::ast_grep_core::...`).
  - *Target*: If production `macro_rules!` macros calling cross-component helpers (`$crate::...`) are introduced outside `src/lib.rs`, or if root-anchored `::` paths appear, extend `summarize_rust_file` to scan `macro_rule` body token trees and normalize leading `::` prefixes.
- **Non-transitive DAG edges**: every edge is transitive today, so a component reaches everything its dependencies reach. A "private" edge (a dependency that dependents do not inherit) would let the graph express isolation rules that currently need bespoke conformance checks.
- **Compiler-enforced subtree visibility (`pub(in crate::...)` / `pub(super)`)**: tighten item visibility to `pub(in crate::code_lint)`, `pub(in crate::command_lint)`, or `pub(super)` wherever component boundaries align with a directory subtree, so cross-domain access fails in `rustc` (`E0603`) before conformance tests run.
- **`command_lint` Shell AST & Test Harness Migration (dropping `ast-grep` completely)**:
  - *Current*: `src/command_lint/command.rs` (`InterceptedCommand::parse_all`) walks `SupportLang::Bash` for `kind() == "command"` and splits child arguments via `shell_words::split`. This flattens connectors (`&&`, `||`, `;`, `&`) and pipelines (`|`), loses subshell `(...)` and command substitution `$(...)` context, drops leading `FOO=bar` environment assignments and I/O redirects (`2>&1`, `<<EOF`), loses per-argument spans and quote kinds, and keeps `ast-grep-core` + `ast-grep-language` (`tree-sitter-bash`) in `Cargo.toml`. Command rules also lack a declarative `command_rule_test!` macro and executed examples.
  - *Disqualified crates*: `yash-syntax` (`0.25.0`), `mystsh` (`0.0.3`), and `bash-ast` are `GPL-3.0-or-later` (incompatible with `omni-lint`'s `MIT` license); `conch-parser` (`0.1.1`) is unmaintained (2018) and POSIX-only.
  - *Viable parser options to choose from when starting the `command_lint` expansion*:
    - **Option A (`brush-parser = "0.4.0"`, `MIT`, pure Rust PEG)**: 0 C compilers (makes Omni 100% pure Rust); full typed POSIX/Bash AST out of the box (`CommandList`, `AndOr`, `Pipeline`, `SimpleCommand` with `prefix` assignments/redirects and `suffix` words/redirects, `CompoundCommand` for subshells/loops/conditionals, and `word::parse` for quote/expansion pieces). Trade-offs: adds ~15 pure-Rust transitive crates (`cached`, `bon`, `uuid`, `peg`, `tracing`, `indenter`, `utf8-chars`); `SourcePosition.index` is a 0-based `char` index requiring a 3-line byte-offset helper; strict PEG parser returns `Err` on malformed shell syntax.
    - **Option B (Direct `tree-sitter` + `tree-sitter-bash = "0.25"`, `MIT`)**: 0 new crates in `Cargo.lock`, error-tolerant CST, and native UTF-8 byte spans (`node.byte_range()`). Trade-offs: keeps 1 C grammar (`cc` build script) and requires a ~150–200 line CST-to-domain lowering module in `command.rs` plus `shell-words` for quote stripping.
    - **Option C (In-tree shell lexer + recursive-descent parser)**: 0 external crates and 0 C compilers, native byte spans and quote kinds, ~350–450 lines of parser code owned in `src/command_lint/`.
  - *Target*: Replace `ast-grep` in `command_lint::command`, remove `ast-grep-core` and `ast-grep-language` from `Cargo.toml`, expose connectors/pipelines/subshells/env assignments/redirects/compound statements on the command AST, and add `command_rule_test!` in `src/test_utils.rs`.
- **Standalone Rust crate extraction (`omni-architecture`)**:
  - *Context & Ecosystem*: A crates.io survey (`cargo_pup`, `archaven`, `zoning`, `archunit`, `rust_arkitect`, `cargo-modules`) shows no published crate combines in-file compiler-checked component declarations (`architecture_component!`), compile-time acyclicity by declaration order (`define_architecture!`), subtree inheritance, sibling isolation, single canonical paths, and external-crate encapsulation (`zoning` is conceptually closest with ordered zones and module seals; `cargo_pup` runs inside nightly `rustc` over a `pup.ron` rule list). External download volume for Rust architecture crates is tiny (`cargo_pup` ~5k, `archaven` ~860), so extraction is driven by reuse across our own Rust repositories rather than broad adoption.
  - *Target*: Once the DAG design items above settle and a second Rust repository needs it, extract the zero-dependency `define_architecture!` / `architecture_component!` macros (`[dependencies]`) and the `ra_ap_syntax`-backed CST conformance runner (`[dev-dependencies]` behind a `check` feature) into a workspace/standalone crate, moving `summarize_rust_file` out of `src/code_lint/ast/rust.rs`.
- **Multi-Language Architectural Conformance in `.omnilint.toml` (Python & Rust)**:
  - *Context*: Python projects (such as a downstream Python project linted by omnicoach) currently rely on external tools like `import-linter` (layers, independence, and forbidden contracts in `pyproject.toml`) or `tach`, neither of which unifies DAG reachability, sibling subtree isolation, external-package adapter encapsulation, and colocated in-file component declarations under one model.
  - *Design Split (Per-File vs. Cross-File)*: When component names map deterministically to module path prefixes declared in topological order under `[architecture]` in `.omnilint.toml` (acyclicity checked at config load), four policies reduce to embarrassingly parallel per-file checks over `ParsedFile`'s existing `ImportMap`: (1) **DAG reachability**, (2) **sibling subtree isolation** (e.g. `strategies.*` never importing each other), (3) **external library encapsulation** (restricting specific third-party packages to designated adapter components), and (4) **colocated component headers** (requiring `# omni:component = "..."` or `architecture_component!(...)` to match the file's component path). Cross-file checks (unused components, pure namespace routers, single public re-export paths) remain a project-level pass.
  - *Trigger*: After a downstream Python project establishes its initial layer and independence boundaries via `import-linter` and untangles existing upward imports (`execution` → `engine`, `models` → `engine`, `utils` → `engine`/`models`), use that project as the concrete consumer to design the `[architecture]` config schema and rule set.

## Rule Engine & Declarative Rules

- **Unified Single-Pass AST Visitor Dispatch**:
  - *Context & Problem*: Today, each registered rule implements `check_file` independently and executes its own AST search or traversal over the file. At 16 rules on ~8k lines, rule passes take ~180ms total (~11ms/rule). However, this scales linearly as $O(\text{files} \times \text{rules})$: at 100+ rules, traversing the syntax tree 100 times per file becomes a multi-second bottleneck.
  - *Investigation & Design Questions*:
    - Investigate how high-throughput linters dispatch rules. (e.g. Ruff's `Checker` uses a single AST walk with match arms and $O(1)$ bitset checks; Biome groups subscriptions by `SyntaxKind`; Clippy fuses passes into combined callbacks).
    - Can we establish a rule interest declaration (e.g. target node kinds) early in the trait lifecycle without breaking existing rule independence?
    - How do we handle rules that need multi-stage context (like `environment_variable_in_function` traversing upward or `too_many_assertions` counting inner blocks) within a unified traversal?
  - *Trigger*: When the code rule registry approaches ~30–40 rules, or when rule evaluation time exceeds parse time.
- **Rule Naming Canonicalization (`heck`)**:
  - *Target*: Support case-insensitive and format-tolerant rule selection (matching `SingleLetterName`, `single-letter-name`, and `single_letter_name` interchangeably).
  - *Trigger*: When adding declarative AST rule files or multi-rule alias configurations.
- **Rule Autofix Engine (`diffy` / `similar`)**:
  - *Target*: Extend `CodeRule` and `CommandRule` with optional auto-fix transformations. Support `--fix` and `--fix --dry-run` with in-memory unified diff previews before writing changes to disk.
  - *Trigger*: When implementing the first batch of auto-fixable rules (e.g., replacing `logging.error` with `logging.exception`).
- **Multiline Decorator & Attribute Span Awareness for `omni:ignore`**:
  - *Current*: `compute_effective_target_line` in `src/code_lint/suppression.rs` advances `end_target_line` across contiguous lines starting with `@`, `#[`, `//`, or `#`, handling single-line decorators and attributes. However, multiline decorators or attributes whose continuation lines do not start with `@` or `#[` stop the line scan early.
  - *Target*: Use AST decorated/attributed node spans in the suppression resolver so `omni:ignore` placed above a multiline decorator/attribute block suppresses diagnostics on the underlying declaration.
- **Generic Container Base-Type Matching (`identical-positional-types`)**:
  - *Current*: Positional parameter types are compared by exact formatted annotation string (`dict[str, int]` != `dict[str, float]`).
  - *Target*: Optionally normalize or group generic collection/mapping containers (`dict[...]`, `Mapping[...]`, `list[...]`, `Sequence[...]`) so multiple positional mappings or sequences are flagged even when their inner type arguments differ. Equivalent spellings of one type (`Optional[str]` vs `str | None`, `List[int]` vs `list[int]`) are also compared as different today.
- **Escaping Nested Scopes (`environment-variable-in-function`)**:
  - *Current*: The boundary exemption (`main`, `from_env`, ...) is inherited by every scope declared inside it, which is correct for nested functions and closures but also exempts a class declared inside a boundary whose methods later escape (returned, registered as a callback).
  - *Target*: Treat a `class` / `impl` declared inside a boundary as a barrier that resets the exemption, once a real-world occurrence justifies the added language-specific complexity.
- **Import-Aware Qualified Call Resolution (`src/code_lint/semantic/calls.rs`)**:
  - *Current*: Literal banned-call entries, dedent wrappers, decorators, class bases, and collection annotations are resolved through a per-file import map (`ast::resolve_name`, documented in [docs/name_resolution.md](docs/name_resolution.md)): an imported name matches only by its canonical path (`import typing as t; t.cast(...)` matches `typing.cast`; `from sqlalchemy import cast` no longer matches `cast`), a module-level definition never matches, and an unimported name matches as written. Only module-level Python imports and root-level Rust `use` items are considered; imports inside functions or `mod` blocks, function parameters, assignment aliases, and the receiver of `<callee>().<method>` entries are not resolved.
  - *Target*: Evaluate scope-aware resolution (nested imports, parameter/fixture receivers such as `mocker.patch`, `monkeypatch.setattr`, `loop.create_task`), modeled on Ruff's `SemanticModel::resolve_qualified_name`.
- **Dependency-Aware Rule Activation (`pyproject.toml` / `Cargo.toml`) & Loguru Format Enforcement (`LoggerPrintfFormatRule`)**:
  - *Context*: Some rules only make sense when a project uses a specific library ecosystem. The legacy linter's `LoggerPrintfFormatRule` targets `loguru`: while `"..." % ...` is covered by Ruff `G002` (`logging-percent-format`) and `UP031` (`printf-string-formatting`), multi-argument `logger.info("User %s", user)` is required in stdlib `logging` (enforced by Ruff `G001`–`G004`), yet in `loguru` it is a silent data-loss bug (`str.format` ignores unused positional arguments when no `{}` is present; today only Pylint `E1205` with `[tool.pylint.logging] logging-format-style = "new"` and `logging-modules = ["loguru"]` checks this).
  - *Target*: Investigate dependency-aware rule activation (inspecting `pyproject.toml` / `uv.lock` / `Cargo.toml` or project-wide imports) or a configurable logging format mode so a `printf-log-format` rule enforcing `{}` placeholders over `%s` in logger calls can be enabled automatically when `loguru` is installed. (Implementation note: unparenthesized printf regexes must exclude the `' '` space flag so ordinary English percentages like `"100% complete"` or `"50% discount"` do not match `% c` / `% d`.)
- **Third-Party Suppression Directive Hygiene (`# noqa`, `# type: ignore`, `# pyright: ignore`)**:
  - *Context*: Today `src/code_lint/suppression.rs` audits only `omni:` directives (`unscoped-suppression`, `unexplained-suppression`, `unused-suppression`), whereas the legacy linter's `check_file_line_comments` also required inline explanations (`-- reason`) and specific rule codes on `# noqa`, `# type: ignore`, and `# pyright: ignore`.
  - *Target*: Evaluate extending suppression audits (or adding a dedicated directive-hygiene audit) to enforce scoped codes and explanations on Ruff and type-checker suppression comments, avoiding the need to duplicate Ruff rules solely to enforce Omni's explanation discipline.
- **Rule test harness (`rule_test!` in `src/test_utils.rs`)**:
  - *Repeat check vs. scope-wide rules*:
    - *Current*: every `fail` case also runs as `{code}\n{code}` in one module and must report exactly the two copies' spans. The second copy redefines every top-level name, so a rule that reasons about the other definitions in a scope sees a different program than the case states. For module-level declaration-order rules such as `statement-after-main-guard`, any statement preceding the first `if __name__ == "__main__":` guard in the snippet appears after the first guard in the second copy, so `fail` snippets must start with the guard.
    - *Target*: a repetition that stays strict for these rules. Candidates: a `RepeatCheck::DistinctNames` variant, modeled on `DistinctLiterals`, that renames the second copy's definitions and their references (solves redefinitions, not ordering); isolating each copy in its own scope where the language allows it (a Rust `mod`; Python has no equivalent that keeps module semantics); or requiring both copies' spans to be among the findings rather than equal to them (fits ordering rules, weaker). Today's escape hatch is a unit test on the collector (`rule_test!` docs, "Writing cases").
  - *Multi-diagnostic `fail` cases*: a `fail` case expects exactly one span, so 3+ copies and a constant plus two uses are only unit-tested on the collector (`repeated-literal`). A `fail` case listing several spans would cover them (also needed by `nested-function`).
  - *Collocating exemption verification with `rule_test!` and retiring `scripts/exemptions/`*:
    - *Current*: `scripts/exemption_mutations.py` and the 8 cluster modules under `scripts/exemptions/` (~3.4k lines of Python) map every `pass` case of all 42 code rules to exact string replacements in Rust source files and recompile `cargo test --lib` per mutation. While this audit surfaced ~15 real bugs and dead exemptions across the rule corpus, keeping a parallel Python catalog of whitespace-sensitive Rust source snippets and test case names creates high ongoing maintenance friction: any refactor, formatting change, or renamed `pass` case requires shotgun surgery across Rust and Python, and many mutations reach into shared AST internals rather than toggling rule-level exemptions.
    - *Target*: Replace the external string-replacement catalog with collocated, zero-rebuild checks inside `rule_test!` (and/or on-demand `cargo mutants --in-diff` for touched files):
      1. **In-memory framework exemption checks**: For exemptions owned by the rule declaration (`EnforcementMode::RequireExplanation`, allowlist/denylist entries, numeric thresholds), `rule_test!` can automatically re-run `pass` cases with comments stripped or options tightened in-memory during `cargo test` in milliseconds.
      2. **Collocated contrast pairs in `rule_test!`**: Express `pass` cases alongside the minimal edit to the *input snippet* (not the Rust implementation) that turns the snippet into a `fail` case (e.g. changing `def from_env()` → `def fetch_env()`, removing `@override`, or changing `tuple[int, ...]` → `list[int]`), proving inside `cargo test` that the `pass` case would fail without the exempted construct.
      3. **Diff-scoped mutation testing (`cargo-mutants`)**: Use `cargo mutants --in-diff` when modifying a rule or AST extractor to catch untested branches without maintaining static source-edit catalogs.
  - Related: "Examples for suppression audits and command rules" (§5).
- **Rule dependencies and soundness**:
  - *Status*: Understood and explored: [01_understand.md](docs/dev/rule_dependencies/01_understand.md), [02_references.md](docs/dev/rule_dependencies/02_references.md), [03_design.md](docs/dev/rule_dependencies/03_design.md). Both committing to a design and implementing it are deferred until a larger rule corpus lands, so we do not overfit the abstractions to today's 8 Omni ↔ Omni edges (D13).
  - *Context*: Rules relate through detection (`Partitions`, `Equivalent` / `Subsumes` / `Overlaps`), fixes (`Chain`, `Cycle`, `Contradiction`, `DivergentAdvice`) and soundness (`ReliesOn`, `Delegates`). These relations live only in prose, and nothing checks that one rule's fix does not trigger another rule.
  - *When resumed*: Do not blindly implement `03_design.md`. Re-inventory the expanded rule corpus, challenge the prototype findings (`Partition` constants, active-voice edges, `Concept` intermediates, compile-time rule names, executable witnesses, and the Option D fix-conflict harness), and look for simpler or stronger abstractions before committing (`03_design.md` §4).
  - Related: "Typed overlap / sources field" (§5).

## Candidate Rules & Rule Follow-ups

Candidates and known-gap follow-ups to prioritize, not commitments.

- **`repeated-index-access` follow-ups** (Python, Rust):
  - *Named record for sparse positional access*: reads needing more `_` placeholders than `repeated-index-access` allows (`row[0]`, `row[7]`) → `NamedTuple` / dataclass / struct (or `csv.DictReader` for CSV rows).
  - *Tuple-returning functions*: functions returning tuples of ≥3 elements → `NamedTuple` / dataclass / struct.
  - *Multi-field tuple structs* (Rust): tuple structs with ≥2 fields → named-field struct (tuple structs reserved for newtypes).
  - *`re.Match` group indexing* (Python): `m[1]`, `m[2]` → `a, b = m.groups()`.
  - *Rust slice patterns*: `arr[0]`, `arr[1]` on fixed-size arrays → `let [a, b] = arr;`.
  - *Macro arguments*: reads inside expression-like macros (`format!`, `assert_eq!`, `vec![]`) are not inspected because `ra_ap_syntax` leaves them as flat `TokenTree` tokens (pass case `known_gap_macro_arguments_not_inspected`). Supporting them needs a multi-token span in `AstNode` (`cmd.span.0` is several tokens, not one node) or re-parsing the arguments as expressions.
  - *Receiver rebinding*: `a = p[0]; p = nxt(); b = p[1]` groups two different values as one receiver (both languages). Needs binding awareness (split the group at each rebinding).
  - *Shadowing*: a closure parameter (`|t| t.1`) or comprehension variable reusing the receiver's name is grouped with the outer receiver (both languages).
  - *Receiver normalization*: receivers are grouped by source text, so `(t).0` vs `t.0`, `len((xs))` vs `len(xs)`, and chains split across lines are not recognized as the same receiver.
- **Signature & attribute collection type rule follow-ups** (`concrete-collection-*`, `mutable-collection-*`, `specific-collection-parameter`, `nullable-collection-return`):
  - *Type alias resolution*: `type IntList = list[int]` or `IntList: TypeAlias = list[int]` used in annotations is treated as an unknown generic/identifier until per-file type-alias resolution is added.
  - *Cross-file caller and subclass mutation tracking*: return and attribute mutation checks (`collect_locally_mutated_return_functions`, `collect_public_class_attributes`) operate within a single file (`G3`); cross-module mutations rely on the default `require-explanation` mode.
  - *String annotation resolution*: string annotations (`"list[int]"`) are not parsed (pass case `known_gap_string_annotation_not_parsed`). Import aliases are covered by "Import-Aware Qualified Call Resolution".
  - *Callee resolution for returned values*: `collect_locally_mutated_return_functions` matches callees by bare name, so mutating `cfg.get(...)` exempts every function named `get` in the file (pass case `known_gap_same_named_callee_exempts_function`).
  - *Header-wide explanation scope*: one `RequireExplanation` comment above a `def` silences every finding on that header, across rules. Framework-wide behavior; per-rule scoping would need comment-to-rule association.
  - *Unified use-site classifier*: `is_safe_readonly_parameter_reference` (mutation) and `record_reference_capability` (capability) classify the same uses separately and can diverge. Merge into one `classify_use`; the `python.rs` unit tests cover both.
  - *Body walk cost*: body walkers recurse without a depth bound and run once per parameter, and each rule rebuilds the same file-wide data. Share one per-function use index if profiling shows a cost.
- **Declaration ordering follow-ups** ([ADR 011](docs/dev/decisions/011_colocated_abstraction_ordering.md)):
  - *Python import-time / definition-time call reachability*: in Python, a function called at module import time (`TABLE = _build_table()`), inside a decorator expression (`@_trace`), in a parameter default (`def f(x=_default()):`), or during `class` body execution must be defined *above* the executing statement, along with all of its transitive callees, to avoid `NameError`. Static tracking of import-time-executed call trees would let `private-before-public-function`, `uncolocated-helper`, and `callee-before-caller` exempt helpers forced above their consumers by Python's top-to-bottom execution model.
  - *Mixed type + function module roots*: in modules whose public API consists primarily of exported types (`pub struct` / `impl` or `class`) rather than top-level `pub fn`s, module-level private helpers called exclusively by a single `impl` block or `class` currently count `impl`/`class` references only when the module also defines at least one top-level `pub fn` (bridging). Treating exported `impl` blocks / `class` definitions as first-class public roots in module-level `CallableScope`s would enforce `private-before-public-function` and `uncolocated-helper` between types and their private free helpers even when no top-level `pub fn` exists.
  - *Non-`fn` helper items*: the call-cluster rules order only `fn` items, so a private `struct` / `enum` / `const` / `static` and its `impl` blocks (Python: private classes and module constants) can sit anywhere, such as above the public entrypoint that uses it (`UnwrappedMultilineFinder` in `ast/python/strings.rs`, `BindingVisitor` in `ast/python/scopes.rs`; both fixed by hand). Treat such items as private callables that their users reference, so they follow the same placement rules as private helper functions.
  - *Bindings inside Rust macros*: macro arguments are unparsed tokens, so a bare identifier there that matches a sibling `fn` counts as a reference. This is needed for functions passed as values (`matches!(e, X(v) if v.iter().any(helper))`), but a pattern binding or named argument with the same name (`matches!(x, Some(value) if value > 0)` next to `fn value`) creates a false call edge. Telling them apart needs the macro arguments parsed as expressions or patterns. `self.field` inside a macro is already told apart from `self.method(...)`.
- **`repeated-literal` follow-ups** (Python, Rust):
  - *Values inside exempt macros* (Rust): the whole exempt macro (`assert_eq!`, `format!`, …) is skipped, so `assert_eq!(x, "expected")` repeated in production code is not counted (pass case `known_gap_values_inside_exempt_macros`). Counting only value arguments needs per-macro knowledge of which arguments are format strings.
  - *Negative numbers as separate tokens* (Rust): inside a macro `token_tree` the sign is a bare `-` token, so the number is skipped rather than counted as `N` (pass case `known_gap_negative_numbers_in_macros`).
  - *Unpacked and chained constants* (Python): `A, B = 1, 2` and `A = B = 1` are not recognized as constant definitions; their values count as inline uses.
  - *Typed template placeholders*: constants such as `CALLEE = "callee"` exist only to name a template placeholder once. A typed placeholder (enum or typed key) would remove them.
  - *Multi-diagnostic `rule_test!` cases*: see "Rule test harness".
- **`no-manual-enum-name-map`** (Python, Rust — tip `#protobufs`):
  - *Detection*: A dict literal where every entry is `'NAME': X.Y.NAME` (string key equals the value's last attribute). Suggest `Enum.Value(name)` (protobuf), `Enum[name]`, or `Enum.__members__`.
  - *Rust*: `match` arms mapping `"Alpha" => Kind::Alpha` → derive `strum::EnumString`.
- **`no-overprecise-float-in-tests`** (Python, Rust — tip `#keep_it_simple`):
  - *Detection*: Float literals in test code with more significant digits than a configurable threshold (e.g. >6).
  - *Overlap*: Clippy `excessive_precision` only flags digits beyond `f64` representability, not unreadable test values.
- **`nested-class`** (Python — companion of `nested-function`):
  - *Detection*: a `class` defined inside a function or method body. Suggest moving it to module level and passing captured values to its constructor.
  - *Context*: `nested-function` no longer flags the methods of such a class (pass case `method_of_local_class_not_flagged`), since the local class, not each method, is the design choice to review.
- **Time-unit literal arithmetic** (extension of `primitive-duration` — tip `#rationale`):
  - *Detection*: `24 * 60 * 60`, `60 * 60`, `86400`, `3600` → `timedelta` / `Duration`.
- **Import alias conventions** (`import x as y`, `use x as y`):
  - *Context*: Naming rules (`single-letter-name`, `abbreviated-name`, `type-suffixed-name`, `primitive-duration`) skip all imports, including aliased imports.
  - *Investigation*: Evaluate how much is already covered by Ruff's `flake8-import-conventions` (`ICN001` `unconventional-import-alias`, `ICN002` `banned-import-alias`) and Pylint (`PLC0414` `useless-import-alias`), and whether a dedicated multi-language import-alias rule is warranted in Omni.
- **`error-log-in-except` vs. Ruff `TRY400` (`error-instead-of-exception`) & `G201` (`logging-exc-info`)**:
  - *Context*: `error-log-in-except` flags `logging.error(...)` inside `except` blocks (including with `exc_info=True`), overlapping with Ruff `TRY400` + `G201` (whereas Ruff `TRY401` `verbose-log-message` checks the subsequent step of passing the bound exception variable into `logging.exception(...)`). Evaluate whether `error-log-in-except` should be broadened to `$OBJ.error` (like `unmatched-logger-placeholder`) or documented alongside `TRY400`/`G201`.
- *Not pursued* (with external linter alternatives and configuration):
  - Magic numbers in comparisons (Ruff `PLR2004`), bare HTTP status codes (too narrow), path composition (Ruff `PTH`), and test correspondence-signaling / same-value-different-meaning constants (require semantic understanding).
  - `CatchGenericExceptionRule`: covered by Ruff `[tool.ruff.lint] extend-select = ["E722", "BLE001", "TRY002", "TRY203", "TRY400", "S110", "S112"]` (plus `logger-objects = ["loguru.logger"]` and `[tool.ruff.lint.flake8-bandit] check-typed-exception = true`).
  - `EmptyInitRule`: covered 1:1 by Ruff `[tool.ruff.lint] preview = true`, `extend-select = ["RUF067", "INP001"]`, and `[tool.ruff.lint.ruff] strictly-empty-init-modules = true` (or `false` for library facades; add `extend-ignore = ["D104"]` if `pydocstyle` `D` is enabled).
  - `LoggerRedundantExceptionRule`: covered by Ruff `[tool.ruff.lint] extend-select = ["TRY400", "TRY401", "G201", "LOG004", "LOG007", "LOG014"]` (plus `logger-objects = ["loguru.logger"]` and `extend-ignore = ["PLE1205"]` when using Loguru).
  - `LoggerPrintfFormatRule` Branch B (`"..." % ...` in logs): covered by Ruff `[tool.ruff.lint] extend-select = ["UP031", "G001", "G002", "G003", "G004"]` (and Branch A for Loguru today via Pylint `E1205` with `[tool.pylint.logging] logging-format-style = "new"` and `logging-modules = ["loguru"]`).
  - `InstanceAttributeAnnotationRule` Checks 2 & 3: Check 2 conflicts with PEP 526/PEP 591/Pydantic/attrs/`__slots__`; Check 3 is covered by Mypy (`[tool.mypy] enable_error_code = ["no-redef"]`), Pyright (`[tool.pyright] reportUninitializedInstanceVariable = "warning"`, `reportGeneralTypeIssues = "error"`), Ruff (`RUF012`), and Pylint (`W0201`).
  - `DefineBeforeUseRule` (`call-before-definition`), file-wide `constant-after-function`, and rigid module-level item-kind / visibility ordering: top-level undefined/forward references in Python are covered by Ruff `[tool.ruff.lint] extend-select = ["F821", "F823"]` and Pylint `E0601` (`used-before-assignment`), module-level import placement is covered by Ruff `E402` (`module-import-not-at-top-of-file`), and rigid module-level item-kind or constant-before-function ordering (`clippy::arbitrary_source_item_ordering`) breaks cohesion by splitting types and functions from their colocated constants, constructors, errors, and helpers.
  - `AvoidOptionalOrNoneRule` Check 1 (`Optional[T]` syntax): covered by Ruff `target-version = "py310"` and `[tool.ruff.lint] extend-select = ["UP007", "UP045", "RUF013", "RUF036"]`; scalar returns, attributes, and parameters under Check 2 are intentionally not flagged.
  - `quote-wrapped-placeholder` for Rust: not extended to Rust because `std::fmt::Debug` (`{:?}`) is not "quoted `Display`" (for structs and enums implementing `Display` + `#[derive(Debug)]`, `{:?}` dumps internal struct layout or unquoted enum variant names rather than quoting the `Display` output, and for strings `{:?}` forces double quotes and Rust escape syntax).
  - Values formatted into SQL strings (SQL injection): covered by Ruff `[tool.ruff.lint] extend-select = ["S608"]` (`hardcoded-sql-expression`), whose fix is a parameterized query. `quote-wrapped-placeholder` does not special-case SQL.

---

## 2. Performance & Concurrency Architecture

- **Single-Pass Rust CST Extraction to Eliminate Rowan Red-Tree Cursor Allocation Multiplication**:
  - *Status*: **Measured, deferred** (O3). Each Group E Rust rule costs about one full Rowan walk (~850 µs / 18.6k allocations per `descendants_with_tokens()` pass over the 49 KB Rust fixtures). End to end, cargo 0.100.0 takes 2.6 s CPU / 0.48 s wall, so fusing the walks would save a fraction of that. Revisit with a dedicated prototype when large Rust repositories are a target. (Note: memoizing Python `extract_function_signatures` / `collect_class_attributes` on `ParsedFile` was also evaluated and not justified because those types borrow from `ParsedFile` and the remaining cost after `visit_stmt` range pruning is ~1.9 ms per 88 KB.)
  - *Context & Empirical Measurement*: On `rs_real_ast_module` (`annotations.rs`, 23.6 KiB), `linter::end_to_end_all_rules` takes `4.84 ms` and performs **46,843 heap allocations (2.22 MB)**; across the 4 Rust fixtures, `E_dedicated_extractors` accounts for **78.0% of warm Rust rule time (`6.28 ms`) and 68,726 heap allocations (2.76 MB)**.
  - *Root Cause*: `ra_ap_syntax` uses Rowan's green/red tree. Unlike `ruff_python_ast` (where AST nodes live in contiguous `Vec`/`Box` slices and traversing them allocates 0 bytes), each call to `syntax.descendants()` or `syntax.descendants_with_tokens()` on a Rowan `SyntaxNode` dynamically allocates heap-backed red-node cursors (`4,400–8,800` heap allocations per full-file walk on `annotations.rs`). Today, 4 `OnceLock` extractors and 6 Group E rules each perform their own independent `syntax.descendants()` walk.
  - *Target*: Fuse the Rust CST extractors (`call_candidates`, `bindings`, `comment_nodes`, `rust_inline_test_ranges`, `literal_occurrences`, `positional_reads`, `find_unwrapped_multiline_strings`, `collect_functions`, `collect_test_function_assertion_counts`) into a single top-down `syntax.descendants_with_tokens()` visitor pass per file so the Rowan red-tree is materialized only once. Red nodes cannot be cached on `ParsedFile` (`SyntaxNode` is `!Send`/`!Sync`).
- **Remaining Serial Time in End-to-End Runs**:
  - *Context*: CPython 3.14.8 takes ~1.4 s wall for ~7.3 s of CPU on 16 cores (ideal ≈ 0.5 s).
  - *Investigation*: Attribute the serial part between directory discovery (see the directory item below), the final sort, and diagnostic formatting before changing anything.
- **Suppression Fast-Path False Trigger on `"omni:"` Inside String Literals**:
  - *Context & Empirical Measurement*: `SuppressionTracker::from_file` runs at `7.8–15.9 GB/s` with `0` allocations when `"omni:"` is absent, but on `rs_real_test_suite` (`tests/cli.rs`) the substring appears inside test string literals, so the fast path falls through to `collect_comment_nodes` (`521 µs`, `2,213` allocations).
  - *Investigation*: Not pursued yet (O4): no measurement shows it mattering on real repositories. The fallthrough is correct, only slower, and a narrower pre-check (e.g. `"omni:disable"`) must not miss any valid directive.
- *Evaluation protocol for the items above*: each change ships only with a before/after `omni_bench` run on the pinned fixtures (median time + allocation counts) plus an end-to-end `hyperfine` run; any change that adds complexity (new caches, fused visitors, new data on extracted structs) needs a measured win that justifies it, prototyped on its own branch first.
- **Directory Discovery Parallelism & Micro-Run Overhead**:
  - *Current*: File-level analysis runs in parallel via `rayon` (`targets.into_par_iter()`) with deterministic sorting across both plain-text and JSON output, while directory traversal in `collect_directory_candidates` runs single-threaded via `ignore::WalkBuilder::build()`.
  - *Investigation*:
    - Evaluate whether `ignore::WalkParallel` improves directory discovery on large repositories compared to single-threaded collection + `rayon`.
    - Measure `rayon` thread-pool initialization overhead on small repositories to ensure micro-runs and pre-commit hooks are not penalized.
- **Subprocess Batching & Caching (`EnvContext`)**:
  - *Current*: Command rules spawn individual `jj` or `git` CLI calls per evaluation.
  - *Target*: Introduce a shared `EnvContext` struct that pre-fetches and caches repository state (e.g., batching queries into a single `jj log --json` or `git status` invocation) to ensure sub-10ms execution across multiple rules.
- **VCS Error Propagation**:
  - *Current*: VCS client query errors in command rules are swallowed to avoid blocking users on query failures.
  - *Target*: Propagate structured errors or display user warnings when the underlying VCS client fails unexpectedly, distinguishing clean working copies from failed CLI calls.

---

## 3. Performance Measurement, Tracing & Tooling

- **Execution Timing & Observability (`--timings`)**:
  - *Context*: Understanding which rules or pipeline stages dominate execution on a user's machine is essential for performance triage.
  - *Investigation & Design Questions*:
    - Compare a lightweight, zero-dependency `--timings` flag (using `std::time::Instant` around rule executions to produce a sorted table, following oxlint's `--debug timings` or ESLint's `TIMING=1`) against heavyweight runtime tracing.
    - SOTA review shows that full `tracing-subscriber` pipelines pull in substantial dependencies (`sharded-slab`, `regex-automata`, etc.) and are best suited for server/LSP contexts rather than fast batch CLI invocations.
    - Explore what level of timing granularity is useful (per-rule vs. pipeline phase) without penalizing normal runs.
- **CI Instruction-Count Regression Gating (CodSpeed / Valgrind Cachegrind)**:
  - *Context*: Wall-clock assertions on shared CI runners suffer from 15–30% variance.
  - *Target*: Evaluate integrating simulated CPU instruction-count tracking (`codspeed-divan-compat` or Valgrind Cachegrind) once CI performance gating is needed.

---

## 4. Reporting & Diagnostics

- **Feature-Gated Rich Terminal Diagnostics (`miette`)**:
  - *Current*: Fast, zero-dependency printer in `src/diagnostic.rs` outputting standard compiler-style format (`path:line:col: [CODE] message`) and JSON.
  - *Target*: Add an optional Cargo feature (`features = ["miette"]`) that enables rich, syntax-highlighted source snippets with colored squiggly underlines and clickable rule documentation URLs, while keeping the default pre-commit hook binary lightweight and fast.
- **Polished Diagnostic Summaries (`pluralizer` / Native Helper)**:
  - *Target*: Clean grammatical inflection ("1 violation" vs "3 violations") in terminal summary footers, JSON reports, and future JUnit/SARIF export formats.

---

## 5. Tags, Discovery & Documentation

Design rationale: [ADR 007](docs/dev/decisions/007_rule_taxonomy_and_selection.md) (taxonomy and selection) and [ADR 008](docs/dev/decisions/008_rule_documentation_and_discovery.md) (rule docs and discovery). Contributor guide: [docs/dev/tag_guide.md](docs/dev/tag_guide.md).

- **Shadowed-selector config warning**: warn when a `select` or `ignore` entry changes no rule's outcome (needs config warning plumbing).
- **Examples for suppression audits and command rules**:
  - *Current*: Code rules document one executed example per language (`RuleDoc.examples`, run by `rule_test!`). Suppression audits and command rules have no harness that could run an example, so they declare `examples: &[]` (enforced empty in `tests/registry.rs`).
  - *Target*: Give them executed examples once a harness exists. Audits can reuse `Example` (code with suppression comments) and only need a harness. Command rules need another shape (a command line plus a fake `jj` state), hence their own type and harness.
  - *Placement (decided: keep in `RuleDoc`)*: Moving `examples` from `RuleDoc` to `CodeRule` would make empty examples unrepresentable for audits and command rules and drop the emptiness test, without losing any compile check. Rejected for now: it splits the user-facing doc across two places, plumbs examples through `AnyCodeRule` and `RegisteredRule`, and audits (bare `Declaration`s) would need a wrapper type before they could get examples. Revisit when command rule examples land, since they need their own typed field anyway.
- **Discovery & Documentation Follow-ups**:
  - JSON output for discovery commands (`--format json` for `--list-rules` / `--explain`), and `tags` on JSON diagnostics. Include per-language message overrides (`summary` / `rationale` / `suggestion`), not just the base text.
  - Path-aware status in `explain` (evaluating `per-file-ignores` for a given file path).
  - Generated in-repo rule catalog guarded by a golden-file drift test.
  - Styled Markdown rendering in the terminal.
  - JSON Schema for `.omnilint.toml` (editor completion). It must be registry-aware, because `rules` is a free map: per-rule keys and defaults, the threshold bounds each rule uses, language sub-tables, and every rule name and tag as a selector value. Document the `#:schema` directive (Taplo) in the README.
  - Typed overlap / sources field linking rules to equivalent Ruff / Clippy rules.
  - Decide whether plain diagnostics keep printing the full rationale and suggestion on every hit.
  - Investigate subcommands (`rules`, `explain`, a default `check`) instead of the `--list-rules` / `--explain` flags.
- **Later / Out-of-Scope Ideas**:
  - **Abstraction-driven auto-tagging**: importing or using a domain helper (e.g. a logging abstraction) automatically attaches its topic tag to the rule.
  - **Computed `recommended` view**: if curated presets are ever introduced, define `recommended` as a computed view (`exact` ∧ `unopinionated`) rather than a second hand-maintained list.
