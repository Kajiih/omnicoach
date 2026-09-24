# Rule Layering & Enforced Abstraction Levels

Project combining two ROADMAP items:

- **#1 Rule Definition Layering & Declarative Cleanliness**: rule files express policy only; grammar and
  tree navigation live in `ast_*.rs`; orchestration lives in the framework runner.
- **#2 Explicit, Enforced Abstraction Levels**: one declared module graph for the whole crate, enforced
  in CI so it cannot regress.

## Phases

| # | Phase | Artifact | Status |
|---|---|---|---|
| 1 | Understand | [01_understand.md](01_understand.md) | Validated |
| 2 | Gather resources & references | [02_references.md](02_references.md) | Validated |
| 3 | Design / plan | [03_plan.md](03_plan.md) | Validated |
| 4 | Execute | [04_execution_log.md](04_execution_log.md) | Validated |
| 5 | Clean up | [05_cleanup.md](05_cleanup.md) | Validated |
| 6 | Review & audit | [06_review.md](06_review.md) | Validated |
| 7 | Learn | [07_learnings.md](07_learnings.md) | Validated |

All phases are complete and validated.
