# Agent Guidelines

## Verification Scope

Run the narrowest checks that cover what you changed.

- `cargo test` and `cargo clippy --all-targets` are fast; run them as usual.
- When adding or modifying a rule or AST helper, verify branch and test adequacy scoped to what you changed:
  - Diff-scoped or file-scoped mutation testing: `cargo mutants --cap-lints=true --in-diff -- --lib` or `cargo mutants --cap-lints=true -f <path> -- --lib`.
  - Single-build branch coverage across the library: `cargo +nightly llvm-cov --lib --branch`.
- When testing that a rule is suppressed by configuration (`ignore`, `per-file-ignores`) or an inline directive, use a fixture that triggers the rule when unsuppressed (avoid identifiers in a rule's default allowlist such as `x`, `i`, `j`, `f` in `single-letter-name`) and pair the check with an unignored positive control.

## Fixing Lint Findings

- Fix a finding by applying its `Suggestion`, not by reshaping the code until it falls under the rule's threshold or outside its pattern (for example, trimming a trailing `\n` so a literal has fewer escaped newlines). Such edits keep the problem the rule targets and only hide it from the linter.
- Keep values (literals, fixtures, expected outputs) unchanged when fixing a finding, unless the value itself is wrong.
- When the suggested fix would make the code worse, suppress the finding with `omni:ignore [<rule>] -- <reason>` instead of working around it.

## Architecture and Module Placement

- Treat adding or updating a rule as an opportunity to improve the shared abstractions and component boundaries, never as a narrow patch: when an existing `ast`, `semantic`, or `policy` helper bakes in one caller's policy or lacks a general fact, refactor the abstraction rather than bolting on flags or callbacks ([ADR 012](docs/dev/decisions/012_module_placement_by_abstraction.md) P6–P7).
- Place every item in the module whose abstraction vocabulary states its contract, regardless of who calls it or what it is built on: see [ADR 012](docs/dev/decisions/012_module_placement_by_abstraction.md) (alongside [ADR 006](docs/dev/decisions/006_architectural_dag_and_conformance.md) for component dependencies and [ADR 011](docs/dev/decisions/011_colocated_abstraction_ordering.md) for intra-scope declaration ordering).

## Structural Navigation & Queries with `omnicoach graph`

When a task needs the structure of code you have not read yet (where a symbol lives, who calls it, how a file's helpers cluster), run `omnicoach graph` before multi-step `rg` or `read_file` exploration. Run one `omnicoach graph` command per tool call rather than chaining several with `&&`, so stdout stays within inline tool limits:

- **Intra-file call hierarchy & `[Lk]` clusters (when the target file is known):** run `omnicoach graph <file>` to see all `[Lk]` levels, `roots=[...]`, line spans (`:start..end`), and intra-file call edges in one view. Do not also run `slice` on internal functions of that same file unless you need their cross-module callers.
- **Component and module map of a directory:** run `omnicoach graph <dir>`; directories and multi-file targets default to module depth (pass `--depth public` or `--depth full` explicitly for more detail on a small subtree).
- **Locate a symbol or module across the repo (when the defining file is unknown):** run `omnicoach graph find <pattern>`.
- **Cross-module callers (with call-site lines), callees, and signature contracts:** run `omnicoach graph slice <symbol>...` (pass multiple symbols in one call to batch within the shared inline budget; add `--hops 2` for 2-hop cross-module or large-file call chains, and `--path <dir>` to restrict the search).
