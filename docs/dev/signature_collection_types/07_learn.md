# Phase 7: Learn — signature collection type rules

Process and principles only; the code, tests, and docs 01–06 hold the technical details. Lessons 6 and 7 are also recorded as message standards in `docs/dev/naming_and_message_style_guide.md` §2.1 and §2.4.

---

## 1. Testing

1. **Mutation-check exemptions as a step, not a review finding.** This repeats lesson 1 of `prefer-tuple-unpacking`: two `abstractmethod_exempt` cases still passed with the exemption disabled, because an `abc.ABC` base exempted them too. Reading the cases did not catch it; disabling each exemption in turn with a script did. Run that script before review, not after.
2. **Prove the bug before fixing it.** Each correctness fix started with a case that failed on the old code. Several fixes added complexity (per-function keys for returned bindings, analysis of nested-def defaults); the failing case is what justifies that complexity, and it stops the bug from coming back.
3. **Test message content below the harness.** `rule_test!` checks spans, not text, so the inferred `{replacement}` and the capability detectors are only tested by unit cases on the helpers that compute them. Whatever a rule computes and puts in a message needs a unit test of its own.

## 2. Design

4. **Check prior art at the level of each token, not only the concept.** The concept had references; the meaning of unqualified `Set` did not, and Ruff `PYI025` showed the plan was wrong. For each name a rule classifies, look up how established linters treat it before fixing the semantics.
5. **Sharing a predicate changes every consumer.** Extracting `has_imposed_signature()` changed `identical-positional-types` (`__call__` checked, `.register` exempt). When a refactor shares logic, list the behavior changes for each consumer and add a case for each.
6. **Messages describe the rule, not its configuration.** The first wording said "by default" and offered "add a comment", which is false once a user sets `ban`. Anything a user can configure (thresholds, lists, mode) stays out of static text or becomes a placeholder; a mode-dependent hint belongs to the framework (ROADMAP: mode-aware explanation hint).

## 3. Review

7. **Review the message as the user reads it, and check its certainty.** Three reviewer passes checked behavior and tests; the user's review found what they missed: a suggestion presented the read-only counterpart as *the* fix when the rule never looks at usage. For each message, ask what the rule actually knows (annotation only, a syntactic body walk, same-file callers) and word the summary and suggestion to match. Frame the flagged choice as a design decision, not an error with one fix.
8. **Triage every finding with a reason.** As before: fix correctness and coherence gaps now with a failing case first, defer limits that need new capability to the ROADMAP with a `known_gap_*` case, reject what contradicts a validated decision.
