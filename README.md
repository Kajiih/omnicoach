# Omnicoach: Custom Domain & Code Style Linter

Lightweight, high-signal static analysis engine and command coach designed to enforce domain-specific code style rules, architectural boundaries, and test hygiene across Python and Rust codebases.

---

## 📦 Installation

Prebuilt binaries are published to PyPI:

```bash
uv tool install omnicoach   # or: pipx install omnicoach
```

From crates.io:

```bash
cargo install omnicoach --locked
```

Both install the `omnicoach` binary.

---

## 🚀 Quickstart

Scan the entire repository:

```bash
omnicoach code .   # or simply: omnicoach
```

Scan only modified lines/files (VCS aware):

```bash
omnicoach code --diff
```

Validate an intercepted workflow command:

```bash
omnicoach command --cmd "jj edit @"
```

Try `omnicoach` in the browser or call its WebAssembly request/response module directly at **[kajiih.github.io/omnicoach](https://kajiih.github.io/omnicoach/)**:

```javascript
import { loadOmnicoach } from "https://kajiih.github.io/omnicoach/omnicoach.js";

const omni = await loadOmnicoach("https://kajiih.github.io/omnicoach/omnicoach_playground.wasm");
const result = omni.lint({ path: "tests/test_app.py", code: "import time\ndef test_a(): time.sleep(1)\n", config: "" });
```

To pin a release, load `wasm/<tag>/omnicoach_playground.wasm` instead.

---

## ⚙️ Configuration (`.omnicoach.toml`)

Create a `.omnicoach.toml` file at the root of your project workspace.

### Framework Rule Design: Enforcement Modes

Every rule in Omnicoach is built on a unified enforcement framework:

- **`enforcement-mode = "ban"`** (default for most rules): Prohibits the pattern. Violations can only be bypassed using explicit `# omni:ignore[rule] -- <reason>` (or `# omnicoach:ignore[rule] -- <reason>`) directives.
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

Unknown labels, facet names (`precision`) and a selector in both `select` and `ignore` fail at config load. Run `omnicoach --list-tags` for the topic tree; facet definitions and selection rules are in [docs/dev/tag_guide.md](https://github.com/Kajiih/omnicoach/blob/main/docs/dev/tag_guide.md).

---

## 📋 Rules

`omnicoach` documents every rule and topic, code and command alike:

```bash
omnicoach --list-rules              # every rule, with its languages and summary
omnicoach --list-rules --tag testing # the rules `select = ["testing"]` would select
omnicoach --list-tags               # the topic tree: parents, synonyms, descriptions, scope notes
omnicoach --explain sleep-in-tests  # one rule: what it does, why, configuration, tags, status
```

`--explain` also reports whether the rule is on under the `.omnicoach.toml` of the current directory, and which selector decided it.

---

## 🔕 Suppressions

Suppression directives accept either `omni:` or `omnicoach:` and require explicit bracketed rule targets and a `-- <reason>` explanation:

### Inline Suppression (`omni:ignore` / `omnicoach:ignore`)

```python
task = asyncio.create_task(loop())  # omni:ignore [unstructured-task] -- top-level daemon lifecycle
```

```rust
let x = 1; // omni:ignore [single-letter-name] -- 2D vector coordinate
```

### Preceding-Line Suppression (`omni:ignore` / `omnicoach:ignore`)

```python
# omni:ignore [nested-function] -- factory requires localized closure
@dataclass
def make_handler():
    def helper(): pass
    return helper
```

### File-Level Suppression (`omni:disable-file` / `omnicoach:disable-file`)

```python
# omni:disable-file [nested-function, single-letter-name] -- generated schema
```

---

## 🤝 Contributing

To add a rule, follow [docs/dev/adding_a_rule.md](https://github.com/Kajiih/omnicoach/blob/main/docs/dev/adding_a_rule.md).
