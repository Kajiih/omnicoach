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
# Select only specific tags or rule codes (optional)
select = ["Style", "LOG-001"]

# Globally ignore specific rules (optional)
ignore = ["NAME-001"]

# Per-file rule ignores using glob patterns
[per_file_ignores]
"tests/**" = ["NAME-001", "heuristic"]

# Rule-specific configuration parameters
[rules.no-hungarian-notation]
banned_suffixes = ["_list", "_arr", "_dict"]
```

### Supported Rules:
*   **`ASYNC-001` (`no-unstructured-task-creation`)**: Bans unstructured task creation (`asyncio.create_task`, `ensure_future`, `loop.create_task`).
*   **`NAME-001` (`single-letter-variable-name`)**: Bans short single-letter variables except allowed exceptions.
*   **`NAME-002` (`banned-abbreviations`)**: Bans naming definitions using cryptic abbreviations (`ctx`, `cfg`, etc.).
*   **`NAME-003` (`no-hungarian-notation`)**: Bans Hungarian type suffixes (e.g., `user_list`, `value_int`).
*   **`SUPP-001` (`missing-suppression-reason`)**: Enforces non-empty `-- <reason>` explanations on inline and file suppressions.
*   **`SUPP-002` (`unused-suppression`)**: Flags stale suppression directives when no violation occurred on that line or file.
*   **`SUPP-003` (`unknown-suppression-code`)**: Flags suppression directives targeting unknown or invalid rule codes.
*   **`SUPP-004` (`blanket-suppression`)**: Bans blanket suppression directives without explicit bracketed rule codes.
*   **`LOG-001` (`no-logging-in-except`)**: Bans using `logging.error` inside Python except blocks (suggests `logging.exception`).
*   **`SCOPE-001` (`flat-scope-enforced`)**: Bans nested function definitions in Python source files.
*   **`JJ-001` (`no-edits-on-described-commits`)**: Discourages/blocks running `jj edit` on commits that already have descriptions.

---

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
