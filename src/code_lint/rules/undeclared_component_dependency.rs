//! Flags imports and path references targeting an architecture component that the enclosing
//! component does not declare as a dependency.

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
    summary: "Module in component `{component}` references `{expression}` in `{dependency}`, which `{component}` does not depend on.",
    rationale: "Reaching across undeclared component boundaries couples layers that the architecture keeps independent and hides dependency cycles.",
    suggestion: {
        Python => "Move the shared logic into a lower component `{component}` already depends on, or add `{dependency}` to `depends-on` in `.omnicoach.toml`.",
        Rust => "Move the shared logic into a lower component `{component}` already depends on, or add `{dependency}` to the declared dependencies of `{component}`.",
    },
};

/// The rule's declaration.
pub const RULE: ProjectCodeRule = ProjectCodeRule {
    declaration: Declaration {
        name: RuleName("undeclared-component-dependency"),
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
            summary: "Flags imports and path references targeting an architecture component that the enclosing component does not depend on.",
            what_it_does: indoc::indoc! {r"
                Checks every import and path reference in a file belonging to a declared
                architecture component against the project's component dependency DAG (declared via
                `[[architecture.components]]` in `.omnicoach.toml` or `define_architecture!` and
                `architecture_component!` in Rust). Reports any reference to another component that
                is not in the enclosing component's allowed dependencies."},
            why_is_this_bad: indoc::indoc! {r"
                Layered and modular architectures rely on an explicit directed acyclic graph of
                component dependencies so lower-level domains can be understood, tested, and changed
                without pulling in higher-level adapters or entrypoints. An undeclared cross-component
                reference silently erodes those boundaries."},
            known_problems: Some(indoc::indoc! {r"
                Dynamic string-based imports (`importlib.import_module`) and reflection are not
                resolved statically."}),
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
                        ("pkg/cli.py", "def render() -> str:\n    return 'ok'\n"),
                    ],
                    path: "pkg/domain.py",
                    flagged: indoc::indoc! {r"
                        from pkg.cli import render
                    "},
                    flagged_span: "from pkg.cli import render",
                    fixed: indoc::indoc! {r"
                        def domain_value() -> str:
                            return 'ok'
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
                        ("src/cli.rs", "pub fn render() {}\n"),
                    ],
                    path: "src/domain.rs",
                    flagged: indoc::indoc! {r"
                        use crate::cli::render;
                    "},
                    flagged_span: "use crate::cli::render;",
                    fixed: indoc::indoc! {r"
                        pub fn compute() {}
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
        .undeclared_component_dependencies
        .iter()
        .map(|finding| {
            let expression = file.span_text(finding.span);
            rule.diagnostic_at_span(
                path,
                file,
                finding.span,
                &[
                    ("component", &finding.component),
                    ("expression", expression),
                    ("dependency", &finding.dependency),
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
                allowed_direct_dependency => {
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
                    "pkg/domain.py" => "class Order: pass",
                    "pkg/cli.py" => "
                        from pkg.domain import Order
                    ",
                },
                type_checking_import_exempt_when_configured => {
                    ".omnicoach.toml" => r#"
                        [architecture]
                        ignore-type-checking-imports = true

                        [[architecture.components]]
                        name = "Domain"
                        roots = ["pkg.domain"]

                        [[architecture.components]]
                        name = "Cli"
                        roots = ["pkg.cli"]
                        depends-on = ["Domain"]
                    "#,
                    "pkg/__init__.py" => "",
                    "pkg/cli.py" => "class CliContext: pass",
                    "pkg/domain.py" => "
                        from typing import TYPE_CHECKING

                        if TYPE_CHECKING:
                            from pkg.cli import CliContext
                    ",
                },
            ],
            fail: [
                reverse_layer_import => {
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
                    "pkg/cli.py" => "def run_cli() -> None: pass",
                    "pkg/domain.py" => "
                        from pkg.cli import run_cli
                    ",
                } => ("pkg/domain.py", "from pkg.cli import run_cli"),
                multi_item_import_deduplicated => {
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
                    "pkg/cli.py" => "def a() -> None: pass\ndef b() -> None: pass",
                    "pkg/domain.py" => "
                        from pkg.cli import a, b
                    ",
                } => ("pkg/domain.py", "from pkg.cli import a, b"),
            ],
        },
        Rust => {
            pass: [
                allowed_declared_dependency => {
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
                        use crate::domain::Order;
                    ",
                },
            ],
            fail: [
                reverse_component_use => {
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
                    "src/cli.rs" => "pub fn run() {}",
                    "src/domain.rs" => "
                        use crate::cli::run;
                    ",
                } => ("src/domain.rs", "use crate::cli::run;"),
                undeclared_component_in_turbofish_argument => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Domain"
                        roots = ["domain"]

                        [[architecture.components]]
                        name = "Store"
                        roots = ["store"]

                        [[architecture.components]]
                        name = "Cli"
                        roots = ["cli"]
                        depends-on = ["Domain"]
                    "#,
                    "src/lib.rs" => "
                        pub mod domain;
                        pub mod store;
                        pub mod cli;
                    ",
                    "src/domain.rs" => "pub fn load<T>() {}",
                    "src/store.rs" => "pub struct Database;",
                    "src/cli.rs" => "
                        pub fn run() {
                            crate::domain::load::<crate::store::Database>();
                        }
                    ",
                } => ("src/cli.rs", "crate::store::Database"),
            ],
        },
    }
);
