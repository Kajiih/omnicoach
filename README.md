# Omni Lints: Custom Domain & VCS Linters

Omni Lints is a lightweight, customizable linter suite designed to enforce domain-specific code style rules and Jujutsu (jj) VCS usage policies.

It provides two binaries:
1.  **`omni-code-lint`**: Scans codebase ASTs for styling and domain safety rules.
2.  **`omni-command-lint`**: Intercepts shell commands to block bad VCS actions (e.g. editing already-described jj commits).

---

## 🛠️ Installation

### Option A: Install from Source (Recommended for Developers)
Any developer with a Rust toolchain installed can clone this repository and install the binaries directly:

```bash
# Clone the repository
git clone <repository-url>
cd custom_lints

# Install the binaries to ~/.cargo/bin/
cargo install --path .
```

*Note: Ensure `~/.cargo/bin` is in your shell's `PATH`.*

### Option B: Distribute Pre-Compiled Binaries
You can compile optimized release binaries to share directly with developers on the same OS:

```bash
cargo build --release
```
The compiled binaries will be located at:
- `target/release/omni-code-lint`
- `target/release/omni-command-lint`

You can package and upload these to your team's shared file store, or attach them as artifacts to your repository releases.

---

## 🚀 Usage

### Code Linter
To scan your codebase for AST styling violations:
```bash
omni-code-lint .
```

To scan specific files or directories:
```bash
omni-code-lint src/main.rs tests/
```

#### Differential Linting (VCS aware)
You can run checks only on modified files and only report violations on newly changed lines using the `--diff` flag:
```bash
omni-code-lint --diff
```
*   **Git**: Diffs working copy (staged & unstaged) changes against `HEAD` by default.
*   **Jujutsu (jj)**: Diffs all mutable draft commits since the last immutable commit (`immutable()..`) by default.

You can also specify a custom revision or range to compare against using the `--diff-rev` flag (which implicitly enables `--diff`):
```bash
# Git: Compare against main branch
omni-code-lint --diff-rev main

# Jujutsu: Compare against the parent commit
omni-code-lint --diff-rev @-
```

### VCS Command Intercepter
To lint specific Jujutsu command invocations:
```bash
omni-command-lint --cmd "jj edit 123"
```

---

## ⚙️ Configuration (`.omnilint.toml`)

Create a `.omnilint.toml` file at the root of your project workspace to customize rule parameters, ignore rules, or run subset filters:

```toml
# Select only specific tags or rule names (optional)
select = ["Style", "no-logging-in-except"]

# Globally ignore specific rules (optional)
ignore = ["single-letter-variable-name"]

# Per-file rule ignores using glob patterns
[per_file_ignores]
"tests/**" = ["single-letter-variable-name", "heuristic"]

# Rule-specific configuration parameters
[rules.no-hungarian-notation]
banned_suffixes = ["_list", "_arr", "_dict"]
```

### Supported Rules:
*   **`no-unstructured-task-creation`**: Bans unstructured task creation (`asyncio.create_task`, `ensure_future`, `loop.create_task`).
*   **`no-sleep-in-tests`**: Bans wall-clock and async `sleep` calls in Python and Rust test files (with a distinct checkpoint/yield suggestion for zero-duration sleeps).
*   **`max-test-assertions`**: Limits test functions to at most 4 assertions by default (configurable via `[rules.max-test-assertions] max = N`) across Python and Rust.
*   **`no-assertion-packing`**: Bans compound boolean conditions (`&&`, `and`) and boolean tuple/collection equality packing in Python and Rust test assertions.
*   **`single-letter-variable-name`**: Bans short single-letter variables except allowed exceptions.
*   **`banned-abbreviations`**: Bans naming definitions using cryptic abbreviations (`ctx`, `cfg`, etc.).
*   **`no-hungarian-notation`**: Bans Hungarian type suffixes (e.g., `user_list`, `value_int`).
*   **`missing-suppression-reason`**: Enforces non-empty `-- <reason>` explanations on inline and file suppressions.
*   **`unused-suppression`**: Flags stale suppression directives when no violation occurred on that line or file.
*   **`unknown-suppression-rule`**: Flags suppression directives targeting unknown or invalid rules.
*   **`blanket-suppression`**: Bans blanket suppression directives without explicit bracketed rule names.
*   **`no-logging-in-except`**: Bans using `logging.error` inside Python except blocks (suggests `logging.exception`).
*   **`flat-scope-enforced`**: Bans nested function definitions in Python source files.
*   **`no-edits-on-described-commits`**: Discourages/blocks running `jj edit` on commits that already have descriptions.

---

## 🔕 Suppressions & Hygiene

Omni provides granular, review-accountable suppression comment directives directly in source code.

### Syntax

The token shown in brackets in any diagnostic `[rule-name]` is the exact token accepted in suppression directives. Every directive requires bracketed rule names and an explicit reason after `--`:

#### 1. Same-Line Suppression (`omni:ignore`)
Suppresses rule violations occurring on the exact same line:
```rust
let x = 1; // omni:ignore [single-letter-variable-name] -- mathematical coordinate in 2D vector
```
```python
task = asyncio.create_task(loop())  # omni:ignore [no-unstructured-task-creation] -- top-level background daemon
```

#### 2. Preceding-Line Suppression (`omni:ignore`)
A standalone directive comment on the line immediately preceding a declaration applies to that declaration, automatically skipping any contiguous decorators or attributes:
```python
# omni:ignore [flat-scope-enforced] -- factory method requires localized closure
@dataclass
def make_handler():
    def helper(): pass
    return helper
```
```rust
// omni:ignore [banned-abbreviations] -- external C FFI struct definition
#[repr(C)]
struct ctx_t;
```

#### 3. File-Level Suppression (`omni:disable-file`)
Placed anywhere in the file (conventionally at the top) to suppress specific rules for the entire file:
```python
# omni:disable-file [flat-scope-enforced, single-letter-variable-name] -- generated protobuf schema
```

## 🤝 Sharing with the Team

### 1. Git Pre-Commit Hook Integration
To automate code checking before git commits, add the following script to your local repository's `.git/hooks/pre-commit` file:

```bash
#!/bin/bash
echo "Running Omni Linter..."
omni-code-lint .
if [ $? -ne 0 ]; then
    echo "Linter checks failed. Commit aborted."
    exit 1
fi
```
Make the hook executable:
```bash
chmod +x .git/hooks/pre-commit
```

### 2. CI/CD Pipeline Integration
Add the linter checks as a step in your CI pipeline (e.g. GitHub Actions):

```yaml
name: Lint Check
on: [push, pull_request]
jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable
      - name: Install Linter
        run: cargo install --git https://github.com/your-org/custom_lints.git
      - name: Run Linter
        run: omni-code-lint .
```
