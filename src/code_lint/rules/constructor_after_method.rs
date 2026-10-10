//! Flags class and inherent `impl` constructors defined after regular methods.

use crate::code_lint::ast::{self, ParsedFile, TypeMethod, TypeMethodScope};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Constructor `{function}` of `{class}` is defined after a non-constructor method.",
    rationale: "Burying initialization logic below regular methods forces readers to scan the body of `{class}` to find how instances are constructed.",
    suggestion: {
        Python => "Move `{function}` to the top of `{class}`, before any regular methods.",
        Rust => "Move `{function}` to the top of the `impl {class}` block, before any non-constructor methods.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("constructor-after-method"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::DECLARATION_ORDER],
            precision: Precision::Exact,
            consensus: Consensus::Unopinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags class and inherent `impl` constructors defined after regular methods.",
            what_it_does: indoc::indoc! {r"
                Flags a constructor declared after a regular method in a Python `class` or Rust
                inherent `impl` block, such as `__init__` below `def close(self)`. In Python,
                constructors are lifecycle methods such as `__new__`, `__init__` and
                `__post_init__`; in Rust, they are public associated functions without `self` named
                `new`, `try_new`, `new_*` or `try_new_*` that return `Self` or the enclosing type."},
            why_is_this_bad: indoc::indoc! {r"
                When reading a class or type implementation from top to bottom, developers look for
                constructors first to understand what state the type holds and how an instance is
                established. Placing `__init__` or `pub fn new` below regular methods hides the
                type's initialization contract in the middle or bottom of the block.

                Move all constructors to the top of the `class` or inherent `impl` block, before
                any non-constructor methods."},
            known_problems: None,
            references: &[
                Reference {
                    title: "Checkstyle: DeclarationOrder",
                    url: "https://checkstyle.org/checks/coding/declarationorder.html",
                },
                Reference {
                    title: "wemake-python-styleguide: WPS338 WrongMethodOrderViolation",
                    url: "https://wemake-python-styleguide.readthedocs.io/en/latest/pages/usage/violations/consistency.html",
                },
            ],
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r"
                        class Connection:
                            def close(self) -> None:
                                pass

                            def __init__(self, host: str) -> None:
                                self.host = host
                    "},
                    flagged_span: "__init__",
                    fixed: indoc::indoc! {r"
                        class Connection:
                            def __init__(self, host: str) -> None:
                                self.host = host

                            def close(self) -> None:
                                pass
                    "},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r"
                        struct Connection {
                            port: u16,
                        }

                        impl Connection {
                            pub fn port(&self) -> u16 {
                                self.port
                            }

                            pub fn new(port: u16) -> Self {
                                Self { port }
                            }
                        }
                    "},
                    flagged_span: "new",
                    fixed: indoc::indoc! {r"
                        struct Connection {
                            port: u16,
                        }

                        impl Connection {
                            pub fn new(port: u16) -> Self {
                                Self { port }
                            }

                            pub fn port(&self) -> u16 {
                                self.port
                            }
                        }
                    "},
                },
            ],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    ast::collect_type_method_scopes(file)
        .iter()
        .flat_map(constructors_after_methods)
        .map(|(type_name, method)| {
            rule.diagnostic_at_node(
                path,
                &method.name_node,
                &[("function", &method.name), ("class", type_name)],
            )
        })
        .collect()
}

/// Returns every constructor in `scope` that appears after at least one non-constructor method.
fn constructors_after_methods<'a>(
    scope: &'a TypeMethodScope<'a>,
) -> Vec<(&'a str, &'a TypeMethod<'a>)> {
    let mut seen_non_constructor = false;
    let mut misplaced = Vec::new();
    for method in &scope.methods {
        if method.is_constructor {
            if seen_non_constructor {
                misplaced.push((scope.type_name.as_str(), method));
            }
        } else {
            seen_non_constructor = true;
        }
    }
    misplaced
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                constructors_before_methods => r#"
                    from dataclasses import dataclass

                    class Service:
                        def __init_subclass__(cls) -> None:
                            pass

                        def __new__(cls, port: int) -> "Service":
                            return super().__new__(cls)

                        def __init__(self, port: int) -> None:
                            self.port = port

                        def connect(self) -> int:
                            return self.port

                    @dataclass(frozen=True, slots=True)
                    class FrozenRecord:
                        port: int

                        def __post_init__(self) -> None:
                            pass

                        def format_port(self) -> int:
                            return self.port
                "#,
                overloaded_init_grouped_at_first_declaration_exempt => r#"
                    from typing import overload

                    class Endpoint:
                        @overload
                        def __init__(self, port: int) -> None: ...

                        @overload
                        def __init__(self, port: str) -> None: ...

                        def default_port(self) -> int:
                            return 80

                        def __init__(self, port: int | str) -> None:
                            self.port = int(port)
                "#,
                nested_class_tracks_constructors_independently => r#"
                    class Outer:
                        def __init__(self) -> None:
                            pass

                        def build(self) -> None:
                            pass

                        class Inner:
                            def __init__(self) -> None:
                                pass

                            def execute(self) -> None:
                                pass
                "#,
            ],
            fail: [
                init_after_regular_method => r#"
                    class Connection:
                        def close(self) -> None:
                            pass

                        def __init__(self, port: int) -> None:
                            self.port = port
                "# => "__init__",
                new_after_method => r#"
                    class Session:
                        def reset(self) -> None:
                            pass

                        def __new__(cls) -> "Session":
                            return super().__new__(cls)
                "# => "__new__",
                post_init_after_method => r#"
                    class Worker:
                        def run(self) -> None:
                            pass

                        def __post_init__(self) -> None:
                            pass
                "# => "__post_init__",
            ],
        },
        Rust => {
            pass: [
                constructors_at_top_of_inherent_impl => r#"
                    pub struct Client {
                        port: u16,
                    }

                    impl Client {
                        pub fn new(port: u16) -> Self {
                            Self { port }
                        }

                        pub(crate) fn try_new(port: u16) -> Option<Self> {
                            Some(Self { port })
                        }

                        pub fn new_with_default() -> Self {
                            Self { port: 1 }
                        }

                        pub fn try_new_from_port(port: u16) -> Option<Self> {
                            Some(Self { port })
                        }

                        pub fn port(&self) -> u16 {
                            self.port
                        }
                    }
                "#,
                private_new_fn_exempt => r#"
                    pub struct Builder {
                        limit: usize,
                    }

                    impl Builder {
                        pub fn limit(&self) -> usize {
                            self.limit
                        }

                        fn new_internal() -> Self {
                            Self { limit: 0 }
                        }
                    }
                "#,
                self_receiver_new_method_exempt => r#"
                    pub struct Builder {
                        limit: usize,
                    }

                    impl Builder {
                        pub fn limit(&self) -> usize {
                            self.limit
                        }

                        pub fn new_child(&self) -> Self {
                            Self { limit: self.limit }
                        }
                    }
                "#,
                new_fn_not_returning_self_exempt => r#"
                    pub struct Builder {
                        limit: usize,
                    }

                    impl Builder {
                        pub fn limit(&self) -> usize {
                            self.limit
                        }

                        pub fn new_request_id() -> u64 {
                            1
                        }
                    }
                "#,
                separate_impl_blocks_checked_independently => r#"
                    pub struct Service;

                    impl Service {
                        pub fn execute(&self) {}
                    }

                    impl Service {
                        pub fn new_instance() -> Self {
                            Self
                        }
                    }
                "#,
                inline_test_before_constructor_exempt => r#"
                    pub struct Parser;

                    impl Parser {
                        /// Test-only helper method.
                        #[cfg(test)]
                        pub fn test_helper(&self) {}

                        pub fn new() -> Self {
                            Self
                        }
                    }
                "#,
            ],
            fail: [
                pub_new_after_getter => r#"
                    pub struct Connection {
                        port: u16,
                    }

                    impl Connection {
                        pub fn port(&self) -> u16 {
                            self.port
                        }

                        pub fn new(port: u16) -> Self {
                            Self { port }
                        }
                    }
                "# => "new",
                pub_crate_prefixed_constructor_after_method => r#"
                    pub struct Pool {
                        capacity: usize,
                    }

                    impl Pool {
                        pub fn capacity(&self) -> usize {
                            self.capacity
                        }

                        pub(crate) fn new_with_capacity(capacity: usize) -> Self {
                            Self { capacity }
                        }
                    }
                "# => "new_with_capacity",
                try_new_prefixed_constructor_after_method => r#"
                    pub struct SharedPool {
                        capacity: usize,
                    }

                    impl SharedPool {
                        pub fn capacity(&self) -> usize {
                            self.capacity
                        }

                        pub fn try_new_shared(capacity: usize) -> Option<Self> {
                            Some(Self { capacity })
                        }
                    }
                "# => "try_new_shared",
            ],
        },
    }
);
