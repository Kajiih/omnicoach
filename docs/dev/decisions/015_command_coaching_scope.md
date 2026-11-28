# ADR 015: Command Coaching Scope

## 1. Problem Statement

`omnicoach command` and `omnicoach hook` inspect the shell commands that agents and developers run. Shell is a full programming language: redirections, pipelines, heredocs, command and process substitutions, file-descriptor tables, launchers (`uv run`, `env`, `xargs`), and nested `sh -c` strings. Every construct a command rule interprets adds code, edge cases, and false-positive risk.

Until now nothing bounded how deep a command rule may interpret a command. Without that bound, command rules drift towards a general shell checker or a security tool, and neither is the product.

## 2. Decision

Command coaching steers agents to the single proper tool for a job through clear diagnostic messages ([ADR 008](008_rule_documentation_and_discovery.md), [ADR 010](010_naming_and_message_conventions.md)). Command rules follow five principles:

| # | Principle | What it means for a rule |
| :--- | :--- | :--- |
| **P1** | **Coaching, not enforcement.** | Command rules are not a security boundary, a sandbox, or a general shell checker. Adversarial or obfuscated input is out of scope. |
| **P2** | **Interpret a construct only as far as needed to recognize a common anti-pattern with confidence.** | A rule reads `echo hi > out.txt` far enough to point at the file-writing tool; it does not model the file-descriptor table to do so. Constructs are interpreted to recognize anti-patterns, not because the construct itself is the recommended practice. |
| **P3** | **When unsure, stay silent.** | Dynamic targets, unknown flags, and constructs outside the model produce no diagnostic, because false positives teach agents to ignore every message. Rules that judge a command's *shape* for prefix approval (such as a sandbox-bypass gate) are the exception: they may flag any construct they do not recognize, because their message tells the agent how to make the command approvable, so flagging is still coaching. |
| **P4** | **Deeper analysis needs its own design first.** | Modeling file-descriptor state (`exec 3>file`, `2>&1 >f` ordering, descriptors inherited from `{ ...; } > f`), unwrapping launchers or `sh -c` strings, and parsing a command's output each require a dedicated design document before implementation. |
| **P5** | **Delegate parsing; settle meaning once.** | `brush-parser` tokenizes and parses shell. `command_lint::command` lowers its AST into a small typed model and resolves shell semantics there (for example, default file descriptors), so rules query facts instead of re-deriving them. Rules never parse shell text by hand. |

## 3. Excluded and Deferred Scope

- **Excluded**: security guarantees, evasion-resistant parsing, and coaching for constructs that no common agent workflow produces.
- **Deferred (P4)**: launcher and wrapper unwrapping, file-descriptor table modeling, and output parsing. Each needs its own design and is tracked in [ROADMAP.md](../../../ROADMAP.md) once proposed.

## 4. Consequences

- Command rules are designed and re-evaluated with the first-principles method in [rule_design_guide.md](../rule_design_guide.md) §7: each exemption a rule needs is first treated as evidence that its abstraction or assumptions may be wrong.
- Each command rule states in `RuleDoc::known_problems` which constructs it deliberately does not interpret, so the scope boundary is visible in `--explain`.
- Reviews reject command rules that need P4-class analysis without an accepted design, and model helpers that make rules re-derive shell semantics (P5).

## References

- **Sadowski et al. (2018), *Lessons from Building Static Analysis Tools at Google*, CACM 61(4)** ([doi:10.1145/3188720](https://doi.org/10.1145/3188720)) — Shows developers ignore analyses with a high effective false-positive rate and act only on actionable messages (P2, P3).
- **Bessey et al. (2010), *A Few Billion Lines of Code Later*, CACM 53(2)** ([doi:10.1145/1646353.1646374](https://doi.org/10.1145/1646353.1646374)) — Explains why false positives and confusing diagnostics erode trust in a checker faster than missed findings (P3).
- **POSIX.1-2017, Shell Command Language §2.7 Redirection** ([pubs.opengroup.org](https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html#tag_18_07)) — Defines the default file descriptors (`<` and `<>` use 0, `>` uses 1) that the lowered model resolves once (P5).
- **GNU Bash Reference Manual §3.6 Redirections** ([gnu.org](https://www.gnu.org/software/bash/manual/html_node/Redirections.html)) — Documents the Bash extensions beyond POSIX (`&>`, `>&word`) that lowering normalizes.
- **`brush-parser` crate documentation** ([docs.rs/brush-parser](https://docs.rs/brush-parser)) — The shell parser that `command_lint::command` delegates tokenizing and parsing to (P5).
