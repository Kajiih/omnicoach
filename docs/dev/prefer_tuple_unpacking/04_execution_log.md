# Phase 4: Execution Log — `prefer-tuple-unpacking`

Records **Phase 4 (Execute)**. Each task ran Audit → RED → GREEN → standard verification (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`). All tasks ended green.

---

## 1. Tasks

| # | Result |
| :--- | :--- |
| T1 | Types and facade in `ast.rs`, minimal Python / Rust collectors, rule, registration. RED: the 2 fail cases reported 0 diagnostics. |
| T2 | End-relative indices (`xs[-1]`, decimal literal only, R7) and stable receivers (no call, D13). RED: `call_receiver`, `head_and_tail_positions`. |
| T3 | Python collection uses: write targets, non-literal / slice / tuple-key / non-decimal indices, iteration, `len()`, mutating methods. RED: all 10 exemption cases. |
| T4 | Lambdas skipped, class bodies walked without a scope. RED: `lambda_body_skipped`, `class_body_ignored`. |
| T5 | Full Rust collector: stable receivers, field writes and `&mut` borrows (R5), transparent closures. RED: `call_receiver`, `field_write_exempts_receiver`, `mutable_borrow_exempts_receiver`. |
| T6 | `jj.rs` L95 destructured (`let (start, end) = cmd.span;`). |
| T7 | README `### Code Style` entry, ROADMAP follow-ups, design guide §6 note. |
| T8 | Collector unit test `test_collect_positional_reads_module_scope`; mutation-checked (fails when the module scope is dropped). No harness change (03 §7 P2). |

## 2. Deviations From the Plan

- **`jj.rs` L95 was never detected.** It sits inside `vec![...]`, whose arguments are an opaque `token_tree` (R8), so dogfooding passed before T6. Fixed anyway as a genuine instance. The gap is pinned by the pass case `known_gap_macro_arguments_not_inspected` and tracked in `ROADMAP.md`.
- **No `const` / `static` branch in the Rust collector.** Outside functions they are already ungrouped; inside, grouping stays fixable (03 §3.2 updated).
- **Dogfooding found no other violation** in `src/` and `tests/`.

## 3. Manual Checkpoint

`omni-code-lint` on scratch Python and Rust fixtures covering CUJ1–CUJ3: flagged `point[0]`, `xs[0]` (positions `-1, 0`), module-level `sys.argv[1]`, `self.1`, `cmd.span.0`; not flagged: iterated `argv`, `row[0]` / `row[4]`, mutated `t`. Messages render the receiver and positions as designed (R4).
