//! Flags Python `Fake*` classes that do not inherit from a `Protocol` or base class.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{PythonBaseClass, extract_classes};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Fake class `{class}` does not inherit from a collaborator `Protocol` or base class.",
    rationale: "A standalone fake class is not checked against its collaborator's contract at definition time, so tests keep passing when the real interface changes.",
    suggestion: "Add the collaborator's `Protocol` or `ABC` as a base class of `{class}`.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("fake-without-protocol"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::TEST_DOUBLES],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Requires Python `Fake*` classes to inherit from a `Protocol` or base class.",
            what_it_does: indoc::indoc! {r"
                Flags a class whose name starts with the word `Fake`, such as `FakeRepository` or
                `_FakeHttpClient`, when it has no base class supplying a collaborator contract.
                `object`, `Generic`, `Protocol` and `ABC` do not count as such a base, so
                `class FakeClient(Protocol):` is still flagged."},
            why_is_this_bad: indoc::indoc! {r"
                Under PEP 544, a class that does not subclass a `Protocol` is only checked against
                that `Protocol` at typed call sites. When a test function or fixture is unannotated,
                or exercises only part of the collaborator's interface, a standalone `Fake*` class
                is never checked against the real contract: if the `Protocol` or `ABC` adds a method
                or changes a signature, the test keeps passing while production breaks.

                Subclass the collaborator's `Protocol` or `ABC` explicitly
                (`class FakeUserRepository(UserRepository):`) so the type checker verifies every
                method signature at the class definition and rejects instantiating a fake with
                missing methods."},
            known_problems: Some(indoc::indoc! {r"
                Any other base counts as a collaborator, even an unrelated one such as
                `class FakeClient(Exception):`."}),
            references: &[
                Reference {
                    title: "PEP 544: Explicitly Declaring Implementation",
                    url: "https://peps.python.org/pep-0544/#explicitly-declaring-implementation",
                },
                Reference {
                    title: "Software Engineering at Google, ch. 13: Test Doubles",
                    url: "https://abseil.io/resources/swe-book/html/ch13.html",
                },
                Reference::NAME_RESOLUTION,
            ],
            examples: &[Example {
                language: Language::Python,
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
        .filter(|class| {
            is_fake_class_name(&class.name)
                && class
                    .bases
                    .iter()
                    .all(PythonBaseClass::is_structural_marker)
        })
        .map(|class| rule.diagnostic_at_node(path, &class.name_node, &[("class", &class.name)]))
        .collect()
}

/// Returns true if `name` starts with the word `Fake` after any leading `_`: `Fake` alone or
/// followed by anything but a lowercase letter, so `Faker` and `Fakeable` do not match.
fn is_fake_class_name(name: &str) -> bool {
    name.trim_start_matches('_')
        .strip_prefix("Fake")
        .is_some_and(|rest| !rest.starts_with(|character: char| character.is_ascii_lowercase()))
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                inherits_domain_protocol => r#"
                    class FakeUserRepository(UserRepository):
                        pass
                "#,
                inherits_qualified_base => r#"
                    class FakeHttpClient(http.ClientProtocol):
                        pass
                "#,
                inherits_generic_protocol => r#"
                    class FakeRepository(Repository[User]):
                        pass
                "#,
                inherits_pep695_generic_protocol => r#"
                    class FakeRepository[T](Repository[T]):
                        pass
                "#,
                inherits_collaborator_with_generic_marker => r#"
                    class FakeRepository(Repository[T], Generic[T]):
                        pass
                "#,
                non_fake_class_without_base => r#"
                    class UserRepository:
                        pass
                "#,
                word_starting_with_fake_exempt => r#"
                    class Faker:
                        pass
                "#,
                lowercase_fake_prefix_exempt => r#"
                    class fake_client:
                        pass
                "#,
            ],
            fail: [
                bare_fake_class => r#"
                    class FakeUserRepository:
                        pass
                "# => "FakeUserRepository",
                empty_parentheses_fake_class => r#"
                    class FakeHttpClient():
                        pass
                "# => "FakeHttpClient",
                private_leading_underscore_fake => r#"
                    class _FakeGateway:
                        pass
                "# => "_FakeGateway",
                underscore_separated_fake => r#"
                    class Fake_Client:
                        pass
                "# => "Fake_Client",
                digit_followed_fake => r#"
                    class Fake2FAVerifier:
                        pass
                "# => "Fake2FAVerifier",
                exact_fake_name => r#"
                    class Fake:
                        pass
                "# => "Fake",
                dataclass_fake_without_base => r#"
                    from dataclasses import dataclass

                    @dataclass(frozen=True, slots=True)
                    class FakeSessionStore:
                        sessions: dict[str, str]
                "# => "FakeSessionStore",
                inherits_only_object => r#"
                    class FakeObjectClient(object):
                        pass
                "# => "FakeObjectClient",
                inherits_only_builtins_object => r#"
                    class FakeBuiltinsObjectClient(builtins.object):
                        pass
                "# => "FakeBuiltinsObjectClient",
                inherits_only_generic => r#"
                    class FakeGenericRepo(Generic[T]):
                        pass
                "# => "FakeGenericRepo",
                inherits_only_typing_generic => r#"
                    class FakeTypingGenericRepo(typing.Generic[T]):
                        pass
                "# => "FakeTypingGenericRepo",
                inherits_only_protocol => r#"
                    class FakeProtocolClient(Protocol):
                        pass
                "# => "FakeProtocolClient",
                inherits_only_typing_protocol => r#"
                    class FakeTypingProtocolClient(typing.Protocol):
                        pass
                "# => "FakeTypingProtocolClient",
                inherits_only_aliased_protocol => r#"
                    from typing import Protocol as P

                    class FakeAliasedProtocolClient(P):
                        pass
                "# => "FakeAliasedProtocolClient",
                inherits_only_abc => r#"
                    class FakeBaseStorage(ABC):
                        pass
                "# => "FakeBaseStorage",
                inherits_only_abc_abc => r#"
                    class FakeQualifiedAbcStorage(abc.ABC):
                        pass
                "# => "FakeQualifiedAbcStorage",
                pep695_generic_without_base => r#"
                    class FakePep695Repo[T]:
                        pass
                "# => "FakePep695Repo",
                metaclass_only_argument => r#"
                    class FakeMetaclassClient(metaclass=ABCMeta):
                        pass
                "# => "FakeMetaclassClient",
                comment_inside_superclasses => r#"
                    class FakeCommentedClient(
                        # Not a base class
                    ):
                        pass
                "# => "FakeCommentedClient",
            ],
        },
    }
);
