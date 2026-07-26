# Core API Design & Modular Structure

Based on our design, the Core API is decoupled to support the two distinct tools (`omni-lint` for static files, and `omni-guard` for dynamic actions). The shared logic resides in the main library module `src/core.rs`.

## 1. The `RuleMetadata` Trait (`src/core.rs`)
To share configuration filters across different rule types without coupling them to a single enum, both file rules (`LintRule`) and command guards (`GuardRule`) implement the `RuleMetadata` trait.

```rust
pub trait RuleMetadata {
    fn code(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn tags(&self) -> &'static [Tag];
}
```

## 2. Decoupled Rule Registries (`src/core.rs`)
Instead of a single enum polluting both binaries, we separate rules by their execution domains:

```rust
// Macro helper definitions...
define_rules! {
    LintRule,
    NoLoggingInExcept => ("PY001", "no-logging-in-except", [Logging, Exceptions, Python]),
    FlatScopeEnforced => ("PY002", "flat-scope-enforced", [Style, Python]),
}

define_rules! {
    GuardRule,
    BannedForceFlag => ("CMD001", "banned-force-flag", [Safety, Cli]),
    NoEditsOnDescribedCommits => ("VCS001", "no-edits-on-described-commits", [Workflow, Vcs]),
}
```

We also define tags to categorize rules (e.g., target language, style, safety):
```rust
define_tags! {
    Logging => "Checks related to logging configurations and invocations",
    Exceptions => "Checks targeting exception handling structures",
    Python => "Checks targeting Python source code ASTs",
    Style => "Code style and formatting conventions",
    Safety => "Safety guidelines and command restrictions",
    Cli => "Command-line syntax checks",
    Workflow => "Workflow execution rules",
    Vcs => "Version control systems integrations",
}
```

## 3. Configuration & Tag Filtering (`src/core.rs`)
The `Config` struct supports selecting or ignoring rules using the `RuleMetadata` trait:

```rust
#[derive(Deserialize, Debug, Default, Clone)]
pub struct Config {
    pub select: Option<HashSet<String>>,
    pub ignore: Option<HashSet<String>>,
}

impl Config {
    pub fn is_rule_enabled(&self, rule: &impl RuleMetadata) -> bool {
        let code = rule.code();
        let name = rule.name();
        
        if let Some(ref select) = self.select {
            let matches_code_or_name = select.contains(code) || select.contains(name);
            let matches_tag = rule.tags().iter().any(|tag| {
                select.iter().any(|s| s.eq_ignore_ascii_case(tag.as_str()))
            });
            if !matches_code_or_name && !matches_tag {
                return false;
            }
        }
        
        if let Some(ref ignore) = self.ignore {
            let matches_code_or_name = ignore.contains(code) || ignore.contains(name);
            let matches_tag = rule.tags().iter().any(|tag| {
                ignore.iter().any(|s| s.eq_ignore_ascii_case(tag.as_str()))
            });
            if matches_code_or_name || matches_tag {
                return false;
            }
        }
        
        true
    }
}
```

## 4. Decoupled `Diagnostic` Struct (`src/core.rs`)
To avoid generic types or enum serialization overhead at the reporting layer, the `Diagnostic` struct stores rule metadata as static strings:

```rust
#[derive(Debug, Serialize, Clone)]
pub struct Diagnostic {
    pub rule_code: &'static str,
    pub rule_name: &'static str,
    pub message: String,
    pub file_path: String,
    pub span: (usize, usize),
}
```

Line and column calculation is handled at the final display/print stage by reading the target file once and caching line-starts, keeping byte offsets (`span`) lightweight during compilation and analysis.

## 5. The Linter Interface (`src/code.rs`)
File linter rules run static analysis on files and return diagnostics:
```rust
pub fn lint_file(path: &Path, content: &str, config: &Config) -> Vec<Diagnostic> {
    // Under the hood, delegates to ast-grep-core rules...
}
```

## 6. The Guard Interface (`src/cmd.rs` & `src/env.rs`)
Command rules act as a pre-execution firewall, evaluating command strings against the environment:
*   **Command Rules** (`src/cmd.rs`) parse raw shell strings using tokenization or AST.
*   **Environment Rules** (`src/env.rs`) check system context (like Jujutsu repo states) before allowing modifying commands to execute on described commits.

