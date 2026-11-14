//! Flags relative imports (`from ..`) and relative paths (`super::`) that cross out of the
//! enclosing architecture component's root subtree.

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
    summary: "Relative reference `{expression}` crosses out of component `{component}`.",
    rationale: "Relative references across component boundaries hide cross-component coupling from search and break when directories move.",
    suggestion: {
        Python => "Replace `{expression}` with an absolute import from `{replacement}`.",
        Rust => "Replace `{expression}` with the root-qualified path `{replacement}`.",
    },
};

/// The rule's declaration.
pub const RULE: ProjectCodeRule = ProjectCodeRule {
    declaration: Declaration {
        name: RuleName("cross-component-relative-path"),
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
            summary: "Flags relative imports and `super::` paths that escape the enclosing architecture component.",
            what_it_does: indoc::indoc! {r"
                Checks relative imports (`from ..`) in Python and relative path prefixes (`super::`)
                in Rust inside files belonging to a declared architecture component. Relative
                references that stay inside the component's own root module subtree are allowed;
                relative references that climb above the component root or resolve into another
                component are flagged."},
            why_is_this_bad: indoc::indoc! {r"
                Within a single component, relative references keep sibling submodules portable.
                Across component boundaries, however, relative parent hops (`from ...core` or
                `super::super::core`) obscure which external component is being coupled to and make
                cross-component dependencies invisible to text search."},
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
                                name = "Domain"
                                roots = ["pkg.domain"]

                                [[architecture.components]]
                                name = "Cli"
                                roots = ["pkg.cli"]
                                depends-on = ["Domain"]
                            "#},
                        ),
                        ("pkg/__init__.py", ""),
                        ("pkg/domain/__init__.py", "class Order: pass\n"),
                        ("pkg/cli/__init__.py", ""),
                    ],
                    path: "pkg/cli/app.py",
                    flagged: indoc::indoc! {r"
                        from ..domain import Order
                    "},
                    flagged_span: "from ..domain import Order",
                    fixed: indoc::indoc! {r"
                        from pkg.domain import Order
                    "},
                },
                ProjectExample {
                    language: Language::Rust,
                    context: &[
                        (
                            ".omnicoach.toml",
                            indoc::indoc! {r#"
                                [[architecture.components]]
                                name = "Domain"
                                roots = ["domain"]

                                [[architecture.components]]
                                name = "Cli"
                                roots = ["cli"]
                                depends-on = ["Domain"]
                            "#},
                        ),
                        ("src/lib.rs", "pub mod domain;\npub mod cli;\n"),
                        ("src/domain.rs", "pub struct Order;\n"),
                        ("src/cli.rs", "pub mod app;\n"),
                    ],
                    path: "src/cli/app.rs",
                    flagged: indoc::indoc! {r"
                        use super::super::domain::Order;
                    "},
                    flagged_span: "use super::super::domain::Order;",
                    fixed: indoc::indoc! {r"
                        use crate::domain::Order;
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
        .cross_component_relative_paths
        .iter()
        .map(|finding| {
            let expression = file.span_text(finding.span);
            let replacement = match file.lang() {
                Language::Rust => format!("crate::{}", finding.dependency),
                Language::Python => finding.dependency.clone(),
            };
            rule.diagnostic_at_span(
                path,
                file,
                finding.span,
                &[
                    ("expression", expression),
                    ("component", &finding.component),
                    ("replacement", &replacement),
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
                relative_within_same_component => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Cli"
                        roots = ["pkg.cli"]
                    "#,
                    "pkg/__init__.py" => "",
                    "pkg/cli/__init__.py" => "",
                    "pkg/cli/fmt.py" => "def render() -> str: return 'ok'",
                    "pkg/cli/app.py" => "
                        from .fmt import render
                    ",
                },
            ],
            fail: [
                relative_escapes_component_root => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Domain"
                        roots = ["pkg.domain"]

                        [[architecture.components]]
                        name = "Cli"
                        roots = ["pkg.cli"]
                        depends-on = ["Domain"]
                    "#,
                    "pkg/__init__.py" => "",
                    "pkg/domain/__init__.py" => "class Order: pass",
                    "pkg/cli/__init__.py" => "",
                    "pkg/cli/app.py" => "
                        from ..domain import Order
                    ",
                } => ("pkg/cli/app.py", "from ..domain import Order"),
                relative_import_climbing_to_source_root => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Other"
                        roots = ["other_pkg"]

                        [[architecture.components]]
                        name = "Cli"
                        roots = ["pkg"]
                        depends-on = ["Other"]
                    "#,
                    "other_pkg.py" => "def helper() -> int: return 1",
                    "pkg/__init__.py" => "",
                    "pkg/sub.py" => "
                        from .. import other_pkg
                    ",
                } => ("pkg/sub.py", "from .. import other_pkg"),
            ],
        },
        Rust => {
            pass: [
                super_within_same_component => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Cli"
                        roots = ["cli"]
                    "#,
                    "src/lib.rs" => "pub mod cli;",
                    "src/cli.rs" => "pub mod app;\npub fn helper() {}",
                    "src/cli/app.rs" => "
                        use super::helper;
                    ",
                },
                super_escape_inside_test_code => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Cli"
                        roots = ["cli"]
                    "#,
                    "src/lib.rs" => "pub mod cli;",
                    "src/cli.rs" => "pub mod app;",
                    "src/cli/app.rs" => "
                        #[cfg(test)]
                        fn fixture() {
                            use super::super::domain::Order;
                        }
                    ",
                },
                super_escape_text_in_string_literal => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Cli"
                        roots = ["cli"]
                    "#,
                    "src/lib.rs" => "pub mod cli;",
                    "src/cli.rs" => "pub mod app;",
                    "src/cli/app.rs" => r#"
                        const FIXTURE: &str = "use super::super::domain::Order;";
                    "#,
                },
            ],
            fail: [
                super_escapes_component_root => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Domain"
                        roots = ["domain"]

                        [[architecture.components]]
                        name = "Cli"
                        roots = ["cli"]
                        depends-on = ["Domain"]
                    "#,
                    "src/lib.rs" => "pub mod domain;\npub mod cli;",
                    "src/domain.rs" => "pub struct Order;",
                    "src/cli.rs" => "pub mod app;",
                    "src/cli/app.rs" => "
                        use super::super::domain::Order;
                    ",
                } => ("src/cli/app.rs", "use super::super::domain::Order;"),
                super_escape_in_expression_path => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Domain"
                        roots = ["domain"]

                        [[architecture.components]]
                        name = "Cli"
                        roots = ["cli"]
                        depends-on = ["Domain"]
                    "#,
                    "src/lib.rs" => "pub mod domain;\npub mod cli;",
                    "src/domain.rs" => "pub fn load() {}",
                    "src/cli.rs" => "pub mod app;",
                    "src/cli/app.rs" => "
                        pub fn run() {
                            super::super::domain::load();
                        }
                    ",
                } => ("src/cli/app.rs", "super::super::domain::load"),
                component_root_escapes_with_super => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Domain"
                        roots = ["domain"]

                        [[architecture.components]]
                        name = "Cli"
                        roots = ["cli"]
                        depends-on = ["Domain"]
                    "#,
                    "src/lib.rs" => "pub mod domain;\npub mod cli;",
                    "src/domain.rs" => "pub struct Order;",
                    "src/cli.rs" => "
                        use super::domain::Order;
                    ",
                } => ("src/cli.rs", "use super::domain::Order;"),
                component_root_escapes_with_self_super => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Domain"
                        roots = ["domain"]

                        [[architecture.components]]
                        name = "Cli"
                        roots = ["cli"]
                        depends-on = ["Domain"]
                    "#,
                    "src/lib.rs" => "pub mod domain;\npub mod cli;",
                    "src/domain.rs" => "pub struct Order;",
                    "src/cli.rs" => "
                        use self::super::domain::Order;
                    ",
                } => ("src/cli.rs", "use self::super::domain::Order;"),
            ],
        },
    }
);
