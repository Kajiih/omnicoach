# Phase 4 — Execution Log (P3 Dedicated AST Migration)

> **Status**: IN PROGRESS

---

## Slice 0 — Toolchain Update (`rustc 1.99.0`)

- **Action**: Ran `rustup update stable` to update the local Rust toolchain from `1.90.0` to `1.99.0 (b940084d7 2026-09-28)` and `cargo 1.99.0 (5f94df478 2026-08-27)`.
- **Verification**: Confirmed via `cargo info` that crates.io `ruff_python_parser = "=0.0.16"` (`rust-version = 1.97`), `ruff_python_ast = "=0.0.16"`, `ruff_text_size = "=0.0.2"`, and `ra_ap_syntax = "=0.0.357"` (`rust-version = 1.98`) are compatible with the local toolchain and `.github/workflows/release.yml` (`cargo publish --locked`).

---

## Slice 1 — First-Class `crate::diagnostic::Language` Enum (Decision D3) — DONE

- **Goal**: Replace `ast_grep_language::SupportLang` outside `src/code_lint/ast.rs` and `src/command_lint/command.rs` with `crate::diagnostic::Language { Python, Rust }`, and remove `support_lang_name` in favor of `Language::as_str` / `Display`.
- **Changes** (57 files, +490 / −547):
  - `src/diagnostic.rs`: `Language` with `as_str()` (configuration keys, Markdown fences), `from_path(&Path)`, `Display` (`Python` / `Rust`, used by the catalog renderer) and `strum::VariantArray` (`Language::VARIANTS`).
  - Mechanical `SupportLang` → `Language` rename in rules, semantic engines, runner, rule declarations, catalog, taxonomy, `test_utils` (including `rule_test!`, which now names `$crate::diagnostic::Language`) and `tests/registry.rs`.
  - `ParsedFile` stores `lang: Language`. Transitional bridges `to_support_lang` (private) and `from_support_lang` (`pub(in crate::code_lint::ast)`) live in `ast.rs` until `AstGrep` leaves `ParsedFile` (Slices 2 and 5).
- **Deviations from the plan** (all simplifications):
  1. `detect_language` is deleted instead of retyped: its 7 call sites use `Language::from_path`, so one concern has one API.
  2. `SUPPORTED_LANGUAGES` is deleted in favor of `Language::VARIANTS`. It only existed because `SupportLang` has 28 variants; keeping a second list would let it drift when a language is added.
  3. `dispatch_lang!` drops its `$fallback` argument and is exhaustive over `Language`: a new language becomes a compile error at every dispatch site instead of a silent `false` / `None` / `Vec::new()`. Its `pub(crate) use` re-export is removed (the macro is in textual scope for `ast/*`, and rustc 1.99 reports the path import as unused).
  4. Exhaustive matches replace the now-unreachable wildcard and panic arms in `nullable_collection_return::check_file`, `packed_assertion::check_file`, `RegisteredRule::new` (`filter_map` → `map`) and `test_utils::dummy_filename` (a test-time panic becomes a compile-time error).
  5. `Language` derives only what is used: no `serde`, no `PartialOrd` / `Ord`.
- **Toolchain follow-up (Slice 0)**: rustc 1.99 clippy lints on pre-existing code, fixed without behavior change:
  - `question_mark`: `extract_valid_field_root` (`format_strings.rs`) and `parse_directive_prefix` (`suppression.rs`).
  - `collapsible_match`: `summarize_rust_node` (`rust.rs`).
  - `assert_is_empty`: 6 test assertions (`runner.rs` ×3, `edit_of_described_commit.rs` ×2, `architecture_conformance.rs` ×1) now use `assert_eq!` against an empty value, so a failure prints the unexpected content.
- **Remaining `SupportLang`** (all expected):
  - `code_lint/ast.rs`: the bridge and `SourceDoc` (Slices 2 and 5).
  - `code_lint/ast/statements.rs`: raw-CST tests (Slices 3 and 5).
  - `command_lint/command.rs`: Bash; `command_lint` is out of scope (see `ROADMAP.md`).
  - `bin/ast_dumper.rs`: carried to Slice 5, where it is ported to dump `ruff_python_ast` / `ra_ap_syntax` trees (see `01_understand.md`) or deleted.
- **Tests**: no test added or removed. Test edits only rename the language type, apart from the 6 `assert_is_empty` assertions above.
- **Verification**: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps --document-private-items` is green:
  - lib 1396 passed;
  - `architecture_conformance` 11, `cli` 23 (includes self-dogfooding), `registry` 17;
  - doctests 8 passed, 1 ignored;
  - no rustdoc warnings.
