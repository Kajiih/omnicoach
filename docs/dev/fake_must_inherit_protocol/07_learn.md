# Phase 7: Learn — `fake-without-protocol` (`FakeMustInheritProtocolRule`)

This document captures the reusable lessons from designing, implementing, and auditing `fake-without-protocol` (`FakeMustInheritProtocolRule`).

> Status: **COMPLETE.**

---

## 1. Key Takeaways

1. **Check the Character After `Fake`, Not Just the Prefix**:
   - `name.starts_with("Fake")` misses `_FakeRepo` and also matches `Faker` and `Fakeable`. `PythonClassInfo::is_fake_class_name` trims leading underscores, strips the `Fake` prefix, and accepts the name only if nothing follows or the next character is not an ASCII lowercase letter. It accepts camelCase (`FakeRepo`), snake_case (`Fake_Repo`), digit boundaries (`Fake2FA`), and bare `Fake`, and rejects `Faker` and `Fakeable`.
2. **Tree-Sitter Python `superclasses` Is an `argument_list`**:
   - In `tree-sitter-python`, the `superclasses` field of `class_definition` uses the `argument_list` grammar production, which can contain positional base expressions, `keyword_argument` (`metaclass=ABCMeta`, `total=False`), and `dictionary_splat` (`**kwargs` for `__init_subclass__`). Base-class extractors must filter out both `keyword_argument` and `dictionary_splat`.
3. **Distinguish Collaborator Base Classes from Structural Markers**:
   - Inheriting from `object`, `Generic[T]`, or `Protocol` itself does not supply a collaborator contract. Having `PythonBaseClass::is_contract_base` return `false` for those markers (and stating "collaborator `Protocol` or base class" in the diagnostic summary) prevents false negatives on `class FakeRepo(Generic[T]):` and `class FakeClient(Protocol):`.
