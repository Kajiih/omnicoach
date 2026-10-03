# Phase 7: Learn — `inline-public-attribute-annotation` (`InstanceAttributeAnnotationRule`)

This document captures the reusable lessons from designing, implementing, and auditing `inline-public-attribute-annotation` (`InstanceAttributeAnnotationRule`).

> Status: **COMPLETE.**

---

## 1. Key Takeaways

1. **Prune Multi-Check Legacy Rules Against Ecosystem Standards Early**:
   - The legacy the legacy linter `InstanceAttributeAnnotationRule` bundled three checks: (1) inline attribute annotations in methods, (2) banning class-body annotations on plain classes, and (3) unannotated attributes. Separating and auditing each check in Phase 1–2 showed that Check 2 directly contradicts PEP 526, Pydantic, `attrs`, and `__slots__`, and Check 3 is already enforced by Mypy (`--disallow-untyped-defs`) and Pyright. Focusing Omni solely on Check 1 (`inline-public-attribute-annotation`) for public attributes (`!attr_name.starts_with('_')`) produced an `Exact` rule that complements rather than fights PEP 526.
2. **Exempt Bare `Final` Under PEP 591 While Flagging `Final[T]`**:
   - Under PEP 591, an unparameterized `self.attr: Final = value` in `__init__` cannot be moved to the class body without an explicit type argument (`attr: Final` in a class body without a RHS value or type argument is invalid). Parameterized `self.attr: Final[int] = value`, by contrast, moves cleanly to `attr: Final[int]` in the class body with `self.attr = value` in `__init__`.
3. **Do Not Cross Nested Function/Class Boundaries When Walking Method Statements**:
   - Walking method statements with a recursive block walker (`collect_method_inline_public_attr_annotations_rec`) that stops at `function_definition` and `class_definition` ensures `self.attr: Type = value` inside `if`/`for`/`with`/`try`/`match` blocks is caught while inner closures or local classes with their own `self` parameter are not attributed to the outer method.
