//! Flags production modules and non-routing items that do not belong to any declared architecture
//! component when `require-component-coverage = true`.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::contract::{ProjectCodeRule, RuleTarget};
use crate::code_lint::project::context::ProjectContext;
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, ImpactedQuality, Precision, ProjectExample, RuleDoc,
    RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Item `{name}` in `{module}` belongs to no declared architecture component.",
    rationale: "Code outside every declared component bypasses dependency conformance checks and obscures architectural ownership.",
    suggestion: {
        Python => "Add the package of `{module}` to `[[architecture.components]]` in `.omnicoach.toml`, or keep `{module}` as a pure package router.",
        Rust => "Add `architecture_component!(...)` to `{module}` (or an ancestor module), or keep `{module}` as a pure `mod` router.",
    },
};

/// The rule's declaration.
pub const RULE: ProjectCodeRule = ProjectCodeRule {
    declaration: Declaration {
        name: RuleName("unmapped-architecture-module"),
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
            summary: "Flags production modules and non-routing items that do not belong to any declared architecture component.",
            what_it_does: indoc::indoc! {r"
                When `require-component-coverage = true` in `[architecture]` (or when a Rust project
                declares its component graph via `define_architecture!`), every production source
                file must either belong to a declared component's module subtree or act as a pure
                namespace router containing only child `mod` declarations (or an empty `__init__.py`
                routing to declared child packages). When a component or submodule scope declares an
                explicit `submodules` / `modules: { ... }` DAG, every direct child submodule in that
                scope must also be listed in that DAG. Any unmapped file, undeclared submodule, or
                non-routing item inside a namespace router is flagged."},
            why_is_this_bad: indoc::indoc! {r"
                Architectural dependency rules only govern modules mapped to a component. New files
                or helper functions added outside every component root silently escape boundary
                checks and become unowned architectural drift."},
            known_problems: None,
            references: &[],
            examples: &[
                ProjectExample {
                    language: Language::Python,
                    context: &[
                        (
                            ".omnicoach.toml",
                            indoc::indoc! {r#"
                                [architecture]
                                require-component-coverage = true

                                [[architecture.components]]
                                name = "Domain"
                                roots = ["pkg.domain"]
                            "#},
                        ),
                        ("pkg/domain.py", "class Order: pass\n"),
                    ],
                    path: "pkg/__init__.py",
                    flagged: indoc::indoc! {r"
                        def orphan_helper() -> int: return 1
                    "},
                    flagged_span: "def orphan_helper() -> int: return 1",
                    fixed: indoc::indoc! {r#"
                        __all__ = ["domain"]
                    "#},
                },
                ProjectExample {
                    language: Language::Rust,
                    context: &[
                        (
                            ".omnicoach.toml",
                            indoc::indoc! {r#"
                                [architecture]
                                require-component-coverage = true

                                [[architecture.components]]
                                name = "Domain"
                                roots = ["domain"]
                            "#},
                        ),
                        ("src/domain.rs", "pub struct Order;\n"),
                    ],
                    path: "src/lib.rs",
                    flagged: indoc::indoc! {r"
                        pub mod domain;
                        pub fn stray_helper() {}
                    "},
                    flagged_span: "pub fn stray_helper() {}",
                    fixed: indoc::indoc! {r"
                        pub mod domain;
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
        .unmapped_architecture_modules
        .iter()
        .map(|finding| {
            rule.diagnostic_at_span(
                path,
                file,
                finding.span,
                &[("name", &finding.name), ("module", &finding.module)],
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
                mapped_component_module => {
                    ".omnicoach.toml" => r#"
                        [architecture]
                        require-component-coverage = true

                        [[architecture.components]]
                        name = "Domain"
                        roots = ["pkg.domain"]
                    "#,
                    "pkg/__init__.py" => "",
                    "pkg/domain.py" => "
                        def compute() -> int:
                            return 42
                    ",
                },
                coverage_disabled_does_not_require_component_coverage => {
                    ".omnicoach.toml" => r#"
                        [architecture]
                        require-component-coverage = false

                        [[architecture.components]]
                        name = "Domain"
                        roots = ["pkg.domain"]
                    "#,
                    "pkg/__init__.py" => "",
                    "pkg/app.py" => "
                        def run() -> int:
                            return 1
                    ",
                },
            ],
            fail: [
                unmapped_python_module_item => {
                    ".omnicoach.toml" => r#"
                        [architecture]
                        require-component-coverage = true

                        [[architecture.components]]
                        name = "Domain"
                        roots = ["pkg.domain"]
                    "#,
                    "pkg/__init__.py" => "",
                    "pkg/domain.py" => "class Order: pass",
                    "pkg/stray.py" => "
                        def orphan_helper() -> int: return 1
                    ",
                } => ("pkg/stray.py", "def orphan_helper() -> int: return 1"),
                undeclared_submodule_under_component_with_submodules => {
                    ".omnicoach.toml" => r#"
                        [architecture]
                        require-component-coverage = true

                        [[architecture.components]]
                        name = "Project"
                        roots = ["pkg.project"]
                        submodules = [
                            { name = "foundation", submodules = [
                                { name = "source" },
                            ] },
                        ]
                    "#,
                    "pkg/__init__.py" => "",
                    "pkg/project/__init__.py" => "",
                    "pkg/project/foundation/__init__.py" => "",
                    "pkg/project/foundation/source.py" => "class FileSource: pass",
                    "pkg/project/foundation/stray.py" => "
                        def unmapped_submodule() -> int: return 1
                    ",
                } => ("pkg/project/foundation/stray.py", "def unmapped_submodule() -> int: return 1"),
            ],
        },
        Rust => {
            pass: [
                pure_crate_root_router => {
                    ".omnicoach.toml" => r#"
                        [architecture]
                        require-component-coverage = true

                        [[architecture.components]]
                        name = "Domain"
                        roots = ["domain"]
                    "#,
                    "src/lib.rs" => "pub mod domain;",
                    "src/domain.rs" => "
                        pub struct Order;
                    ",
                },
                pure_architecture_definition_module_exempt => {
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
                router_with_test_gated_module => {
                    ".omnicoach.toml" => r#"
                        [architecture]
                        require-component-coverage = true

                        [[architecture.components]]
                        name = "Domain"
                        roots = ["domain"]
                    "#,
                    "src/domain.rs" => "pub struct Order;",
                    "src/lib.rs" => "
                        pub mod domain;

                        #[cfg(test)]
                        mod fixtures {
                            fn allowed() {}
                        }
                    ",
                },
            ],
            fail: [
                non_routing_item_in_unmapped_router => {
                    ".omnicoach.toml" => r#"
                        [architecture]
                        require-component-coverage = true

                        [[architecture.components]]
                        name = "Domain"
                        roots = ["domain"]
                    "#,
                    "src/domain.rs" => "pub struct Order;",
                    "src/lib.rs" => "
                        pub mod domain;
                        pub fn stray_helper() {}
                    ",
                } => ("src/lib.rs", "pub fn stray_helper() {}"),
                use_declaration_in_unmapped_router => {
                    ".omnicoach.toml" => r#"
                        [architecture]
                        require-component-coverage = true

                        [[architecture.components]]
                        name = "Domain"
                        roots = ["domain"]
                    "#,
                    "src/domain.rs" => "pub struct Order;",
                    "src/lib.rs" => "
                        pub mod domain;
                        use crate::domain::Order;
                    ",
                } => ("src/lib.rs", "use crate::domain::Order;"),
                router_routing_to_no_declared_component => {
                    ".omnicoach.toml" => r#"
                        [architecture]
                        require-component-coverage = true

                        [[architecture.components]]
                        name = "Domain"
                        roots = ["domain"]
                    "#,
                    "src/lib.rs" => "pub mod domain;",
                    "src/domain.rs" => "pub struct Order;",
                    "src/orphan.rs" => "pub mod nothing;",
                } => ("src/orphan.rs", "pub mod nothing;"),
                undeclared_mod_in_component_with_modules_dag => {
                    "src/lib.rs" => "
                        mod architecture;
                        pub mod project;
                    ",
                    "src/architecture.rs" => "
                        define_architecture! {
                            Project => [] (modules: {
                                foundation => [],
                            }),
                        }
                    ",
                    "src/project.rs" => "
                        architecture_component!(Project);
                        mod foundation;
                        mod undeclared_child;
                    ",
                    "src/project/foundation.rs" => "pub struct Config;",
                    "src/project/undeclared_child.rs" => "pub struct Secret;",
                } => ("src/project.rs", "mod undeclared_child;"),
                undeclared_nested_mod_in_submodule_with_modules_dag => {
                    "src/lib.rs" => "
                        mod architecture;
                        pub mod project;
                    ",
                    "src/architecture.rs" => "
                        define_architecture! {
                            Project => [] (modules: {
                                foundation => [] (modules: {
                                    source => [],
                                }),
                            }),
                        }
                    ",
                    "src/project.rs" => "
                        architecture_component!(Project);
                        mod foundation;
                    ",
                    "src/project/foundation.rs" => "
                        mod source;
                        mod undeclared_leaf;
                    ",
                    "src/project/foundation/source.rs" => "pub struct FileSource;",
                    "src/project/foundation/undeclared_leaf.rs" => "pub struct Secret;",
                } => ("src/project/foundation.rs", "mod undeclared_leaf;"),
            ],
        },
    }
);
