# Phase 2: Gather References — Global Configuration & Linter Architecture

## 1. State of the Art (SOTA) Survey

| Tool / Ecosystem | Local Hierarchy | Global Fallback / User Scope | Multi-File Merging | Domain Splitting |
| :--- | :--- | :--- | :--- | :--- |
| **Ruff** | Nearest `.ruff.toml` / `pyproject.toml` up ancestor tree | `$XDG_CONFIG_HOME/ruff/ruff.toml` (or `~/.config/ruff/ruff.toml`) | None by default; explicit `extend = "..."` opt-in | Code linting only |
| **Biome** | Nearest `biome.json` up to project root | None by default (`--config-path` / `BIOME_CONFIG_PATH` env) | Explicit `extends: [...]` | Formatter + Linter combined |
| **Cargo vs. Clippy** | Repo `clippy.toml` / `Cargo.toml` | Cargo: `~/.cargo/config.toml` (global tools/aliases/registries); Clippy: in-tree | Cargo merges config keys across hierarchy; Clippy is strictly repo-local | Cargo = workflow/build; Clippy = code analysis |
| **Jujutsu (`jj`)** | `.jj/repo/config.toml` | User-global: `~/.config/jj/config.toml` (and `JJ_CONFIG`) | Layered: builtin $\to$ user $\to$ repo $\to$ flags | Command/VCS only |
| **Git** | `.git/config` | Global: `~/.gitconfig` / `$XDG_CONFIG_HOME/git/config` (`GIT_CONFIG_GLOBAL`) | Cascading key-value merge | VCS/Workflow only |
| **Claude Code** | Project `.claude/` / `CLAUDE.md` | User-global: `~/.claude/config.json` | Project overrides user | Tooling / Agent permissions |

### Key Takeaways for `omnicoach`

1. **Separation of Code vs. Workflow Concerns**:
   Linters (Ruff, Clippy) keep code lints strictly repo-deterministic when repo config exists so CI matches local developer machines. In contrast, workflow tools (Cargo, Jujutsu, Git, Claude Code) prioritize user-global configuration across repos for personal safety guardrails, commands, and aliases.
2. **Glob Anchoring**:
   In Ruff, globs in `per-file-ignores` are always evaluated relative to the directory containing the configuration file that declared them (`package_root`). When evaluating files from subdirectories, paths are normalized relative to this root.
3. **Hermeticity**:
   Standard toolchains honor `XDG_CONFIG_HOME` to facilitate isolated testing and containerized execution without side-effects from the developer's home directory.

---

## 2. Codebase Audit (`custom_lints-hook`)

### Call Sites of Configuration Loaders

1. **`src/bin/omnicoach.rs`**:
   - Line 35: Top-level default `code` execution calls `load_config(&cli.config_overrides)`.
   - Line 55: `DomainCommand::Code` calls `load_config(&cli.config_overrides)`.
   - Line 65: `DomainCommand::Command` calls `load_config(&cli.config_overrides)`.
   - Line 73: `DomainCommand::Graph` calls `load_config(&cli.config_overrides)`.
   Previously, all CLI commands called `load_config` which only read `./.omnicoach.toml` in `current_dir()`.

2. **`src/hook/runner.rs`**:
   - `check_command`: calls `resolve_project_root(&working_directory, workspace_paths)` and loads config via `load_config_from_root(&project_root, ...)`.
   - `lint_proposed_edit`: calls `resolve_project_root(parent_directory, workspace_paths)` and loads config via `load_config_from_root(&project_root, ...)`.

3. **`src/rule_selection.rs`**:
   - `discover_project_root(start_directory)` traverses `start_directory.ancestors()` searching for `.omnicoach.toml`, `.jj`, or `.git`.
   - `load_rule_status(rule, overrides)` is called by `src/rule_catalog.rs` for `--explain <rule>`. Because `rule` carries `rule.matches(Selector::Tag(Tag::Derived(Derived::Command)))`, it already knows whether the rule is a command rule or code rule.

4. **`src/config.rs` (`normalize_path_for_glob`)**:
   - Stripped `std::env::current_dir()` only if `path.is_absolute()`.
   - When running from a subdirectory (e.g. `cd tests && omnicoach code cli.rs`), `path` was `"cli.rs"`, which failed to match `"tests/**"` in `per-file-ignores` or `"**/tests/**"` in `test_patterns`.
   - Storing `root_dir: Option<PathBuf>` on `Config` allows resolving relative paths against `std::env::current_dir()` and stripping `root_dir` to produce normalized repository-relative paths (`"tests/cli.rs"`).

5. **Architectural Self-Lint Constraint**:
   - Built-in rule `environment-variable-in-function` enforces that `std::env::var` / `std::env::var_os` may only be called in top-level `main` or functions named `from_env`, `from_environ`, or `load_env`.
   - Global config resolution must implement an explicit configuration boundary (`load_env()`).
