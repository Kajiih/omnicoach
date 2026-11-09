//! Flags imports of external third-party packages or crates outside their designated owner components.

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
    summary: "Module `{module}` imports external package `{dependency}` outside its owner {owner}.",
    rationale: "Spreading direct third-party calls across the codebase leaks external types and makes library upgrades touch unrelated modules.",
    suggestion: "Access `{dependency}` through the adapter API exposed by {owner}.",
};

/// The rule's declaration.
pub const RULE: ProjectCodeRule = ProjectCodeRule {
    declaration: Declaration {
        name: RuleName("unowned-external-dependency"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::ARCHITECTURE],
            precision: Precision::Exact,
            consensus: Consensus::Unopinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags imports of third-party packages or crates outside their declared owner components.",
            what_it_does: indoc::indoc! {r"
                Checks imports and path references against `owns-external` on
                `[[architecture.components]]` in `.omnicoach.toml` or `(owns_external: [...])` in
                `define_architecture!`. When a third-party package or crate has designated owner
                components, any direct import from outside those owner component subtrees is flagged."},
            why_is_this_bad: indoc::indoc! {r"
                Low-level or volatile external dependencies (such as HTTP clients, database drivers,
                or language parser crates) are easier to test, mock, and upgrade when encapsulated
                behind an internal adapter component. Direct imports scattered across domain logic
                couple the entire codebase to third-party APIs."},
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
                                name = "Http"
                                roots = ["pkg.http"]
                                owns-external = ["httpx"]

                                [[architecture.components]]
                                name = "Domain"
                                roots = ["pkg.domain"]
                                depends-on = ["Http"]
                            "#},
                        ),
                        ("pkg/__init__.py", ""),
                        (
                            "pkg/http.py",
                            "def fetch_json(url: str) -> dict:\n    return {}\n",
                        ),
                    ],
                    path: "pkg/domain.py",
                    flagged: indoc::indoc! {r"
                        import httpx
                    "},
                    flagged_span: "import httpx",
                    fixed: indoc::indoc! {r"
                        from pkg.http import fetch_json
                    "},
                },
                ProjectExample {
                    language: Language::Rust,
                    context: &[
                        (
                            ".omnicoach.toml",
                            indoc::indoc! {r#"
                                [[architecture.components]]
                                name = "Ast"
                                roots = ["ast"]
                                owns-external = ["ra_ap_syntax"]

                                [[architecture.components]]
                                name = "Rules"
                                roots = ["rules"]
                                depends-on = ["Ast"]
                            "#},
                        ),
                        ("src/lib.rs", "pub mod ast;\npub mod rules;\n"),
                        ("src/ast.rs", "pub struct ParsedFile;\n"),
                    ],
                    path: "src/rules.rs",
                    flagged: indoc::indoc! {r"
                        use ra_ap_syntax::SyntaxNode;
                    "},
                    flagged_span: "use ra_ap_syntax::SyntaxNode;",
                    fixed: indoc::indoc! {r"
                        use crate::ast::ParsedFile;
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
        .unowned_external_dependencies
        .iter()
        .map(|finding| {
            let formatted_owners = format_owners(&finding.owners);
            rule.diagnostic_at_span(
                path,
                file,
                finding.span,
                &[
                    ("module", &finding.module),
                    ("dependency", &finding.dependency),
                    ("owner", &formatted_owners),
                ],
            )
        })
        .collect()
}

fn format_owners(owners: &[String]) -> String {
    owners
        .iter()
        .map(|prefix| format!("`{prefix}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                imported_inside_owner_component => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Http"
                        roots = ["pkg.http"]
                        owns-external = ["httpx"]
                    "#,
                    "pkg/__init__.py" => "",
                    "pkg/http.py" => "
                        import httpx
                    ",
                },
            ],
            fail: [
                imported_outside_owner_component => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Http"
                        roots = ["pkg.http"]
                        owns-external = ["httpx"]

                        [[architecture.components]]
                        name = "Domain"
                        roots = ["pkg.domain"]
                        depends-on = ["Http"]
                    "#,
                    "pkg/__init__.py" => "",
                    "pkg/http.py" => "",
                    "pkg/domain.py" => "
                        import httpx
                    ",
                } => ("pkg/domain.py", "import httpx"),
                imported_outside_multiple_owner_components => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "HttpA"
                        roots = ["pkg.http_a"]
                        owns-external = ["httpx"]

                        [[architecture.components]]
                        name = "HttpB"
                        roots = ["pkg.http_b"]
                        owns-external = ["httpx"]

                        [[architecture.components]]
                        name = "Domain"
                        roots = ["pkg.domain"]
                        depends-on = ["HttpA", "HttpB"]
                    "#,
                    "pkg/__init__.py" => "",
                    "pkg/http_a.py" => "",
                    "pkg/http_b.py" => "",
                    "pkg/domain.py" => "
                        import httpx
                    ",
                } => ("pkg/domain.py", "import httpx"),
            ],
        },
        Rust => {
            pass: [
                used_inside_owner_component_via_define_architecture => {
                    "src/lib.rs" => "
                        mod architecture;
                        pub mod ast;
                    ",
                    "src/architecture.rs" => r#"
                        define_architecture! {
                            Ast => [] (owns_external: ["ra_ap_syntax"]),
                        }
                    "#,
                    "src/ast.rs" => "
                        architecture_component!(Ast);
                        use ra_ap_syntax::SyntaxNode;
                    ",
                },
            ],
            fail: [
                used_outside_owner_component_via_define_architecture => {
                    "src/lib.rs" => "
                        mod architecture;
                        pub mod ast;
                        pub mod rules;
                    ",
                    "src/architecture.rs" => r#"
                        define_architecture! {
                            Ast => [] (owns_external: ["ra_ap_syntax"]),
                            Rules => [Ast],
                        }
                    "#,
                    "src/ast.rs" => "architecture_component!(Ast);",
                    "src/rules.rs" => "
                        architecture_component!(Rules);
                        use ra_ap_syntax::SyntaxNode;
                    ",
                } => ("src/rules.rs", "use ra_ap_syntax::SyntaxNode;"),
            ],
        },
    }
);
