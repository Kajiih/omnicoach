# Command Lint Shell AST & Test Harness: 01 Understand

> [!NOTE]
> **Status: IN REVIEW (Phase 1)** — Formalizes problem statement, goals/non-goals, and the architectural options for `D1` (Parser), `D2` (`ParsedCommand` vs per-command contract), and `D3` (`command_rule_test!` and executed examples).

## 1. Problem Statement & Current Defects (`F1`–`F5`)

In [`src/command_lint/command.rs`](../../../src/command_lint/command.rs), shell command parsing currently relies on `ast-grep-core` + `ast-grep-language` (`tree-sitter-bash`) plus `shell_words::split`:

| # | Defect in current `command_lint` | Concrete Failure / Impact |
| :-- | :--- | :--- |
| **F1** | **Legacy `ast-grep` + C `tree-sitter-bash` dependency.** `command_lint::command` is the last module using `ast-grep-core` and `ast-grep-language`. | Pulls in a C toolchain (`cc`), prevents `command_lint` from compiling on `wasm32`, and keeps `AST_GREP_OWNERS` in `tests/architecture_conformance.rs`. |
| **F2** | **I/O redirections are mis-parsed as positional CLI arguments.** `InterceptedCommand::parse_single` runs `shell_words::split` on every CST child after `command_name`, including `file_redirect` and `heredoc_redirect`. | In `jj edit > out.txt` or `jj edit 2>&1`, `arguments` becomes `["edit", ">", "out.txt"]`, so `extract_jj_edit_revision` treats `">"` as the revision argument instead of `"@"`. Conversely, rules cannot distinguish `cat foo > bar` (file redirect) from `echo ">" bar` (string argument). |
| **F3** | **Leading environment assignments (`FOO=bar cmd`) are dropped.** `children.find(|c| c.kind() == "command_name")` consumes and discards all `variable_assignment` nodes before `command_name`. | Rules cannot inspect leading `VAR=val` prefixes on a command, and `cmd.span` starts at `FOO=bar` while `program_name` and `arguments` omit it. |
| **F4** | **Lost structural context & unparsed command substitutions in strings.** `parse_all` flattens commands into `Vec<InterceptedCommand>` with `Vec<String>` args and a single `(usize, usize)` byte range, and never parses inside `"$(jj edit d123)"`. | Rules cannot inspect connectors (`&&`, `||`, `;`, `&`, `\n`), pipelines (`|`, `|&`), subshells `(...)`, command substitutions `$(...)` / `` `...` ``, quote kinds, or per-argument/per-redirect spans. |
| **F5** | **No `command_rule_test!` harness or executed command examples.** `assert_command_rule_snapshot` calls `InterceptedCommand::parse_all(input).remove(0)` (panicking on 0 commands and silently ignoring any 2nd command in `cmd1 && cmd2`), and command rules declare `examples: &[]`. | Command rules lack `rule_test!`'s span verification, multi-occurrence repeat checks (`{cmd} && {cmd}`), registry test enforcement, and `--explain` examples. |

---

## 2. Scope: Goals & Non-Goals

### Goals

- **G1 (Pure-Rust SOTA Shell AST)**: Replace `ast-grep` and `shell-words` in `command_lint::command` with `brush-parser = "0.4.0"`, removing `ast-grep-core`, `ast-grep-language`, and `shell-words` from `Cargo.toml` and enabling `command_lint` on all targets (including `wasm32`).
- **G2 (Domain AST Encapsulation)**: Encapsulate `brush-parser` inside `command_lint::command` (enforced by `DEDICATED_AST_OWNERS` in `tests/architecture_conformance.rs`, removing `AST_GREP_OWNERS`), exposing only Omni domain types (`ParsedCommand`, `InterceptedCommand`, `CommandArg`, `EnvAssignment`, `CommandRedirect`, `CommandContext`).
- **G3 (Fix Argument vs. Redirect/Env Conflation & Inner Command Substitution Extraction)**: Separate `env_assignments`, `arguments`, and `redirects` on each command, and recursively extract commands inside `$(...)` and `` `...` `` word pieces (even inside double quotes).
- **G4 (`command_rule_test!` & Executed Command Examples)**: Add `command_rule_test!` in `src/test_utils.rs` (enforced by `tests/registry.rs`), support executed shell examples in `RuleDoc` (rendered as ```` ```sh ```` in `--explain`), and migrate `edit-of-described-commit`.

### Non-Goals

- **NG1 (Runtime Shell Evaluation)**: Do not evaluate environment variables (`$VAR`), globs (`*.py`), or arithmetic (`$((1+1))`) against the host OS; expose their syntactic structure and literal/expansion pieces for static analysis.
- **NG2 (New Command Rule Batch)**: Do not bundle the new `jj` or shell command rules into this foundation track; land the AST and test harness cleanly first.

---

## 3. Decisions & Open Questions to Validate

### D1 (Validated): Use `brush-parser = "0.4.0"` Encapsulated in `command_lint::command`

- `brush-parser` (`0.4.0`, MIT, Rust 2024) is the only actively maintained, MIT-licensed, pure-Rust POSIX/Bash parser with both a full command AST (`brush_parser::ast`) and a word-expansion sub-parser (`brush_parser::word`) that tracks source spans (`SourceSpan` / `WordPieceWithSource`).
- See [02_references.md](02_references.md) §1 for the full comparison against `deno_task_shell`, `tree-sitter-bash`, `yash-syntax`, `flash`, and `conch-parser`.

### D2 / Q2: `ParsedCommand` Container vs. Bare `InterceptedCommand` in `CommandRule`

See [02_references.md](02_references.md) §2 for the full investigation. Summary of the two options:

- **Option 2A (`ParsedCommand` container + `parsed.commands()` — mirrors `ParsedFile` in `code_lint`)**:
  - `ParsedCommand::new(raw_cmd: &str)` parses the command string once and stores:
    - `source: &str` (stored once, instead of cloning `raw_string: String` into every `InterceptedCommand`),
    - `has_syntax_error: bool` (whether `brush-parser` rejected malformed shell syntax, used by `command_rule_test!` just like `!parsed.has_syntax_error()` in `run_code_rule`),
    - `commands: Vec<InterceptedCommand>` (every simple command in execution order, including commands inside pipelines `|`, chains `&&`/`||`/`;`, subshells `(...)`, compound blocks, and `$(...)` / `` `...` `` substitutions).
  - `CommandRule::check` has signature `fn(&Self, &ParsedCommand<'_>, &dyn JjClient) -> Vec<Diagnostic>`, and `CommandRule` provides `rule.diagnostic_at(parsed, span, params)` (mirroring `CodeRule::diagnostic_at_node`).
  - *Why this matters*:
    1. **Cross-command state**: Rules that need to see earlier commands in the same line (e.g., `cd subdir && ./script.sh` changing `Cwd` for executable existence, or `jj new main && jj squash -m "msg"` changing whether `@` is described) can inspect `parsed.commands()` in sequence.
    2. **Single-command rules stay trivial**: A rule that checks each command independently either iterates `for cmd in parsed.commands()` or uses a helper on `CommandRule`.
    3. **Zero `raw_string` cloning & no `.remove(0)` test bug**: Eliminates cloning `raw_string` into every `InterceptedCommand` and fixes `test_utils` so every command in a multi-command test snippet is checked.

- **Option 2B (Keep `CommandRule::check` taking `&InterceptedCommand` directly)**:
  - `run_command_lint` loops `for cmd in parsed.commands()` and calls `rule.check_command(cmd, &jj_client)` for each command individually.
  - Simpler for single-command rules, but a rule cannot see sibling commands in the same `CommandLine` (such as `cd dir && ...` or `jj new && ...`) and script-wide shape rules must deduplicate per-command findings.

### D3 / Q3: Executed Examples for Command Rules (`RuleDoc` + `command_rule_test!`)

See [02_references.md](02_references.md) §3 for the full investigation. Summary of how command rule examples work and the two options:

- **What the problem is**:
  1. `RuleDoc.examples` is `&'static [Example]`, where `Example` currently has `pub language: Language` (`Python` | `Rust`). Command rules have `languages: &[]` and their examples are shell commands (`sh`).
  2. Unlike code rules (whose examples are self-contained Python/Rust snippets), a `jj` command rule's example (`flagged: "jj edit kpqxywon"`, `fixed: "jj new kpqxywon"`) only triggers when `JjClient` knows that revision `kpqxywon` has a non-empty description!
- **Option 3A (Unify `Example` / `CommandExample` and execute via `command_rule_test!`'s `jj` fixture)**:
  - Make `Example.language` optional (or add `Example::shell(flagged, flagged_span, fixed)` / `CommandExample`), rendered by `rule_catalog.rs` with a ```` ```sh ```` fence when `language` is `None` (or for command rules).
  - In `command_rule_test!(RULE, jj: [ ... ], { pass: [...], fail: [...] })`, the generated `documented_examples` test runs `RULE` on the documented `flagged` and `fixed` commands using **the same `FakeJjClient` declared in `command_rule_test!`**.
  - Update `tests/registry.rs` to require every `CommandRule` to document 1 example (so `omnicoach --explain edit-of-described-commit` renders a verified `## Example` section).
- **Option 3B (Keep `examples: &[]` on `CommandRule` for now)**:
  - Leave `RuleDoc` and `Example` untouched in this track; `command_rule_test!` only generates `pass` and `fail` tests (with the `{cmd} && {cmd}` repeat check).
