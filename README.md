# Omnicoach: Custom Domain & Code Style Linter

Lightweight, high-signal static analysis engine and command coach designed to enforce domain-specific code style rules, architectural boundaries, and test hygiene across Python and Rust codebases.

---

## 📦 Installation

### Python (`uv`)

```bash
uv tool install --managed-python omnicoach
```

Or per-project (`uv add --dev omnicoach`), run via `uv run omnicoach`.

### Standalone Binary

Install with [cargo-binstall](https://github.com/cargo-bins/cargo-binstall), or compile from [crates.io](https://crates.io/crates/omnicoach):

```bash
cargo binstall omnicoach          # prebuilt GitHub Release binary
cargo install omnicoach --locked  # compile from source
```

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

Explore module structure, call hierarchies, and symbol slices (alpha):

```bash
omnicoach graph src/config.rs               # intra-file [Lk] call clusters and line spans
omnicoach graph src/                        # component and module map (defaults to --depth module)
omnicoach graph find ArchitectureSpec       # locate declarations across the repository
omnicoach graph slice load_config --hops 2  # cross-module callers, callees, and signature contracts
```

Run as an agent lifecycle hook (Antigravity):

```bash
omnicoach hook antigravity pre-tool-use    # evaluates PreToolUse events from stdin
omnicoach hook antigravity pre-invocation  # injects queued code warnings on PreInvocation
```

Wire into `~/.gemini/config/hooks.json` (listing `~/.local/bin` before `~/.cargo/bin` prefers a `uv` tool install):

```json
{
  "omnicoach": {
    "PreToolUse": [
      {
        "matcher": "run_command|write_file|edit_file|write_to_file|replace_file_content|multi_replace_file_content",
        "hooks": [
          {
            "type": "command",
            "command": "PATH=\"$HOME/.local/bin:$HOME/.cargo/bin:$PATH\" omnicoach hook antigravity pre-tool-use"
          }
        ]
      }
    ],
    "PreInvocation": [
      {
        "type": "command",
        "command": "PATH=\"$HOME/.local/bin:$HOME/.cargo/bin:$PATH\" omnicoach hook antigravity pre-invocation"
      }
    ]
  }
}
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

Omnicoach resolves configuration across two scopes:

- **Project configuration (`.omnicoach.toml`)**: Discovered by walking up ancestor directories from the target path (or current directory), stopping at `.git` or `.jj` repository boundaries. Relative `[per-file-ignores]` globs are anchored to the directory containing `.omnicoach.toml`.
- **Global user configuration (`$XDG_CONFIG_HOME/omnicoach/config.toml` or `~/.config/omnicoach/config.toml`)**:
  - **`code` & `graph` (repo-first fallback)**: Used only when no ancestor `.omnicoach.toml` is found for a target, preserving team-shared repository configs.
  - **`command` & `--explain <command-rule>` (global-first precedence)**: User-level shell and VCS guardrails in global config take precedence over repository `.omnicoach.toml`.

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

### Project Context & Architectural Conformance

Configure test-file globs, Python import roots, and an acyclic component dependency graph enforced by the `architecture` rules across Python and Rust:

```toml
[context]
test-patterns = ["**/tests/**", "**/test_*.py", "**/*_test.py", "**/tests.rs"]
source-roots = ["src"] # optional; inferred automatically when omitted

[architecture]
transitive-dependencies = true       # default: true (multi-hop DAG reachability)
require-component-coverage = true    # default: true (every module belongs to a component or router)
ignore-type-checking-imports = false # default: false (exempts Python `if TYPE_CHECKING:` when true)

[[architecture.components]]
name = "Domain"
roots = ["my_app.domain"]
depends-on = []
owns-external = ["pydantic"]         # only `Domain` may import `pydantic`
public-exports = ["Order", "Order*"] # optional public facade allowlist

[[architecture.components]]
name = "Adapters"
roots = ["my_app.adapters"]
depends-on = ["Domain"]              # must reference components declared earlier
isolate-children = true              # forbids lateral imports between peer submodules
```

Rust crates can alternatively declare their component DAG in code with `define_architecture!` in `src/architecture.rs` and `architecture_component!(...)` on each component root module (`omnicoach --list-rules --tag architecture`).

---

## 📋 Rules

`omnicoach` documents every rule and topic, code and command alike:

```bash
omnicoach --list-rules              # every rule, with its languages and summary
omnicoach --list-rules --tag testing # the rules `select = ["testing"]` would select
omnicoach --list-tags               # the topic tree: parents, synonyms, descriptions, scope notes
omnicoach --explain sleep-in-tests  # one rule: what it does, why, configuration, tags, status
```

`--explain` also reports whether the rule is on under the active `.omnicoach.toml` (or global `config.toml`), and which selector decided it.

---

## 🔕 Suppressions

Suppression directives accept either `omni:` or `omnicoach:` and require explicit bracketed rule targets and a `-- <reason>` explanation:

### Inline Suppression (`omni:ignore` / `omnicoach:ignore`)

```python
task = asyncio.create_task(loop())  # omni:ignore [unstructured-task] -- top-level daemon lifecycle
```

```rust
let y = 1; // omni:ignore [single-letter-name] -- 2D vertical coordinate
```

### Preceding-Line Suppression (`omni:ignore` / `omnicoach:ignore`)

```python
def traced(handler):
    # omni:ignore [nested-function] -- decorator requires closure over handler
    @functools.wraps(handler)
    def wrapper(request):
        return handler(request)
    return wrapper
```

### File-Level Suppression (`omni:disable-file` / `omnicoach:disable-file`)

```python
# omni:disable-file [nested-function, single-letter-name] -- generated schema
```

---

## 🤝 Contributing

To add a rule, follow [docs/dev/adding_a_rule.md](https://github.com/Kajiih/omnicoach/blob/main/docs/dev/adding_a_rule.md).
