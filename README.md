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
- **`enforcement_mode = "ban"`** (default for most rules): Prohibits the pattern. Violations can only be bypassed using explicit `# omni:ignore[rule] -- <reason>` directives.
- **`enforcement_mode = "require-explanation"`**: Permits the pattern as long as it is accompanied by an adjacent or inline substantive explanatory comment.

You can configure enforcement mode globally or per-language for any code rule (suppression audits and command rules reject it):

```toml
[rules.no-typing-cast]
enforcement_mode = "ban" # default: strictly banned

[rules.no-sleep-in-tests]
enforcement_mode = "require-explanation" # permitted only when documented with an explanation comment

# Language-specific mode overrides
[rules.single-letter-variable-name.python]
enforcement_mode = "require-explanation"
```

### Global Selection & File Scoping

```toml
# Select tags (topics, facet values such as `heuristic`, or languages) and rule names.
# A topic includes its subtopics: `testing` covers `test-timing`, `test-doubles`, ...
select = ["testing", "no-typing-cast"]

# The nearest selector wins along a topic path, so `testing` rules stay on except the
# `test-doubles` ones. A rule name beats any tag.
ignore = ["test-doubles"]

# Per-file rule ignores using glob patterns, applied after `select` / `ignore`
[per_file_ignores]
"tests/**" = ["single-letter-variable-name", "heuristic"]
```

Unknown labels, facet names (`precision`) and a selector in both `select` and `ignore` fail at config load. Tags, their meaning and the full topic tree are in [docs/dev/tag_guide.md](docs/dev/tag_guide.md).

---

## 📋 Rules

Both binaries document every rule, code and command alike:

```bash
omni-code-lint --list-rules              # every rule, with its languages and summary
omni-code-lint --list-rules --tag testing # the rules `select = ["testing"]` would select
omni-code-lint --explain no-sleep-in-tests # one rule: what it does, why, configuration, tags, status
```

`--explain` also reports whether the rule is on under the `.omnilint.toml` of the current directory, and which selector decided it.

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
