# Architecture & Design Decisions

This document records the foundational design choices for the Custom Lint Toolkit.

## Branch 1: Core Parsing Strategy
**Decision:** Dedicated Compiler-Grade ASTs per Language (`ruff_python_parser` + `ruff_python_ast` for Python, `ra_ap_syntax` for Rust) behind a unified `AstNode<'a>` boundary (`{ file: &'a ParsedFile, span: SourceSpan }`).
**Rationale:**
- **Typed ASTs without `unsafe`**: Tree-sitter CSTs required hundreds of unchecked string `.kind()` / `.field()` lookups and leaked editor CST quirks (`Final[int]` as `generic_type` vs. `typing.Final[int]` as `subscript`, bare `"-"` sibling tokens in `match` patterns, 5 parameter shapes). Code-generated `type-sitter` wrappers introduced 940 `unsafe` blocks (violating `unsafe_code = "forbid"`) without fixing CST shape quirks.
- **Why `ruff_python_ast` and `ra_ap_syntax`**: Both are pure Rust, zero-`unsafe` at the call site, error-tolerant (unlike `syn` or `rustpython-parser`, which abort on the first syntax error or discard comments), and actively maintained by Astral and `rust-analyzer`.
- **Strict Encapsulation**: Parser crates are confined to `src/code_lint/ast/`. Rules receive only opaque `Copy` `AstNode<'a>` handles and domain projections (`AstCallCandidate`, `ScopePositionalReads`, `PythonFunctionSignature`, etc.).

## Branch 2: Rule Definition Interface
**Decision:** Static Rust `CodeRule` and `CommandRule` Declarations Over Shared Semantic Extractors
**Rationale:**
- **Single-Source-of-Truth Declarations:** Every rule is a `const RULE` combining its `Declaration` (name, `ViolationTemplate`, supported languages, typed `RuleOptions`, `Classification` facets, and `RuleDoc` with executed `Example`s) and its evaluation function.
- **Shared Semantic Extractors & Call Patterns:** Common structural queries (`CallPattern` matching `pkg.func`, `*.method`, `*().method`, `pkg.func().method`; import-alias resolution via `OnceLock<ImportMap>`; parameter usage classification; scope call graphs) live in `code_lint::ast` and `code_lint::semantic`, keeping individual rule files concise and declarative.
- **Static Compilation:** All rules are statically compiled into the binary and validated by `tests/registry.rs` at test time.

## Branch 3: Configuration & Standard Alignment
**Decision:** Option B: Ruff-Aligned TOML
**Rationale:** We adopt a Ruff-like `.omnicoach.toml` configuration syntax. It supports `select`/`ignore` filtering by rule names and taxonomy tags, `per-file-ignores`, per-rule and per-language configuration tables (`[rules.<name>]` and `[rules.<name>.<lang>]`), inline suppression comments (`omni:ignore[rule] -- reason` / `omnicoach:ignore[rule] -- reason`), and aligns cleanly with modern linter ergonomics.
