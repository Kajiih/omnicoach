# Phase 3 — Design: Agent Lifecycle Hook Implementation & Verification Plan

## 1. Critical User Journeys (CUJs) & Acceptance Criteria

### CUJ 1: Non-Local Spillover on Unmodified Line

- **Context**: An agent edits an existing file in an active workspace. The edit introduces a second occurrence of a string literal, which triggers `[repeated-literal]` anchored on an *unmodified* line elsewhere in the file.
- **Expected Behavior**:
  1. `PreToolUse` returns blank stdout (`""` / `NoObjection`), allowing the edit.
  2. The hook's differential filter recognizes that the violation on the unmodified line was not present in the pre-edit file content.
  3. The warning is queued in `WarningQueue`.
  4. On the subsequent `PreInvocation`, the warning is injected into `ephemeralMessage` with the `RuleCatalog` guidance footer.
  5. Pre-existing legacy violations on unmodified lines remain completely suppressed.

### CUJ 2: Heuristic Rule Trigger on Mandated API Signature

- **Context**: The user requests a function signature using `delay_ms: int`. The agent implements it, triggering the heuristic rule `[primitive-duration]`.
- **Expected Behavior**:
  1. `PreToolUse` allows the edit without blocking.
  2. `PreInvocation` delivers the warning containing `summary`, `rationale`, `suggestion`, `explanation_hint`, and the guidance footer:
     `Note: Some rules use heuristics. When a finding conflicts with the clearest, most robust design, suppress it with omni:ignore [<rule>] -- <reason> rather than working around the linter and degrading the design.`
  3. The agent applies `# omni:ignore [primitive-duration] -- ...` without breaking the API signature or calling `search_web`.

### CUJ 3: Out-of-Workspace Scratchpad & `/tmp` Writes

- **Context**: The agent writes a test script to `/tmp/reproduce_issue.py` or updates internal session notes in `artifactDirectoryPath`.
- **Expected Behavior**:
  1. The hook inspects the path against `event.workspace_paths` and `event.agent_owned_dir`.
  2. The target path lies outside the workspace (or inside the agent directory); `lint_proposed_edit` immediately returns `Vec::new()`.
  3. Zero warnings are generated or queued.

### CUJ 4: Workspace Without `.omnicoach.toml` (e.g. Shared Team Monorepos)

- **Context**: The agent edits a Python or Rust file in a workspace without `.omnicoach.toml` (e.g. a shared team codebase `/workspace/monorepo/...` where checking in `.omnicoach.toml` is not permitted).
- **Expected Behavior**:
  1. `resolve_project_root` detects no `.omnicoach.toml` in the project or ancestor tree.
  2. The hook runs with `Config::default()` and uses net-new differential scoping (CUJ 1) to evaluate the edit.
  3. Pre-existing legacy debt on untouched lines in the unconfigured file is 100% suppressed; only violations on modified lines or net-new spillovers are warned.
  4. If the user places an `.omnicoach.toml` in an ancestor subdirectory (e.g. `monorepo/experimental/users/<user>/.omnicoach.toml`), `resolve_project_root` discovers it and applies those project overrides.
  5. Shell commands (`command_lint`) continue enforcing VCS guardrails in all `.jj` repos.

### CUJ 5: Long-Running Sessions Without `PreInvocation`

- **Context**: The agent operates in an environment where `PreInvocation` is omitted from `hooks.json` or fails to trigger.
- **Expected Behavior**:
  1. `WarningQueue` caps stored entries at `MAX_QUEUED_WARNINGS = 64`.
  2. Further pushes overwrite or drop excess entries, preventing monotonic file growth.

---

## 2. Component Architecture & ADR Conformance

### Architectural DAG Placement ([ADR 006](../decisions/006_architectural_dag_and_conformance.md), [ADR 012](../decisions/012_module_placement_by_abstraction.md))

In `src/architecture.rs`, `Hook` sits at the top application-edge layer alongside `Cli`:

```rust
define_architecture! {
    Cli => [
        CodeLintRunner,
        CommandLintRunner,
        CommandLintVcs,
        RuleSelection,
        RuleCatalog,
        Diagnostic,
    ],
    Hook => [
        CodeLintRunner,
        CommandLintRunner,
        CommandLintVcs,
        RuleSelection,
        RuleCatalog,
        Diagnostic,
    ],
    CodeLintRunner => [CodeLintAst, RuleSelection, Config, Diagnostic],
    ...
}
```

#### Shared Components Reused Without Duplication

1. **`RuleCatalog` (`src/rule_catalog.rs`)**:
   - Single source of truth for user-facing coaching text (`FIX_NOTE`, `SUPPRESSION_NOTE`, `explain_footer`).
   - Expose `pub fn code_report_footer_for_rules<'a>(binary: &str, mut rule_names: impl Iterator<Item = &'a str>) -> String` so both CLI and `PreInvocation` ephemeral messages format footers consistently, including `SUPPRESSION_NOTE` conditionally when any reported rule is `heuristic` or `opinionated`.

---

## 3. Detailed Component Design

### 3.1 Lazy Net-New Differential Filtering (`src/hook/runner.rs`)

To achieve zero overhead on the dominant fast path while guaranteeing complete accuracy for non-local spillovers:

```rust
fn filter_relevant_diagnostics(
    relative_path: &Path,
    original_content: Option<&str>,
    virtual_file: &VirtualFile,
    config: &Config,
    post_edit_diagnostics: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    // Fast path: if every diagnostic falls on a modified line, no baseline linting needed.
    let has_unmodified_line_diagnostic = post_edit_diagnostics
        .iter()
        .any(|d| !virtual_file.modified_lines.contains(&d.location.line));

    if !has_unmodified_line_diagnostic {
        return post_edit_diagnostics;
    }

    // Greenfield file (no original content): all diagnostics are net-new.
    let Some(before_source) = original_content else {
        return post_edit_diagnostics;
    };

    // Lazily lint original content to compute the baseline multiset.
    let before_diagnostics = lint_file(relative_path, before_source, config);
    let mut baseline_counts: std::collections::HashMap<(crate::diagnostic::RuleName, String), usize> =
        std::collections::HashMap::new();
    for d in before_diagnostics {
        *baseline_counts.entry((d.rule_name, d.message.summary)).or_default() += 1;
    }

    // Keep diagnostics on modified lines unconditionally; match unmodified lines against baseline.
    post_edit_diagnostics
        .into_iter()
        .filter(|diagnostic| {
            if virtual_file
                .modified_lines
                .contains(&diagnostic.location.line)
            {
                return true;
            }
            let key = (diagnostic.rule_name, diagnostic.message.summary.clone());
            if let Some(count) = baseline_counts.get_mut(&key)
                && *count > 0
            {
                *count -= 1; // Pre-existing legacy debt on unmodified line; suppress
                false
            } else {
                true // Net-new spillover on unmodified line; report!
            }
        })
        .collect()
}
```

#### Why Multiset Matching by `(rule_name, summary)`?

- Matching by line number would fail if edits insert or delete lines above an unmodified line (line shifts would turn pre-existing debt into false positives).
- Matching by multiset count of `(rule_name, summary)` is line-shift invariant: if a file had 1 instance of `[single-letter-name]` on line 10 before an edit, and after inserting 5 lines it has 1 instance on line 15, the multiset count is $1 \to 1$ ($\Delta = 0$), correctly suppressing it.
- Decrementing `baseline_counts` strictly for unmodified-line candidates ensures that a newly added violation of the same rule on a modified line never consumes the baseline slot belonging to an unmodified line.

### 3.2 Workspace Containment (`src/hook/runner.rs`)

In `lint_proposed_edit`:

```rust
// 1. Workspace containment check:
if !event.workspace_paths.is_empty() {
    let in_workspace = event
        .workspace_paths
        .iter()
        .map(|w| normalize_path(w))
        .any(|workspace| target_file_path.starts_with(&workspace));
    if !in_workspace {
        return Vec::new();
    }
}
```

### 3.3 Self-Contained Warning Formatting & Catalog Guidance Footer

In `src/hook/runner.rs`:

```rust
fn format_code_warning(target_path: &Path, diagnostic: &Diagnostic) -> String {
    let mut text = format!(
        "{}:{}:{}: [{}] {}\n  Rationale: {}\n  Suggestion: {}",
        target_path.display(),
        diagnostic.location.line,
        diagnostic.location.column,
        diagnostic.rule_name,
        diagnostic.message.summary,
        diagnostic.message.rationale,
        diagnostic.message.suggestion
    );
    if let Some(hint) = &diagnostic.message.explanation_hint {
        text.push_str(&format!("\n  Or: {hint}"));
    }
    text
}
```

In `src/hook/antigravity.rs`:

```rust
fn serialize_pre_invocation(warnings: &[String]) -> Result<String, AntigravityProtocolError> {
    if warnings.is_empty() {
        return Ok(String::new());
    }
    let footer = code_report_footer_for_rules(
        "omnicoach",
        warnings
            .iter()
            .filter_map(|warning| extract_rule_name(warning)),
    );
    let formatted_warnings = warnings.join("\n\n");
    let inject_steps = vec![WireInjectStep {
        ephemeral_message: indoc::formatdoc! {"
            [omnicoach] Proposed file modification(s) introduced code lint warnings. Please review and fix them if appropriate:

            {formatted_warnings}

            {footer}"},
    }];
    Ok(serde_json::to_string(&WirePreInvocationResult {
        inject_steps,
    })?)
}
```

### 3.4 Bounded `WarningQueue` (`src/hook/warning_queue.rs`)

Enforce `MAX_QUEUED_WARNINGS = 64`:

- When pushing new warnings, if total entries exceed 64, retain the 64 most recent entries.

---

## 4. Phase 4 Ordered TDD Task Plan

Following `/phased-engineering-workflow` (Audit $\to$ RED $\to$ GREEN $\to$ Verify):

### Task T1: Expose `RuleCatalog` Footer Helper & Update DAG

- Add `RuleCatalog` dependency to `Hook` in `src/architecture.rs`.
- Expose `code_report_footer_for_rules` in `src/rule_catalog.rs`.
- *Verification*: `cargo test architecture_conformance` and unit tests pass.

### Task T2: Bounded `WarningQueue` Capacity

- Add test in `src/hook/warning_queue.rs` verifying that pushing 70 entries truncates to the 64 most recent unique entries.
- Implement capacity capping under the file lock.
- *Verification*: `cargo test hook::warning_queue` passes.

### Task T3: Workspace Boundary, Lazy Net-New Scoping, and Enriched Warning Formatting

- In `src/hook/runner.rs`:
  - Add test: file edits outside `workspace_paths` (e.g. `/tmp`) produce 0 warnings.
  - Add test: file edits in a workspace with a non-local line-42 spillover report the spillover while suppressing line 1–20 legacy violations.
  - Add test: file edits that insert a violation above an unmodified line do not unmask the pre-existing unmodified violation.
  - Add test: `format_code_warning` includes `Rationale: ...`, `Suggestion: ...`, and optional `Or: ...`.
  - Implement `filter_relevant_diagnostics`, `is_within_workspace`, and enriched formatting.
- *Verification*: `cargo test hook::runner` passes.

### Task T4: Conditional Guidance Footer in `PreInvocation` & Documentation

- In `src/hook/antigravity.rs`:
  - Add test verifying `serialize_pre_invocation` includes `SUPPRESSION_NOTE` for heuristic rules and omits it for exact/unopinionated rules.
  - Update `tests/cli.rs` integration tests.
- *Verification*: All repository tests pass:

  ```bash
  cargo fmt --check
  cargo clippy --all-targets -- -D warnings
  cargo test
  cargo run -- code
  ```
