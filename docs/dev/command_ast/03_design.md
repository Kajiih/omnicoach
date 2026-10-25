# Command Lint Shell AST & Test Harness: 03 First-Principles Design & Prototypes

> [!NOTE]
> **Status: IN REVIEW (Phase 3)** — First-principles architecture, `brush-parser 0.4.0` integration design, and side-by-side prototypes for `Q2` (`ParsedCommand` & `CommandRule` contract) and `Q3` (`CommandExample` & `command_rule_test!`).

---

## 1. Architectural Placement & Component Boundaries ([ADR 006](../decisions/006_architectural_dag_and_conformance.md), [ADR 012](../decisions/012_module_placement_by_abstraction.md))

### 1.1 Splitting `command_lint::command` by Abstraction Vocabulary (ADR 012 P1–P4)

Today, [`src/command_lint/command.rs`](../../../src/command_lint/command.rs) mixes two distinct levels of abstraction in one file:

1. **Shell syntax & word expansion** (tokenizing and parsing shell scripts into simple commands, environment assignments, redirections, and unquoted words).
2. **CLI flag/option/positional classification** (`ProgramCliSchema` and `ParsedArgs`, which interpret a slice of already-extracted argument words against a program's CLI schema such as `jj`).

Under [ADR 012](../decisions/012_module_placement_by_abstraction.md), `command_lint::command` is structured as a component facade with two focused private submodules:

| Module | Abstraction Vocabulary & Responsibility | Allowed Third-Party Parser |
| :--- | :--- | :--- |
| [`src/command_lint/command.rs`](../../../src/command_lint/command.rs) | Component facade (`architecture_component!(CommandLintCommand)`), re-exporting the domain API. Declares `CommandLintCommand => [Diagnostic]` in [`src/architecture.rs`](../../../src/architecture.rs) (matching `CodeLintAst => [Diagnostic]`). | None directly |
| `src/command_lint/command/shell.rs` | Shell grammar & word AST (`ParsedCommand`, `InterceptedCommand`, `CommandArg`, `EnvAssignment`, `CommandRedirect`, `RedirectKind`, `RedirectTarget`, `ShellScope`). | `brush_parser` (sole owner in the workspace) |
| `src/command_lint/command/cli_args.rs` | CLI flag/option schema & argument classification (`ProgramCliSchema`, `ParsedArgs`) operating purely on `&[CommandArg]`. | None |

### 1.2 Conformance Enforcement (`tests/architecture_conformance.rs`)

- Remove `ast-grep-core`, `ast-grep-language`, and `shell-words` from [`Cargo.toml`](../../../Cargo.toml).
- In [`tests/architecture_conformance.rs`](../../../tests/architecture_conformance.rs):
  - Delete `AST_GREP_OWNERS` and `find_ast_grep_imports` (`command_lint::command` was the last module in the repository importing `ast_grep_*`).
  - Add `"brush_parser"` to `DEDICATED_AST_CRATES` and assert that only `command_lint::command` (and its child modules) may reference `brush_parser`.

---

## 2. Solving the Three `brush-parser 0.4.0` Edge Cases Inside `command::shell`

Inspection of `brush-parser-0.4.0`'s source code revealed three subtle behaviors that `command_lint::command::shell` encapsulates so rule authors never encounter them:

### 2.1 Char Index $\to$ UTF-8 Byte Offset Mapping (`CharToByteMap`)

- **Fact**: `brush_parser::tokenizer` increments `SourcePosition.index` by `1` per Unicode `char` (`tokenizer.rs:571`), whereas `brush_parser::word::parse` (`word.rs`) and `omnicoach::diagnostic::SourceSpan` use 0-based UTF-8 byte offsets.
- **Design**:

  ```rust
  struct CharToByteMap {
      /// `None` when `source.is_ascii()` (byte offset == char index in O(1)).
      /// `Some(offsets)` when non-ASCII characters exist (`offsets[char_idx]` = byte offset).
      non_ascii_offsets: Option<Vec<usize>>,
      byte_len: usize,
  }
  ```

  Every `brush_parser::SourceSpan` is converted immediately to `crate::diagnostic::SourceSpan { start, end }` in UTF-8 bytes at AST construction time.

### 2.2 Exact Spans for `IoRedirect` and `SimpleCommand` via Two-Step Tokenization

- **Fact**: `brush_parser::ast::IoRedirect::location()` is unimplemented (`None` in `ast.rs:1503`), which also causes `CommandPrefix::location()` and `CommandSuffix::location()` to return `None` whenever the first or last item is a redirection.
- **Design**:
  1. Call `brush_parser::tokenize_str_with_options(source, &options)` to get the `Vec<Token>` (where every operator and word token carries its exact `brush_parser::SourceSpan`), then pass `&tokens` to `brush_parser::parse_tokens(&tokens, &options)`.
  2. For each `IoRedirect`, extract the target `Word.loc` (or `SubshellCommand.loc` for process substitutions) and pair it with the preceding redirection operator token in `tokens` to compute the exact `CommandRedirect.span`.
  3. Compute `InterceptedCommand.span` as the hull (`start.min(..)..end.max(..)`) across `program.span`, all `env_assignments`, all `arguments`, and all `redirects`.

### 2.3 Accurate Word Unquoting & Recursive Command Substitutions via `brush_parser::word::parse`

- **Fact**: `brush_parser::unquote_str` (`tokenizer.rs:1323`) naively strips `\` and `'` even inside single quotes.
- **Design**:
  - Run `brush_parser::word::parse(word.value.as_str(), &options)` on every `ast::Word`.
  - Reconstruct the unquoted literal text from `WordPiece`s (`Text`, `SingleQuotedText`, `AnsiCQuotedText`, `EscapeSequence`, and nested pieces inside `DoubleQuotedSequence` / `GettextDoubleQuotedSequence`).
  - Whenever a `WordPiece::CommandSubstitution(inner)` (`$(...)`) or `WordPiece::BackquotedCommandSubstitution(inner)` (`` `...` ``) is encountered—whether at the top level of a word or nested inside double quotes (`"$(jj edit kpqxywon)"`)—recursively parse `inner` and add `word_byte_start + piece.start_index + delimiter_len` to every inner span so inner `InterceptedCommand`s have exact byte spans in the outer `ParsedCommand::source`!

---

## 3. Question `Q2`: `ParsedCommand` & `CommandRule` Contract Prototypes

### 3.1 Domain AST Model (`command_lint::command`)

```rust
use crate::diagnostic::{LineColumn, LineIndex, SourceLocation, SourceSpan};

/// A parsed shell command line (single command, pipeline, or compound script).
#[derive(Debug, Clone)]
pub struct ParsedCommand {
    source: String,
    line_index: LineIndex,
    has_syntax_error: bool,
    commands: Vec<InterceptedCommand>,
}

/// A single simple-command invocation extracted in execution order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterceptedCommand {
    /// Full UTF-8 byte span of this simple command (including prefixes, args, and redirects).
    pub span: SourceSpan,
    /// Invoked program word (e.g. `jj` or `/usr/bin/jj`).
    pub program: CommandArg,
    /// Positional and flag arguments (excluding leading env assignments and I/O redirects).
    pub arguments: Vec<CommandArg>,
    /// Leading `NAME=VALUE` or `NAME+=VALUE` environment assignments.
    pub env_assignments: Vec<EnvAssignment>,
    /// Input/output redirections (`> out`, `2>&1`, `<<EOF`, etc.) attached to this command.
    pub redirects: Vec<CommandRedirect>,
    /// Enclosing shell scopes from outermost to innermost.
    pub enclosing_scopes: Vec<ShellScope>,
}

/// A single shell word (program name or CLI argument) with unquoted text and span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandArg {
    /// Unquoted text value (e.g. `feat: msg` for `"feat: msg"`).
    pub text: String,
    /// UTF-8 byte span in `ParsedCommand::source`.
    pub span: SourceSpan,
    /// True if the word contains dynamic expansions (`$VAR`, `$(...)`, `$((...))`).
    pub has_expansion: bool,
}

/// Leading `NAME=VALUE` or `NAME+=VALUE` assignment on a simple command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvAssignment {
    pub name: String,
    pub value: CommandArg,
    pub append: bool,
    pub span: SourceSpan,
}

/// An I/O redirection attached to a command (`> file`, `>> file`, `2>&1`, `<<EOF`, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRedirect {
    pub fd: Option<u32>,
    pub kind: RedirectKind,
    pub target: RedirectTarget,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectKind {
    Read,
    Write,
    Append,
    Clobber,
    ReadWrite,
    DuplicateInput,
    DuplicateOutput,
    OutputAndError,
    AppendOutputAndError,
    HereDoc,
    HereString,
    ProcessSubstitutionInput,
    ProcessSubstitutionOutput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedirectTarget {
    File(CommandArg),
    Fd(u32),
    HereDoc { delimiter: String, body: String, span: SourceSpan },
    HereString(CommandArg),
    ProcessSubstitution { span: SourceSpan },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellScope {
    Pipeline,
    ChainedList,
    Subshell,
    CompoundBlock,
    CommandSubstitution,
    ProcessSubstitution,
    Background,
}
```

### 3.2 Side-by-Side Comparison: Prototype 2A vs. Prototype 2B

#### Prototype 2A (Implemented & Verified): `ParsedCommand` + `CommandEnv` Contract

In [`src/command_lint/contract.rs`](../../../src/command_lint/contract.rs), `CommandRule` mirrors [`CodeRule`](../../../src/code_lint/contract.rs) by operating on `&ParsedCommand` and `&CommandEnv<'_>` (so non-`jj` shell rules never need to import `JjClient`), while providing `rule.diagnostic_at(parsed, span, params)` so rules never construct `SourceLocation` manually:

```rust
pub struct CommandEnv<'a> {
    jj_client: &'a dyn JjClient,
}

impl<'a> CommandEnv<'a> {
    #[must_use]
    pub const fn new(jj_client: &'a dyn JjClient) -> Self {
        Self { jj_client }
    }

    #[must_use]
    pub fn jj(&self) -> &dyn JjClient {
        self.jj_client
    }
}

pub struct CommandRule {
    pub declaration: Declaration<(), &'static [CommandExample]>,
    pub(super) check: fn(&Self, &ParsedCommand, &CommandEnv<'_>) -> Vec<Diagnostic>,
}

impl CommandRule {
    /// Renders a `Diagnostic` at `span` in `parsed` using this rule's violation template.
    #[must_use]
    pub fn diagnostic_at(
        &self,
        parsed: &ParsedCommand,
        span: SourceSpan,
        params: &[(&str, &str)],
    ) -> Diagnostic {
        self.declaration
            .render_diagnostic(params, parsed.to_source_location(span))
    }
}
```

How `edit_of_described_commit.rs` (a `jj`-stateful rule) looks under **Prototype 2A**:

```rust
fn check_command(
    rule: &CommandRule,
    parsed: &ParsedCommand,
    env: &CommandEnv<'_>,
) -> Vec<Diagnostic> {
    parsed
        .commands()
        .iter()
        .filter_map(|command| {
            let revision = extract_jj_edit_revision(command)?;
            let commit_info = env.jj().get_commit_info(&revision).ok()?;
            let is_described_without_conflict =
                !commit_info.description.trim().is_empty() && !commit_info.conflict;
            is_described_without_conflict
                .then(|| rule.diagnostic_at(parsed, command.span, &[("revision", &revision)]))
        })
        .collect()
}
```

And how a **pure shell rule** (non-`jj`, e.g. `redundant-shell-wrapper` or `cd subdir && ./run.sh`) works under **Prototype 2A**:

```rust
fn check_command(
    rule: &CommandRule,
    parsed: &ParsedCommand,
    _env: &CommandEnv<'_>,
) -> Vec<Diagnostic> {
    let mut virtual_cwd = PathBuf::new();
    let mut diagnostics = Vec::new();
    for cmd in parsed.commands() {
        if cmd.program_name() == "cd" {
            if let Some(dir) = cmd.arguments.first() {
                virtual_cwd.push(&dir.text);
            }
            continue;
        }
        // Can inspect `virtual_cwd` updated by prior commands in the same line!
    }
    diagnostics
}
```

#### Prototype 2B: Per-`InterceptedCommand` Contract Only

```rust
pub struct CommandRule {
    pub declaration: Declaration<(), &'static [CommandExample]>,
    pub check: fn(&Self, &InterceptedCommand, &CommandEnv<'_>) -> Vec<Diagnostic>,
}
```

| Evaluation Dimension | Prototype 2A (`&ParsedCommand` + `&CommandEnv`) | Prototype 2B (`&InterceptedCommand` only) |
| :--- | :--- | :--- |
| **Per-command rule ergonomics** | 1-expression iterator (`parsed.commands().iter().filter_map(...)`) | 1-expression `Option::into_iter().collect()` |
| **Cross-command state (`cd dir && ./bin`, `jj new && jj squash`)** | Native — rule sees all commands in execution order | Impossible — rule only sees one isolated `InterceptedCommand` |
| **Whole-line shape rules (`prefix-approvable-shape`)** | Native — can inspect `parsed` once | Awkward — invoked $N$ times for an $N$-command pipeline |
| **Memory & diagnostic plumbing** | Zero `raw_string` cloning; `LineIndex` built once on `ParsedCommand`; `rule.diagnostic_at(parsed, span, ...)` hides `VCS_CONTEXT_NAME` | Must either clone `raw_string` into every `InterceptedCommand` or pass both `(&ParsedCommand, &InterceptedCommand)` |
| **Symmetry with `CodeRule`** | Exact match (`CodeRule::check` takes `&ParsedFile`) | Asymmetric |

---

## 4. Question `Q3`: `RuleDoc` Examples & `command_rule_test!` Prototypes

### 4.1 First-Principles Observation: Why Code Examples Have `language: Language` and Command Examples Do Not

In [`src/rule_declaration/documentation.rs`](../../../src/rule_declaration/documentation.rs), `Example` currently has:

```rust
pub struct Example {
    pub language: Language,
    pub flagged: &'static str,
    pub flagged_span: &'static str,
    pub fixed: &'static str,
}
```

Why does `Example` have `pub language: Language`?

- Because a `CodeRule` declares `languages: &'static [Language]` (e.g. `&[Language::Python, Language::Rust]`) and must provide one example per supported programming language.
- Conversely, `Language` in [`src/rule_selection/taxonomy.rs`](../../../src/rule_selection/taxonomy.rs) represents **source-file programming languages** (used for file-extension dispatch and `.omnicoach.toml` per-language config tables `[rules.<name>.python]` and `[rules.<name>.rust]`).
- A `CommandRule` does not lint source files by extension and has no per-language `.omnicoach.toml` table—its examples are **always shell commands** (`sh`).

### 4.2 Side-by-Side Comparison of Three Prototypes for `RuleDoc` Examples

#### Prototype 3A (Recommended): Generic `RuleDoc<E = Example>` + `CommandExample` (Zero Churn on 46 Code Rules)

1. In [`src/rule_declaration/documentation.rs`](../../../src/rule_declaration/documentation.rs):
   - Add `CommandExample` (without a meaningless `language` field):

     ```rust
     /// Verified before/after shell command pair for a command rule.
     #[derive(Debug, Clone, Copy)]
     pub struct CommandExample {
         /// Shell command that triggers the rule.
         pub flagged: &'static str,
         /// Exact substring within `flagged` that the rule's diagnostic span must cover.
         pub flagged_span: &'static str,
         /// Replacement shell command that satisfies the rule with zero findings.
         pub fixed: &'static str,
     }
     ```

   - Parameterize `RuleDoc<E: 'static = Example>` and `Declaration<Options = (), E: 'static = Example>` with default type parameter `E = Example`:

     ```rust
     pub struct RuleDoc<E: 'static = Example> {
         pub summary: &'static str,
         pub Languages: &'static [Language], // or omitted via CommandRule constructor
         pub rationale: &'static str,
         pub remediation: &'static str,
         pub examples: &'static [E],
     }
     ```

   - When erasing `Declaration<Options, E>` into `DeclaredRule` (`DeclaredDoc`), convert `examples` into `DeclaredExamples`:

     ```rust
     #[derive(Debug, Clone, Copy)]
     pub enum DeclaredExamples {
         Code(&'static [Example]),
         Command(&'static [CommandExample]),
     }
     ```

2. **Impact on existing rules**:
   - **All 46 `src/code_lint/rules/*.rs` files and `src/code_lint/suppression.rs`**: **0 lines changed!** Because `Declaration<Options = (), E = Example>` and `RuleDoc<E = Example>` default `E` to `Example`, existing code rules compile without a single edit.
   - **In `src/command_lint/rules/edit_of_described_commit.rs`**:

     ```rust
     examples: &[CommandExample {
         flagged: "jj edit kpqxywon",
         flagged_span: "jj edit kpqxywon",
         fixed: "jj new kpqxywon",
     }],
     ```

   - **In `src/rule_catalog.rs`**:
     - Renders `DeclaredExamples::Code` with ```` ```python ```` / ```` ```rust ```` fences and `DeclaredExamples::Command` with ```` ```sh ```` fences!

#### Prototype 3B: Change `Example.language` to `Option<Language>` (or `ExampleLanguage`)

- Either changes `Example.language` to `Option<Language>` (touching all 46 `src/code_lint/rules/*.rs` files to write `Some(Language::Python)`) or adds a constructor `Example::shell(...)` with a synthetic enum variant on `Example`.
- Downside: A code rule could accidentally pass a shell `Example` (losing compile-time enforcement that code rule examples have a `Language`), or all 46 code rule files suffer mechanical churn.

#### Prototype 3C: Keep `examples: &[]` on `CommandRule`

- Leaves `RuleDoc` untouched, so `omnicoach --explain edit-of-described-commit` has no `## Example` section.

| Evaluation Dimension | Prototype 3A (`RuleDoc<E = Example>` + `CommandExample`) | Prototype 3B (`Example` with `Option<Language>`) | Prototype 3C (`examples: &[]`) |
| :--- | :--- | :--- | :--- |
| **Type safety** | Compile-time separation: `CodeRule` requires `Example` (with `Language`); `CommandRule` requires `CommandExample` (no `Language` field) | Weaker: code rules could specify `language: None` | N/A |
| **Churn across 46 `code_lint/rules/*.rs`** | **0 files touched** | 46 files touched (or synthetic enum field on every `Example`) | 0 files touched |
| **`omnicoach --explain <command-rule>`** | Renders verified ```` ```sh ```` before/after example | Renders verified ```` ```sh ```` before/after example | No `## Example` section |
| **Executed by `command_rule_test!`** | Yes, using the rule's `FakeJjClient` fixture | Yes, using the rule's `FakeJjClient` fixture | No |

---

### 4.3 `command_rule_test!` Design (`src/test_utils.rs` & `tests/registry.rs`)

`command_rule_test!` mirrors `rule_test!` so every command rule has uniform, declarative test coverage:

```rust
command_rule_test!(
    EDIT_OF_DESCRIBED_COMMIT,
    jj: [
        "kpqxywon" => described("feat: add user authentication"),
        "uyzmzqum" => empty_desc(),
        "@" => empty_desc(),
    ],
    {
        pass: [
            empty_description: "jj edit uyzmzqum",
            whitespace_only_description: "jj edit ws_only",
            default_revision_is_at: "jj edit",
            unrelated_program: "git checkout main",
            other_jj_subcommand: "jj new kpqxywon",
            help_flag_short: "jj edit -h kpqxywon",
            help_flag_long: "jj edit --help kpqxywon",
            redirect_on_empty_at: "jj edit > out.txt",
            unknown_revision_graceful_fallback: "jj edit nonexistent",
        ],
        fail: [
            described_positional: "jj edit kpqxywon" => "jj edit kpqxywon",
            described_with_r_flag: "jj edit -r kpqxywon" => "jj edit -r kpqxywon",
            described_with_revision_long_flag: "jj edit --revision kpqxywon" => "jj edit --revision kpqxywon",
            described_with_equals_syntax: "jj edit --revision=kpqxywon" => "jj edit --revision=kpqxywon",
            described_in_pipeline_with_redirect: "jj log | jj edit kpqxywon > out.txt" => "jj edit kpqxywon > out.txt",
            described_in_command_substitution: "echo \"$(jj edit kpqxywon)\"" => "jj edit kpqxywon",
            described_with_env_prefix: "JJ_USER=alice jj edit kpqxywon" => "JJ_USER=alice jj edit kpqxywon",
        ],
    }
);
```

What `command_rule_test!` generates and verifies automatically:

1. **`documented_examples` test**:
   - Asserts `RULE.decl.doc.examples` is non-empty.
   - For each `CommandExample`:
     - Runs `RULE` on `example.flagged` with the macro's `FakeJjClient` and asserts that it produces a diagnostic whose span text equals `example.flagged_span`.
     - Runs `RULE` on `format!("{}\n{}", example.flagged, example.flagged)` and asserts 2 diagnostics on the expected spans.
     - Runs `RULE` on `example.fixed` and asserts 0 diagnostics (and `!parsed.has_syntax_error()`).
2. **`pass::<case_name>` tests**:
   - Parses `$cmd` via `ParsedCommand::new($cmd)`, asserts `!parsed.has_syntax_error()`, and asserts `RULE` produces 0 diagnostics.
   - Also runs `format!("{cmd} && {cmd}")` and asserts 0 diagnostics.
3. **`fail::<case_name>` tests**:
   - Parses `$cmd`, asserts `!parsed.has_syntax_error()`, and asserts `RULE` produces 1 diagnostic whose span slice in `$cmd` equals `$expected_span` (and whose message has no unresolved `{placeholder}`).
   - Also runs `format!("{cmd} && {cmd}")` (when `$cmd` is a single command) and asserts 2 diagnostics whose span slices both equal `$expected_span`.
4. **Enforcement in [`tests/registry.rs`](../../../tests/registry.rs)**:
   - Update `test_every_rule_file_uses_rule_test_harness` to also check every file in `src/command_lint/rules/*.rs` for `command_rule_test!(`.
   - Replace `COMMAND_RULES` in `test_rules_without_an_example_harness_document_no_examples` with `test_command_rules_document_one_example` asserting every `COMMAND_RULES` entry has `examples.len() == 1`.

---

## 5. Step-by-Step Execution Plan (Phase 4 Preview)

Once `Q2` (Prototype 2A) and `Q3` (Prototype 3A) are confirmed:

1. **Dependencies & Architecture Conformance (`Cargo.toml`, `src/architecture.rs`, `tests/architecture_conformance.rs`)**:
   - Add `brush-parser = "0.4.0"`; remove `ast-grep-core`, `ast-grep-language`, and `shell-words`.
   - Update `CommandLintCommand => [Diagnostic]` in `src/architecture.rs` and replace `AST_GREP_OWNERS` with `brush_parser` ownership on `command_lint::command` in `tests/architecture_conformance.rs`.
2. **Shell AST & CLI Schema (`src/command_lint/command.rs`, `src/command_lint/command/shell.rs`, `src/command_lint/command/cli_args.rs`)**:
   - Implement `ParsedCommand`, `InterceptedCommand`, `CommandArg`, `EnvAssignment`, `CommandRedirect`, `ShellScope`, `CharToByteMap`, and recursive `$(...)` / `` `...` `` extraction via `brush_parser`.
   - Update `ProgramCliSchema` and `ParsedArgs` in `cli_args.rs` to operate on `&[CommandArg]`.
3. **Rule Declaration, Contract & Runner (`src/rule_declaration/`, `src/rule_catalog.rs`, `src/command_lint/contract.rs`, `src/command_lint/runner.rs`)**:
   - Add `CommandExample` and `DeclaredExamples` in `rule_declaration` and render ```` ```sh ```` blocks in `rule_catalog.rs`.
   - Update `CommandRule` in `src/command_lint/contract.rs` and `run_command_lint` in `src/command_lint/runner.rs` to use `ParsedCommand`.
4. **Test Harness & Rule Migration (`src/test_utils.rs`, `src/command_lint/rules/edit_of_described_commit.rs`, `tests/registry.rs`)**:
   - Implement `command_rule_test!` and `assert_command_documented_examples` in `src/test_utils.rs`.
   - Migrate `edit_of_described_commit.rs` to `ParsedCommand`, add its `CommandExample`, and migrate its tests to `command_rule_test!` (including regression cases for redirects `jj edit > out.txt`, env prefixes, pipelines, and command substitutions).
   - Enforce `command_rule_test!` and `CommandExample` in `tests/registry.rs`.
5. **Verification (Phase 5 & 6)**:
   - Run `cargo test`, `cargo clippy --all-targets`, diff-scoped `cargo mutants --cap-lints=true --in-diff -- --lib`, and `cargo +nightly llvm-cov --lib --branch`.
