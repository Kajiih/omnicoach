//! Flags imports and path references between peer submodules inside an architecture component with
//! sibling isolation enabled.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::contract::{ProjectCodeRule, RuleTarget};
use crate::code_lint::project::ProjectContext;
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, ImpactedQuality, Precision, ProjectExample, RuleDoc,
    RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Module `{module}` references sibling `{dependency}` inside isolated component `{component}`.",
    rationale: "Lateral dependencies between peer submodules turn independent vertical slices into a coupled tangle.",
    suggestion: "Extract the shared helper into the root module of `{component}` or a lower component.",
};

/// The rule's declaration.
pub const RULE: ProjectCodeRule = ProjectCodeRule {
    declaration: Declaration {
        name: RuleName("sibling-module-dependency"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::ARCHITECTURE],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags references between peer submodules inside an isolated architecture component.",
            what_it_does: indoc::indoc! {r"
                When an architecture component enables `isolate-children = true` in `.omnicoach.toml`
                (or is declared via `architecture_component!` in Rust), its direct child submodules
                form independent vertical slices. This rule reports any import or path reference from
                one direct child subtree of that component to a peer child subtree."},
            why_is_this_bad: indoc::indoc! {r"
                Peer submodules under a feature or language dispatcher (such as `ast::python` and
                `ast::rust`, or peer domain handlers) stay maintainable when each slice depends only
                on the component root or lower layers. Lateral imports between siblings create
                hidden coupling and circular dependencies within the component."},
            known_problems: None,
            references: &[],
            examples: &[
                ProjectExample {
                    language: Language::Python,
                    context: &[
                        (
                            ".omnicoach.toml",
                            indoc::indoc! {r#"
                                [[architecture.components]]
                                name = "Handlers"
                                roots = ["pkg.handlers"]
                                isolate-children = true
                            "#},
                        ),
                        ("pkg/__init__.py", ""),
                        ("pkg/handlers/__init__.py", "SHARED_TIMEOUT = 5\n"),
                        ("pkg/handlers/billing.py", "def charge() -> None: pass\n"),
                    ],
                    path: "pkg/handlers/orders.py",
                    flagged: indoc::indoc! {r"
                        from pkg.handlers.billing import charge
                    "},
                    flagged_span: "from pkg.handlers.billing import charge",
                    fixed: indoc::indoc! {r"
                        from pkg.handlers import SHARED_TIMEOUT
                    "},
                },
                ProjectExample {
                    language: Language::Rust,
                    context: &[
                        (
                            ".omnicoach.toml",
                            indoc::indoc! {r#"
                                [[architecture.components]]
                                name = "Handlers"
                                roots = ["handlers"]
                                isolate-children = true
                            "#},
                        ),
                        ("src/lib.rs", "pub mod handlers;\n"),
                        (
                            "src/handlers.rs",
                            "pub mod billing; pub mod orders;\npub fn shared() {}\n",
                        ),
                        ("src/handlers/billing.rs", "pub fn charge() {}\n"),
                    ],
                    path: "src/handlers/orders.rs",
                    flagged: indoc::indoc! {r"
                        use crate::handlers::billing::charge;
                    "},
                    flagged_span: "use crate::handlers::billing::charge;",
                    fixed: indoc::indoc! {r"
                        use crate::handlers::shared;
                    "},
                },
            ],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

fn check_file(
    rule: &ProjectCodeRule,
    path: &Path,
    file: &ParsedFile,
    project: &ProjectContext<'_>,
    (): (),
) -> Vec<Diagnostic> {
    let Some(findings) = project.file_architecture_findings(file, path) else {
        return Vec::new();
    };
    findings
        .sibling_module_dependencies
        .iter()
        .map(|finding| {
            rule.diagnostic_at_span(
                path,
                file,
                finding.span,
                &[
                    ("module", &finding.module),
                    ("dependency", &finding.dependency),
                    ("component", &finding.component),
                ],
            )
        })
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                imports_component_root => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Handlers"
                        roots = ["pkg.handlers"]
                        isolate-children = true
                    "#,
                    "pkg/__init__.py" => "",
                    "pkg/handlers/__init__.py" => "SHARED_LIMIT = 10",
                    "pkg/handlers/orders.py" => "
                        from pkg.handlers import SHARED_LIMIT
                    ",
                },
            ],
            fail: [
                imports_peer_submodule => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Handlers"
                        roots = ["pkg.handlers"]
                        isolate-children = true
                    "#,
                    "pkg/__init__.py" => "",
                    "pkg/handlers/__init__.py" => "",
                    "pkg/handlers/billing.py" => "def charge() -> None: pass",
                    "pkg/handlers/orders.py" => "
                        from pkg.handlers.billing import charge
                    ",
                } => ("pkg/handlers/orders.py", "from pkg.handlers.billing import charge"),
                wildcard_root_isolates_peer_children => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Handlers"
                        roots = ["pkg.handlers.**"]
                        isolate-children = true
                    "#,
                    "pkg/__init__.py" => "",
                    "pkg/handlers/__init__.py" => "",
                    "pkg/handlers/billing.py" => "def charge() -> None: pass",
                    "pkg/handlers/orders.py" => "
                        from pkg.handlers.billing import charge
                    ",
                } => ("pkg/handlers/orders.py", "from pkg.handlers.billing import charge"),
            ],
        },
        Rust => {
            pass: [
                references_component_root => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Handlers"
                        roots = ["handlers"]
                        isolate-children = true
                    "#,
                    "src/lib.rs" => "pub mod handlers;",
                    "src/handlers.rs" => "pub mod billing; pub mod orders;\npub fn shared() {}",
                    "src/handlers/billing.rs" => "pub fn charge() {}",
                    "src/handlers/orders.rs" => "
                        use crate::handlers::shared;
                    ",
                },
            ],
            fail: [
                references_peer_child_module => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Handlers"
                        roots = ["handlers"]
                        isolate-children = true
                    "#,
                    "src/lib.rs" => "pub mod handlers;",
                    "src/handlers.rs" => "pub mod billing;\npub mod orders;",
                    "src/handlers/billing.rs" => "pub fn charge() {}",
                    "src/handlers/orders.rs" => "
                        use crate::handlers::billing::charge;
                    ",
                } => ("src/handlers/orders.rs", "use crate::handlers::billing::charge;"),
                references_peer_child_module_inside_macro_arguments => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Handlers"
                        roots = ["handlers"]
                        isolate-children = true
                    "#,
                    "src/lib.rs" => "pub mod handlers;",
                    "src/handlers.rs" => "pub mod billing;\npub mod orders;",
                    "src/handlers/billing.rs" => "pub fn is_paid() -> bool { true }",
                    "src/handlers/orders.rs" => "
                        pub fn ship() {
                            assert!(super::billing::is_paid());
                        }
                    ",
                } => ("src/handlers/orders.rs", "super::billing::is_paid"),
            ],
        },
    }
);
