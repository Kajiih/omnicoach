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
    summary: "Fake class `{class}` does not inherit from a collaborator `Protocol` or base class.",
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
                           contract — `object`, `Generic`, `Protocol` and `ABC` (bare or \
                           qualified through `builtins`, `typing`, `typing_extensions` or `abc`) \
                           — do not count on their own, so `class FakeClient(object):`, \
                           `class FakeRepo(Generic[T]):`, `class FakeClient(Protocol):` and \
                           `class FakeBaseStorage(ABC):` are still flagged.",
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
                inherits_protocol_and_generic => r#"
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

                    class FakerProvider:
                        pass

                    class Fakeable:
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
