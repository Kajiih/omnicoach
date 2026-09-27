# Phase 7: Learn — `prefer-tuple-unpacking`

Process and principles only; the code, tests, and docs 01–06 hold the technical details. Lessons 1–3 are also recorded as testing standards in `docs/dev/rule_design_guide.md` §6.

---

## 1. Testing

1. **A pass case must be able to fail.** An exemption case that also satisfies another pass condition (here: a single surviving read) still passes with the exemption deleted, so it proves nothing. Write each exemption case so the exemption is the only reason it passes, and confirm it once by disabling the exemption and watching the case fail.
2. **Test a fact in the layer that owns it.** When the harness cannot express a case, a unit test on the helper that computes the fact is simpler and more precise than bending the harness. Change the harness only when a whole class of rules needs it, with a guardrail so the change cannot hide regressions.
3. **Name known gaps as tests.** An accepted limitation gets a `known_gap_*` pass case and a ROADMAP entry. The gap stays visible, and the day it is fixed the case fails and forces an update.

## 2. Design

4. **Challenge constraints before designing around them.** Of the four constraints listed up front, one came from a misread check and one rested on an untested assumption. Classify each constraint as real, idiomatic, or artifact before it shapes the design.
5. **Verify assumptions where they are made, and sweep the docs when one falls.** "Dogfooding will catch X" was carried through three phase docs before anyone ran it, and survived in four places after it was disproved. Give every claim a plan depends on a cheap check in the phase that makes it; when a claim is invalidated, grep every phase doc for it.
6. **Keep languages coherent.** When one language handles a construct, ask whether the other has the same construct; this found a real false positive. Defer heuristic limits that apply to every language together.

## 3. Review

7. **Use independent reviewers and triage every finding.** Reviewers who did not write the code, each given the same context and a separate area, found a real false-positive source and tests that proved nothing after the author's own passes. Decide each finding explicitly with a reason: fix coherence gaps and weak tests now, defer limits that need new capability to the ROADMAP with an example, reject what contradicts a validated decision.
