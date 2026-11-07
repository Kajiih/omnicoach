//! Flags Rust `architecture_component!` declarations that do not establish a valid component root.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::contract::{ProjectCodeRule, RuleTarget};
use crate::code_lint::project::{InvalidArchitectureComponentKind, ProjectContext};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, ImpactedQuality, Precision, ProjectExample, RuleDoc,
    RuleOptions, Topic,
};
use std::path::Path;

const COMPONENT_PLACEHOLDER: &str = "<Component>";

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Declaration `{expression}` in `{module}` is ignored: {reason}.",
    rationale: "An ignored `architecture_component!` call establishes no component boundary while making `{module}` look mapped.",
    suggestion: "Remove `{expression}`, or add `{component}` to `define_architecture!` in `architecture` with one top-level `architecture_component!` on its root module.",
};

/// The rule's declaration.
pub const RULE: ProjectCodeRule = ProjectCodeRule {
    declaration: Declaration {
        name: RuleName("invalid-architecture-component"),
        template: &TEMPLATE,
        languages: &[Language::Rust],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::ARCHITECTURE],
            precision: Precision::Exact,
            consensus: Consensus::Unopinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags Rust `architecture_component!` declarations that do not establish a valid component root.",
            what_it_does: indoc::indoc! {r"
                Checks every production `architecture_component!(...)` macro call in a Rust crate
                and flags declarations that `omnicoach` ignores: calls in a crate without
                `define_architecture!` in its `architecture` module (`src/architecture.rs` or
                `src/architecture/mod.rs`), calls in a repository whose architecture comes from
                `.omnicoach.toml`, calls naming a component absent from `define_architecture!`,
                calls nested inside an inline item, duplicate top-level calls in the same module,
                and calls inside a module subtree whose ancestor already declares a component root."},
            why_is_this_bad: indoc::indoc! {r"
                An `architecture_component!` marker that has no effect silently leaves the module
                unmapped (or governed by an ancestor component) while looking to readers like an
                enforced architectural boundary."},
            known_problems: None,
            references: &[],
            examples: &[ProjectExample {
                language: Language::Rust,
                context: &[
                    (
                        "src/lib.rs",
                        indoc::indoc! {r"
                            mod architecture;
                            pub mod domain;
                            pub mod billing;
                        "},
                    ),
                    (
                        "src/architecture.rs",
                        indoc::indoc! {r"
                            define_architecture! {
                                Domain => [],
                            }
                        "},
                    ),
                    ("src/domain.rs", "architecture_component!(Domain);\n"),
                ],
                path: "src/billing.rs",
                flagged: indoc::indoc! {r"
                    architecture_component!(Billing);
                "},
                flagged_span: "architecture_component!(Billing);",
                fixed: indoc::indoc! {r"
                    architecture_component!(Domain);
                "},
            }],
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
    let source_text = file.source_text();
    findings
        .invalid_architecture_components
        .iter()
        .map(|finding| {
            let expression = source_text
                .get(finding.span.start..finding.span.end)
                .map_or("", str::trim);
            let trimmed_component = finding.component.trim();
            let component = if trimmed_component.is_empty() {
                COMPONENT_PLACEHOLDER
            } else {
                trimmed_component
            };
            let reason = format_reason(&finding.kind, component, &finding.module);
            rule.diagnostic_at_span(
                path,
                file,
                finding.span,
                &[
                    ("expression", expression),
                    ("module", &finding.module),
                    ("component", component),
                    ("reason", &reason),
                ],
            )
        })
        .collect()
}

fn format_reason(kind: &InvalidArchitectureComponentKind, component: &str, module: &str) -> String {
    match kind {
        InvalidArchitectureComponentKind::MissingDefineArchitecture => {
            "no `define_architecture!` declaration exists in `architecture`".to_owned()
        }
        InvalidArchitectureComponentKind::TomlArchitectureActive => {
            "`.omnicoach.toml` declares `[[architecture.components]]`, so Rust component macros are inactive"
                .to_owned()
        }
        InvalidArchitectureComponentKind::NotAtModuleRoot => {
            "`architecture_component!` appears inside an inner item instead of at the module root"
                .to_owned()
        }
        InvalidArchitectureComponentKind::DuplicateInModule { first_component } => {
            let trimmed_first = first_component.trim();
            if trimmed_first.is_empty() {
                format!("module `{module}` already declares a top-level `architecture_component!`")
            } else {
                format!(
                    "module `{module}` already declares `architecture_component!({trimmed_first})`"
                )
            }
        }
        InvalidArchitectureComponentKind::UnknownComponent => {
            format!("`{component}` is not declared in `define_architecture!`")
        }
        InvalidArchitectureComponentKind::NestedUnderComponent {
            ancestor_module,
            ancestor_component,
        } => format!(
            "ancestor module `{ancestor_module}` already declares component `{ancestor_component}`"
        ),
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Rust => {
            pass: [
                valid_component_root_with_architecture_rs => {
                    "src/lib.rs" => "
                        mod architecture;
                        pub mod domain;
                    ",
                    "src/architecture.rs" => "
                        define_architecture! {
                            Domain => [],
                        }
                    ",
                    "src/domain.rs" => "
                        architecture_component!(Domain);
                        pub struct Order;
                    ",
                },
                valid_component_root_with_architecture_mod_rs => {
                    "src/lib.rs" => "
                        mod architecture;
                        pub mod domain;
                    ",
                    "src/architecture/mod.rs" => "
                        define_architecture! {
                            Domain => [],
                        }
                    ",
                    "src/domain.rs" => "
                        architecture_component!(Domain);
                    ",
                },
                test_gated_component_macro_ignored => {
                    "src/lib.rs" => "
                        pub fn run() {}

                        #[cfg(test)]
                        mod fixtures {
                            architecture_component!(TestOnly);
                        }
                    ",
                },
            ],
            fail: [
                stray_without_define_architecture => {
                    "src/lib.rs" => "pub mod domain;",
                    "src/domain.rs" => "
                        architecture_component!(Domain);
                    ",
                } => ("src/domain.rs", "architecture_component!(Domain);"),
                define_architecture_outside_architecture_module => {
                    "src/lib.rs" => "
                        pub mod domain;

                        define_architecture! {
                            Domain => [],
                        }
                    ",
                    "src/domain.rs" => "
                        architecture_component!(Domain);
                    ",
                } => ("src/domain.rs", "architecture_component!(Domain);"),
                stray_when_toml_architecture_active => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Domain"
                        roots = ["domain"]
                    "#,
                    "src/lib.rs" => "pub mod domain;",
                    "src/domain.rs" => "
                        architecture_component!(Domain);
                    ",
                } => ("src/domain.rs", "architecture_component!(Domain);"),
                undeclared_component_name => {
                    "src/lib.rs" => "
                        mod architecture;
                        pub mod domain;
                        pub mod billing;
                    ",
                    "src/architecture.rs" => "
                        define_architecture! {
                            Domain => [],
                        }
                    ",
                    "src/domain.rs" => "architecture_component!(Domain);",
                    "src/billing.rs" => "architecture_component!(Billing);",
                } => ("src/billing.rs", "architecture_component!(Billing);"),
                nested_under_ancestor_component_root => {
                    "src/lib.rs" => "
                        mod architecture;
                        pub mod domain;
                    ",
                    "src/architecture.rs" => "
                        define_architecture! {
                            Domain => [],
                            Inner => [],
                        }
                    ",
                    "src/domain.rs" => "
                        architecture_component!(Domain);
                        pub mod inner;
                    ",
                    "src/domain/inner.rs" => "architecture_component!(Inner);",
                } => ("src/domain/inner.rs", "architecture_component!(Inner);"),
                component_macro_inside_inline_module => {
                    "src/lib.rs" => "
                        mod architecture;
                        pub mod domain;
                    ",
                    "src/architecture.rs" => "
                        define_architecture! {
                            Domain => [],
                        }
                    ",
                    "src/domain.rs" => "
                        mod inner {
                            architecture_component!(Domain);
                        }
                    ",
                } => ("src/domain.rs", "architecture_component!(Domain);"),
            ],
        },
    }
);
