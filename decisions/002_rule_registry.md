# Rule Registry Definition

Based on our design decisions, we use target-based prefixes for our rule codes. This makes it immediately clear what domain a rule applies to. We also categorize rules with metadata **Tags** for advanced filtering (`select`/`ignore` by tag).

## Code Categories

*   `PY`: Python static analysis rules (e.g., `PY001`)
*   `MD`: Markdown static analysis rules (e.g., `MD001`)
*   `CMD`: Command guard rules for CLI execution (e.g., `CMD001`)
*   `VCS`: Workflow and version control guard rules (e.g., `VCS001`)

## Tag Categories

*   `Logging`: Checks related to logging configurations and invocations
*   `Exceptions`: Checks targeting exception handling structures
*   `Python`: Checks targeting Python source code ASTs
*   `Style`: Code style and formatting conventions
*   `Safety`: Safety guidelines and command restrictions
*   `Cli`: Command-line syntax checks
*   `Workflow`: Workflow execution rules
*   `Vcs`: Version control systems integrations
etc

## Initial Rules & Tag Mappings

### Python (`PY`)
*   **PY001 (`no-logging-in-except`)**: Banned use of `logging.error` inside except blocks without `exc_info=True`.
    *   *Tags*: `Logging`, `Exceptions`, `Python`
*   **PY002 (`flat-scope-enforced`)**: Enforces flat scoping (avoiding deeply nested functions or classes where unnecessary).
    *   *Tags*: `Style`, `Python`

### Command Guards (`CMD`)
*   **CMD001 (`banned-force-flag`)**: Warns/errors on the use of dangerous `--force` or `-f` flags on certain commands without explicit overrides (style/practice check).
    *   *Tags*: `Safety`, `Cli`

> [!NOTE]
> **Safety Scope:** Command guards check bash syntax patterns statically. They do not act as a secure sandbox against malicious actions (which must be enforced at the runtime/execution layer by the agent tools).

### Workflow Guards (`VCS`)
*   **VCS001 (`no-edits-on-described-commits`)**: Prevents modifying JJ commits that already have a detailed description, enforcing a new commit for further changes.
    *   *Tags*: `Workflow`, `Vcs`

---

## 🗺️ Roadmap & Future Work

Performance optimizations, subprocess caching, and compile-time test verification are tracked in the centralized [ROADMAP.md](../ROADMAP.md).
