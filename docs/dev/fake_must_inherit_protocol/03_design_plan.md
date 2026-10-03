# Phase 3: Design & Plan — `fake-without-protocol`

This document records **Phase 3 (Design / Plan)** for implementing `fake-without-protocol` (ported from the legacy linter's `FakeMustInheritProtocolRule`). It builds on validated [01_understand.md](01_understand.md) (G1–G5, NG1–NG5, D1–D10, Q1–Q5) and [02_references.md](02_references.md).

> **Status**: **VALIDATED** (2026-10-03). Approved for Phase 4 implementation.

---

## 1. Definition of Done

### 1.1 Critical User Journeys (CUJs)

| ID | Journey | Acceptance Criteria |
| :--- | :--- | :--- |
| **CUJ1 (Base-less Fake Class)** | A Python developer writes `class FakeUserRepository:` (or `_FakeClient`, `Fake_Client`, `Fake2FA`, `Fake`, `@dataclass class FakeUserRepository:`) with no base class. | Emits 1 diagnostic anchored on the class name identifier (`FakeUserRepository`), with orthogonal `summary`, `rationale`, and `suggestion`. |
| **CUJ2 (Trivial / Non-Contract Base Class)** | A developer writes `class FakeClient(object):`, `class FakeRepo(Generic[T]):`, `class FakeClient(Protocol):`, or `class FakeClient(metaclass=ABCMeta):` without a collaborator base class. | Emits 1 diagnostic anchored on the class name identifier. |
| **CUJ3 (Valid Collaborator Base Class)** | A developer writes `class FakeUserRepository(UserRepository):`, `class FakeClient(http.HttpClient):`, `class FakeRepo(Repository[T], Generic[T]):`, or `class FakeBaseStorage(ABC):`. | Emits 0 diagnostics. |
| **CUJ4 (Non-Fake Class Name)** | A developer writes `class Faker:`, `class FakerProvider:`, `class Fakeable:`, or `class UserRepository:`. | Emits 0 diagnostics. |
| **CUJ5 (Configuration & Suppression)** | A project configures `[rules.fake-without-protocol] enforcement-mode = "require-explanation"` in `.omnilint.toml` or places `# omni:ignore [fake-without-protocol] -- reason` above a class. | Handled uniformly by `CodeRule::check_file` (`RuleOptions::code_rule(())`) and `SuppressionTracker`. |

### 1.2 Success Metrics

| Metric | Target |
| :--- | :--- |
| False positives on repository dogfooding (`src/`, `tests/`) | **0** |
| Registry & style conformance (`tests/registry.rs`, `tests/architecture_conformance.rs`) | **100%** pass |
| Test coverage | All `pass` and `fail` cases in `rule_test!`, unit tests in `ast::python::tests`, and per-exemption mutation verification |

---

## 2. Exact AST Changes in `src/code_lint/ast/python.rs`

### 2.1 Fix `extract_classes` Superclass Child Filtering (`D10` / `G5`)

In [src/code_lint/ast/python.rs L550–561](../../../src/code_lint/ast/python.rs#L550-L561), `extract_classes` currently collects children of `superclasses` (`argument_list`) with:

```rust
        let mut bases = Vec::new();
        if let Some(superclasses) = class_node.field("superclasses") {
            for child in superclasses.children() {
                let kind = child.kind();
                if kind != "(" && kind != ")" && kind != "," && kind != "keyword_argument" {
                    bases.push(PythonBaseClass {
                        name: child.text().to_string(),
                        node: AstNode::from_raw(child),
                    });
                }
            }
        }
```

We tighten this filter to require `child.is_named() && !child.is_extra() && child.kind() != "keyword_argument"` (matching `call_argument_nodes` in [src/code_lint/ast.rs L189](../../../src/code_lint/ast.rs#L189)):

```rust
        let mut bases = Vec::new();
        if let Some(superclasses) = class_node.field("superclasses") {
            for child in superclasses.children() {
                if child.is_named() && !child.is_extra() && child.kind() != "keyword_argument" {
                    bases.push(PythonBaseClass {
                        name: child.text().to_string(),
                        node: AstNode::from_raw(child),
                    });
                }
            }
        }
```

Why this works:
- Unnamed punctuation tokens (`(`, `)`, `,`) have `child.is_named() == false`.
- Comments (`comment`) and line continuations (`line_continuation`) inside `class FakeClient(\n    # comment\n):` have `child.is_extra() == true`.
- Keyword arguments (`metaclass=ABCMeta`, `total=False`) have `child.kind() == "keyword_argument"`.

### 2.2 New Helper Methods on `PythonBaseClass<'a>` and `PythonClassInfo<'a>`

In [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs), add:

```rust
impl<'a> PythonBaseClass<'a> {
    /// Base class name with any generic type argument subscript (`[...]`) stripped.
    #[must_use]
    pub fn unsubscripted_name(&self) -> &str {
        self.name
            .split_once('[')
            .map_or(self.name.as_str(), |(base, _)| base.trim())
    }

    /// Returns true if this base class can represent a collaborator contract (an interface,
    /// domain `Protocol`, `ABC`, or concrete base class) rather than a structural marker
    /// (`object`, `Generic`, or `Protocol` itself).
    #[must_use]
    pub fn is_contract_base(&self) -> bool {
        !matches!(
            self.unsubscripted_name(),
            "object"
                | "builtins.object"
                | "Generic"
                | "typing.Generic"
                | "typing_extensions.Generic"
                | "Protocol"
                | "typing.Protocol"
                | "typing_extensions.Protocol"
        )
    }
}
```

And on `PythonClassInfo<'a>`:

```rust
impl<'a> PythonClassInfo<'a> {
    /// Returns true if the class name starts with the word `Fake` (after any leading `_`),
    /// such as `FakeClient`, `_FakeClient`, `Fake_Client`, `Fake2FA`, or `Fake`, but not
    /// words where `Fake` is followed by a lowercase letter (`Faker`, `Fakeable`).
    #[must_use]
    pub fn is_fake_class_name(&self) -> bool {
        self.name
            .trim_start_matches('_')
            .strip_prefix("Fake")
            .is_some_and(|rest| {
                rest.is_empty() || !rest.starts_with(|ch: char| ch.is_ascii_lowercase())
            })
    }

    /// Returns true if the class declares at least one collaborator contract base class
    /// (excluding `object`, `Generic[...]`, and `Protocol[...]`).
    #[must_use]
    pub fn has_contract_base(&self) -> bool {
        self.bases.iter().any(PythonBaseClass::is_contract_base)
    }
}
```

---

## 3. Exact Rule Declaration (`src/code_lint/rules/fake_without_protocol.rs`)

### 3.1 `ViolationTemplate`, `Classification`, `RuleDoc`, and `check_file`

```rust
//! Flags Python `Fake*` classes that do not inherit from a `Protocol` or base class.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::extract_classes;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Fake class `{class}` does not inherit from a `Protocol` or base class.",
    rationale: "A standalone fake class is not checked against its collaborator's contract at definition time, so tests keep passing when the real interface changes.",
    suggestion: "Add the collaborator's `Protocol` or `ABC` as a base class of `{class}`.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("fake-without-protocol"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::TEST_DOUBLES],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Requires Python `Fake*` classes to inherit from a `Protocol` or base class.",
            what_it_does: "Flags any Python class whose name starts with the word `Fake` (after \
                           any leading underscores, such as `FakeRepository`, `_FakeHttpClient`, \
                           `Fake_Client`, `Fake2FA` or `Fake`) when its class header does not \
                           list a collaborator base class, in all Python files. Names where \
                           `Fake` is only part of a longer word, such as `Faker` or `Fakeable`, \
                           are not flagged. Base classes that do not supply a collaborator \
                           contract — `object`, `Generic` and `Protocol` (bare or qualified \
                           through `builtins`, `typing` or `typing_extensions`) — do not count \
                           on their own, so `class FakeClient(object):`, \
                           `class FakeRepo(Generic[T]):` and `class FakeClient(Protocol):` are \
                           still flagged.",
            why_is_this_bad: "Under PEP 544, a class that does not subclass a `Protocol` is only \
                              checked against that `Protocol` at typed call sites. When a test \
                              function or fixture is unannotated, or exercises only part of the \
                              collaborator's interface, a standalone `Fake*` class is never \
                              checked against the real contract: if the `Protocol` or `ABC` adds \
                              a method or changes a signature, the test keeps passing while \
                              production breaks.\n\n\
                              Subclass the collaborator's `Protocol` or `ABC` explicitly \
                              (`class FakeUserRepository(UserRepository):`) so the type checker \
                              verifies every method signature at the class definition and \
                              rejects instantiating a fake with missing methods.",
            references: &[
                Reference {
                    title: "PEP 544: Explicitly Declaring Implementation",
                    url: "https://peps.python.org/pep-0544/#explicitly-declaring-implementation",
                },
                Reference {
                    title: "Software Engineering at Google, ch. 13: Test Doubles",
                    url: "https://abseil.io/resources/swe-book/html/ch13.html",
                },
            ],
            examples: &[Example {
                language: SupportLang::Python,
                flagged: indoc::indoc! {r"
                    class FakeUserRepository:
                        def __init__(self) -> None:
                            self.users: dict[str, str] = {}

                        def save(self, user_id: str, email: str) -> None:
                            self.users[user_id] = email
                "},
                flagged_span: "FakeUserRepository",
                fixed: indoc::indoc! {r"
                    class FakeUserRepository(UserRepository):
                        def __init__(self) -> None:
                            self.users: dict[str, str] = {}

                        def save(self, user_id: str, email: str) -> None:
                            self.users[user_id] = email
                "},
            }],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    extract_classes(file)
        .into_iter()
        .filter(|class| class.is_fake_class_name() && !class.has_contract_base())
        .map(|class| rule.diagnostic_at_node(path, &class.name_node, &[("class", &class.name)]))
        .collect()
}
```

### 3.2 Registration & Companion Updates

1. **[src/code_lint/rules.rs](../../../src/code_lint/rules.rs)**:
   - Add `pub mod fake_without_protocol;` in alphabetical order.
   - Add `&fake_without_protocol::RULE,` to `CODE_RULES`.
2. **[src/code_lint/rules/mock_in_tests.rs](../../../src/code_lint/rules/mock_in_tests.rs)** (`Q5`):
   - In `pass` case `state_based_fake_repository` (L170), update `class FakeUserRepository:` to `class FakeUserRepository(UserRepository):` so Omni's own test suite models explicit protocol inheritance on fakes.
3. **[tests/snapshots/cli__list_rules.snap](../../../tests/snapshots/cli__list_rules.snap)** (if `--list-rules` snapshot is tested):
   - Update snapshot if needed when `fake-without-protocol` is added to `CODE_RULES`.

---

## 4. Verification & Test Plan

### 4.1 Complete `rule_test!` Cases in `src/code_lint/rules/fake_without_protocol.rs`

#### `pass` Cases

| Case Name | Code Snippet | Exemption / Boundary Verified |
| :--- | :--- | :--- |
| `inherits_domain_protocol` | `class FakeUserRepository(UserRepository): pass` | Direct domain `Protocol` / base class |
| `inherits_qualified_base` | `class FakeHttpClient(http.ClientProtocol): pass` | Attribute/dotted base class (`http.ClientProtocol`) |
| `inherits_generic_protocol` | `class FakeRepository(Repository[User]): pass` | Subscripted generic domain protocol (`Repository[User]`) |
| `inherits_pep695_generic_protocol` | `class FakeRepository[T](Repository[T]): pass` | PEP 695 type parameters with domain base class |
| `inherits_protocol_and_generic` | `class FakeRepository(Repository[T], Generic[T]): pass` | Multiple bases where one is `Generic[T]` and one is a contract base |
| `inherits_abc` | `class FakeBaseStorage(ABC): pass` | Shared abstract fake base class (`ABC` / `abc.ABC`) |
| `non_fake_class_without_base` | `class UserRepository: pass` | Ordinary class not starting with `Fake` |
| `word_starting_with_fake_exempt` | `class Faker: pass\nclass FakerProvider: pass\nclass Fakeable: pass` | Word-boundary check (`Faker`, `FakerProvider`, `Fakeable`) |
| `lowercase_fake_prefix_exempt` | `class fake_client: pass` | Case-sensitive `Fake` check (`fake_client` is not PascalCase `Fake*`) |

#### `fail` Cases

| Case Name | Code Snippet | Expected Span | Condition Verified |
| :--- | :--- | :--- | :--- |
| `bare_fake_class` | `class FakeUserRepository:\n    pass` | `"FakeUserRepository"` | Standard `Fake*` class without parentheses |
| `empty_parentheses_fake_class` | `class FakeHttpClient():\n    pass` | `"FakeHttpClient"` | Empty base list `()` |
| `private_leading_underscore_fake` | `class _FakeGateway:\n    pass` | `"_FakeGateway"` | Leading `_` stripped before `Fake` word check |
| `underscore_separated_fake` | `class Fake_Client:\n    pass` | `"Fake_Client"` | `Fake_` word boundary |
| `digit_followed_fake` | `class Fake2FAVerifier:\n    pass` | `"Fake2FAVerifier"` | `Fake2` digit word boundary |
| `exact_fake_name` | `class Fake:\n    pass` | `"Fake"` | Exact `Fake` identifier |
| `dataclass_fake_without_base` | `@dataclass(frozen=True, slots=True)\nclass FakeSessionStore:\n    sessions: dict[str, str]` | `"FakeSessionStore"` | Decorated `@dataclass` fake without base class (`Q3`) |
| `inherits_only_object` | `class FakeClient(object):\n    pass` | `"FakeClient"` | Trivial `object` base (`E3`) |
| `inherits_only_builtins_object` | `class FakeClient(builtins.object):\n    pass` | `"FakeClient"` | Trivial `builtins.object` base (`E3`) |
| `inherits_only_generic` | `class FakeRepository(Generic[T]):\n    pass` | `"FakeRepository"` | Trivial `Generic[T]` base (`E3`) |
| `inherits_only_typing_generic` | `class FakeRepository(typing.Generic[T]):\n    pass` | `"FakeRepository"` | Trivial `typing.Generic[T]` base (`E3`) |
| `inherits_only_protocol` | `class FakeClient(Protocol):\n    pass` | `"FakeClient"` | Direct `Protocol` base defines a protocol, not a fake (`E4` / `Q2`) |
| `inherits_only_typing_protocol` | `class FakeClient(typing.Protocol):\n    pass` | `"FakeClient"` | Direct `typing.Protocol` base (`E4` / `Q2`) |
| `pep695_generic_without_base` | `class FakeRepository[T]:\n    pass` | `"FakeRepository"` | PEP 695 generic class `class FakeRepository[T]:` without `(Base)` |
| `metaclass_only_argument` | `class FakeClient(metaclass=ABCMeta):\n    pass` | `"FakeClient"` | Keyword argument in `superclasses` without base class (`E5`) |
| `comment_inside_superclasses` | `class FakeClient(\n    # Not a base class\n):\n    pass` | `"FakeClient"` | Comment node inside `superclasses` (`E6` / `D10`) |

### 4.2 Unit Tests in `src/code_lint/ast/python.rs`

1. `test_extract_classes_ignores_comments_and_keywords_in_superclasses`:
   - Parses `class FakeClient(\n    # comment\n    metaclass=ABCMeta,\n):\n    pass` and verifies `cls.bases.is_empty()`.
2. `test_python_class_info_fake_name_and_contract_base`:
   - Verifies `is_fake_class_name()` and `has_contract_base()` across `FakeClient`, `_FakeClient`, `Fake_Client`, `Fake2FA`, `Fake`, `Faker`, `Fakeable`, `object`, `Generic[T]`, `Protocol`, `ABC`, and `UserRepository`.

### 4.3 Per-Exemption Mutation Verification (Phase 4 Step)

During Phase 4, each exemption / filter branch will be temporarily inverted (mutated) to confirm that at least one test fails:
1. Word boundary check (`!rest.starts_with(|ch: char| ch.is_ascii_lowercase())`) → `word_starting_with_fake_exempt` fails.
2. Leading `_` stripping (`trim_start_matches('_')`) → `private_leading_underscore_fake` fails.
3. `object` / `builtins.object` exclusion → `inherits_only_object` and `inherits_only_builtins_object` fail.
4. `Generic` / `typing.Generic` exclusion → `inherits_only_generic` and `inherits_only_typing_generic` fail.
5. `Protocol` / `typing.Protocol` exclusion → `inherits_only_protocol` and `inherits_only_typing_protocol` fail.
6. `!child.is_extra()` in `extract_classes` → `comment_inside_superclasses` fails.
