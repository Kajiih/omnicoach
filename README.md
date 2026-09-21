# Omni Lints: Custom Domain & Code Style Linters

Lightweight, high-signal static analysis engine designed to enforce domain-specific code style rules, architectural boundaries, and test hygiene across Python and Rust codebases.

---

## 🚀 Quickstart

Scan the entire repository:
```bash
omni-code-lint .
```

Scan only modified lines/files (VCS aware):
```bash
omni-code-lint --diff
```

Validate an intercepted workflow command:
```bash
omni-command-lint --cmd "jj edit @"
```

---

## ⚙️ Configuration (`.omnilint.toml`)

Create a `.omnilint.toml` file at the root of your project workspace.

### Framework Rule Design: Enforcement Modes

Every rule in Omni is built on a unified enforcement framework:
- **`mode = "ban"`** (default for most rules): Prohibits the pattern. Violations can only be bypassed using explicit `# omni:ignore[rule] -- <reason>` directives.
- **`mode = "require-explanation"`**: Permits the pattern as long as it is accompanied by an adjacent or inline substantive explanatory comment.

You can configure enforcement mode globally or per-language for any rule:

```toml
[rules.no-typing-cast]
mode = "ban" # default: strictly banned

[rules.no-sleep-in-tests]
mode = "require-explanation" # permitted only when documented with an explanation comment

# Language-specific mode overrides
[rules.single-letter-variable-name.python]
mode = "require-explanation"
```

### Global Selection & File Scoping

```toml
# Select only specific tags or rule names
select = ["Testing", "no-typing-cast"]

# Globally ignore specific rules
ignore = ["single-letter-variable-name"]

# Per-file rule ignores using glob patterns
[per_file_ignores]
"tests/**" = ["single-letter-variable-name", "flat-scope-enforced"]
```

---

## 📋 Rules Catalog

All rules support the `mode = "ban" | "require-explanation"` configuration. Below are the rules and their domain-specific settings:

### Concurrency & Async
* **`no-unstructured-task-creation`**: Bans fire-and-forget background task creation (`asyncio.create_task`, `ensure_future`, `loop.create_task`) in favor of structured concurrency (`asyncio.TaskGroup`). *(Python)*

### Testing Hygiene
* **`no-sleep-in-tests`**: Bans arbitrary wall-clock and async sleep calls (`time.sleep`, `thread::sleep`, `tokio::time::sleep`) in test files. *(Python, Rust)*
* **`no-zero-sleep-in-tests`**: Bans zero-duration scheduler yield hacks (`sleep(0)`, `sleep(Duration::ZERO)`) in test files. *(Python, Rust)*
* **`max-test-assertions`**: Enforces a maximum assertion count per test function to prevent monolithic multi-concept tests. *(Python, Rust)*
  ```toml
  [rules.max-test-assertions]
  max = 4 # default: 4
  ```
* **`no-assertion-packing`**: Bans compound boolean assertions (`and`, `&&`) and boolean collection equality packing designed to circumvent assertion limits. *(Python, Rust)*
* **`no-mocks-in-tests`**: Bans dynamic mocks and monkeypatching (`MagicMock`, `patch`, `mocker.*`, `monkeypatch.*`, `setattr`) in tests in favor of state-based in-memory Fakes. *(Python)*
  ```toml
  [rules.no-mocks-in-tests]
  allowed = ["create_autospec"]
  ```
* **`no-mock-assertions`**: Bans interaction-based mock assertion methods (`assert_called_once`, `assert_called_with`, `assert_awaited`, etc.) in tests in favor of asserting on returned values or observable state changes. *(Python)*

### Naming & Vocabulary
* **`single-letter-variable-name`**: Bans uncommunicative single-letter variable names outside of standard idioms (`i`, `j`, `k`, `x`, `y`, `z`, `_`). *(Python, Rust)*
  ```toml
  [rules.single-letter-variable-name]
  extend_allowed = ["w", "h"]
  ```
* **`banned-abbreviations`**: Bans ambiguous abbreviations (`ctx`, `req`, `resp`, `mgr`, `cb`) in favor of full domain words. *(Python, Rust)*
  ```toml
  [rules.banned-abbreviations]
  extend_banned = ["cfg", "idx"]
  ```
* **`no-hungarian-notation`**: Bans Hungarian type suffixes (`_list`, `_dict`, `_arr`) from identifier names. *(Python, Rust)*
  ```toml
  [rules.no-hungarian-notation]
  allowed = ["_str"]
  ```
* **`prefer-timedelta-over-seconds`**: Enforces strongly typed durations (`datetime.timedelta` / `std::time::Duration`) over numeric variables with raw time-unit suffixes (`_seconds`, `_secs`, `_ms`, `_millis`). *(Python, Rust)*
  ```toml
  [rules.prefer-timedelta-over-seconds]
  allowed = ["_sec"]
  ```
* **`prefer-dedent-for-multiline-strings`**: Enforces wrapping multiline string literals in a dedent helper (`textwrap.dedent` / `inspect.cleandoc` in Python; `indoc!` / `formatdoc!` / `concat!` in Rust) across all scopes while exempting docstrings and `insta` `@"..."` inline snapshots. *(Python, Rust)*
  ```toml
  [rules.prefer-dedent-for-multiline-strings]
  extend_allowed = ["custom_dedent"]
  ```

### Typing & Signatures
* **`no-typing-cast`**: Bans unchecked type assertions (`cast()`, `typing.cast()`, `typing_extensions.cast()`) in production code. *(Python)*
* **`no-dynamic-attribute-access`**: Bans runtime attribute reflection (`getattr`, `hasattr`, `setattr`, `delattr`, `builtins.*`) that erases types to `Any` and obscures symbol references. *(Python)*
* **`enforce-frozen-slots-dataclass`**: Enforces that Python `@dataclass` classes specify `frozen=True` and `slots=True` to guarantee immutability and memory efficiency, unless explicitly opted out with `frozen=False` / `slots=False`. *(Python)*
* **`no-identical-positional-types`**: Bans functions with $\ge 3$ positional parameters where 2 or more share an identical type annotation (suggests keyword-only arguments or domain newtypes). *(Python, Rust)*
  ```toml
  [rules.no-identical-positional-types]
  min_args = 3 # default: 3
  ```

### Architecture & Control Flow
* **`no-env-in-functions`**: Bans reading/writing environment variables (`os.getenv`, `std::env::var`) inside functions and methods outside of configuration entrypoints. *(Python, Rust)*
* **`flat-scope-enforced`**: Bans nested function and closure definitions in source files to prevent hidden state and encourage modular helpers. *(Python)*
* **`no-logging-error-in-except`**: Bans using `logging.error` inside Python `except` blocks (suggests `logging.exception` to preserve stack traces). *(Python)*
* **`no-uncommented-suppress`**: Enforces that `contextlib.suppress(...)` statements document why swallowing the exception is benign (defaults to `mode = "require-explanation"`). *(Python)*

### VCS & Workflow Commands
* **`no-edits-on-described-commits`**: Prohibits running `jj edit` on commits that already have descriptions to preserve review stability. *(JJ)*

### Suppression Hygiene
* **`missing-suppression-reason`**: Enforces non-empty `-- <reason>` justifications on all suppression comments.
* **`unused-suppression`**: Flags stale suppression directives when no violation occurs on the target line or file.
* **`unknown-suppression-rule`**: Flags directives referencing nonexistent or mistyped rule names.
* **`blanket-suppression`**: Bans bare suppression directives that omit bracketed rule names.

---

## 🔕 Suppressions

Suppression directives require explicit bracketed rule targets and a `-- <reason>` explanation:

### Inline Suppression (`omni:ignore`)
```python
task = asyncio.create_task(loop())  # omni:ignore [no-unstructured-task-creation] -- top-level daemon lifecycle
```
```rust
let x = 1; // omni:ignore [single-letter-variable-name] -- 2D vector coordinate
```

### Preceding-Line Suppression (`omni:ignore`)
```python
# omni:ignore [flat-scope-enforced] -- factory requires localized closure
@dataclass
def make_handler():
    def helper(): pass
    return helper
```

### File-Level Suppression (`omni:disable-file`)
```python
# omni:disable-file [flat-scope-enforced, single-letter-variable-name] -- generated schema
```
