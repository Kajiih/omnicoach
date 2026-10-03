# Phase 7: Learn — `repeated-literal`

Process and principles only; the code, tests, and docs 01–06 hold the technical details.

---

## 1. Testing

1. **Make the mutation check a Phase 4 exit criterion.** This is the third track where exemption cases were only mutation-checked after review (04 recorded 2 of ~20). When finally run, it found weak cases that reading had missed. Run the script per exemption before Phase 5, and record the table in 04.
2. **A case must fail for exactly one reason.** Several cases passed through a second path: a `Final` case used an UPPER_SNAKE name (which hid a real `generic_type` bug), a `Literal[...]` case sat inside an already-exempt annotation, and two macro copies sat in different exempt macros. Pick inputs that satisfy only the behavior named by the case.
3. **Know what the harness compares.** `rule_test!` compares the flagged *text*, not its position. When copies are spelled the same, a case about *which* copy is flagged (constant vs inline role) passes either way. Spell the expected copy differently (`'fast'` vs `"fast"`, `r"jj"`, `404u16`) and put it where a wrong role would flag the other one.
4. **Test helpers need tests of their failure paths.** The harness rewrite claimed to reject unparsable rewrites, but `0b11` → `0b31` re-parsed as `0` plus an error node and was accepted. A harness that can silently weaken every case of a rule deserves the same rigor as the rule.
5. **Check a reviewer's test proposal against the suite's design.** The "High" proposal to snapshot `{count}` and `min-occurrences` through the CLI contradicted the suite's conventions (options tested centrally, spans not prose). The user caught it. The real gap behind it was a harness limitation (one diagnostic per `fail` case), which belongs in the ROADMAP.

## 2. Design

6. **Probe the grammar for every shape a matcher names.** Wrong assumptions about Tree-sitter shapes caused most behavior findings: `Final[int]` is a `generic_type`, `-404` in mapping/keyword patterns and macro token trees is a bare `-` token, module constants live under `if`/`try` blocks. Dump each shape before writing the arm; this is also the case for typed or validated node kinds (ROADMAP).
7. **When a value cannot be read reliably, skip it rather than guess.** Counting `-404` as `404` produced wrong findings; skipping it produces a documented miss pinned by a `known_gap_*` case. A missed finding costs less trust than a wrong one.
8. **Mechanical dogfood fixes point to a missing abstraction.** `CALLEE = "callee"` satisfies the rule without single-sourcing anything; the real fix is typed template placeholders (ROADMAP). When a rule's fix in our own code looks cargo-culted, question the abstraction before accepting the constant.

## 3. Process

9. **Isolate a track's changes from parallel work from the start.** The user's work landed in the same working copy and the same file (`ast/python.rs`), so the final squash had to move hunks by line range. Start each track in its own change (or jj workspace) and keep `@` for the active one.
