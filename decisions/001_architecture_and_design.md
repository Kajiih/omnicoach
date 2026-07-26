# Architecture & Design Decisions

This document records the foundational design choices for the Custom Lint Toolkit.

## Branch 1: Core Parsing Strategy
**Decision:** Option A: Generic Multi-Language (Tree-sitter)
**Rationale:** We will use Tree-sitter inside a Pure Rust binary to unify all three layers (Static Analysis, Command Parsing, VCS interaction). This avoids fragmented parser APIs, allows us to analyze multiple languages easily, and provides the foundation for our structural search system.

## Branch 2: Rule Definition Interface
**Decision:** Hybrid (Declarative First + Imperative Escape Hatch)
**Rationale:** 
- **Declarative First:** We will expose a declarative interface (e.g., AST pattern matching with metavariables) for the vast majority of rules. This lowers the barrier to entry, enabling rapid rule definition.
- **Imperative Escape Hatch:** For complex checks that require intricate state tracking (e.g., scoping, cross-file analysis), we will allow fallback to writing imperative Rust functions against the Tree-sitter AST nodes.
- **Static Compilation:** For simplicity, all rules (both declarative and imperative) are statically compiled into the binary. Downstream customizations are introduced directly as modifications to the repository rather than through dynamic plugins.

## Branch 3: Configuration & Standard Alignment
**Decision:** Option B: Ruff-Aligned TOML
**Rationale:** We will adopt a Ruff-like `.omnilint.toml` configuration syntax. It supports standard code categories (e.g., `PY001`), allows for `select`/`ignore` filtering, supports inline comments, and aligns cleanly with the expectations of modern developers.
