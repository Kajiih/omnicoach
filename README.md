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
- **`enforcement-mode = "ban"`** (default for most rules): Prohibits the pattern. Violations can only be bypassed using explicit `# omni:ignore[rule] -- <reason>` directives.
- **`enforcement-mode = "require-explanation"`**: Permits the pattern as long as it is accompanied by an adjacent or inline substantive explanatory comment.

You can configure enforcement mode globally or per-language for any code rule (suppression audits and command rules reject it):

```toml
[rules.type-cast]
enforcement-mode = "ban" # default: strictly banned

[rules.sleep-in-tests]
enforcement-mode = "require-explanation" # permitted only when documented with an explanation comment

# Language-specific mode overrides
[rules.single-letter-name.python]
enforcement-mode = "require-explanation"
```

### Global Selection & File Scoping

```toml
# Select tags (topics, facet values such as `heuristic`, or languages) and rule names.
# A topic includes its subtopics: `testing` covers `test-timing`, `test-doubles`, ...
select = ["testing", "type-cast"]

# The nearest selector wins along a topic path, so `testing` rules stay on except the
# `test-doubles` ones. A rule name beats any tag.
ignore = ["test-doubles"]

# Per-file rule ignores using glob patterns, applied after `select` / `ignore`
[per-file-ignores]
"tests/**" = ["single-letter-name", "heuristic"]
```

Unknown labels, facet names (`precision`) and a selector in both `select` and `ignore` fail at config load. Run `omni-code-lint --list-tags` for the topic tree; facet definitions and selection rules are in [docs/dev/tag_guide.md](docs/dev/tag_guide.md).

---

## 📋 Rules

Both binaries document every rule and topic, code and command alike:

```bash
omni-code-lint --list-rules              # every rule, with its languages and summary
omni-code-lint --list-rules --tag testing # the rules `select = ["testing"]` would select
omni-code-lint --list-tags               # the topic tree: parents, synonyms, descriptions, scope notes
omni-code-lint --explain sleep-in-tests  # one rule: what it does, why, configuration, tags, status
```

`--explain` also reports whether the rule is on under the `.omnilint.toml` of the current directory, and which selector decided it.

---

## 🔕 Suppressions

Suppression directives require explicit bracketed rule targets and a `-- <reason>` explanation:

### Inline Suppression (`omni:ignore`)
```python
task = asyncio.create_task(loop())  # omni:ignore [unstructured-task] -- top-level daemon lifecycle
```
```rust
let x = 1; // omni:ignore [single-letter-name] -- 2D vector coordinate
```

### Preceding-Line Suppression (`omni:ignore`)
```python
# omni:ignore [nested-function] -- factory requires localized closure
@dataclass
def make_handler():
    def helper(): pass
    return helper
```

### File-Level Suppression (`omni:disable-file`)
```python
# omni:disable-file [nested-function, single-letter-name] -- generated schema
```

---

## 🤝 Contributing

To add a rule, follow [docs/dev/adding_a_rule.md](docs/dev/adding_a_rule.md).
