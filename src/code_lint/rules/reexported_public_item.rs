//! Flags `pub use` re-exports of items outside a private child module and `#[macro_export]`
//! outside the crate root.

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
    summary: "Item `{expression}` in `{module}` creates a second public path outside its defining module.",
    rationale: "Multiple public paths to the same item split caller imports across aliases and obscure which module owns the symbol.",
    suggestion: "Narrow the source module to `mod ...;` so `{expression}` is the sole public path, or remove `{expression}`.",
};

/// The rule's declaration.
pub const RULE: ProjectCodeRule = ProjectCodeRule {
    declaration: Declaration {
        name: RuleName("reexported-public-item"),
        template: &TEMPLATE,
        languages: &[Language::Rust],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::ARCHITECTURE],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags `pub use` re-exports of items outside a private child module and `#[macro_export]` outside the crate root.",
            what_it_does: indoc::indoc! {r"
                Checks visible `use` declarations (`pub use`, `pub(crate) use`) and `#[macro_export]`
                attributes in Rust modules. A re-export is allowed only when it exposes items from a
                private direct child module (`mod child;` or `mod child { ... }`, then
                `pub use self::child::Item;`), acting as a single-path component facade.
                Re-exporting from a `pub mod` child or from another module creates a duplicate path
                and is flagged, as is `#[macro_export]` outside the crate root, which exports the
                macro at the crate root."},
            why_is_this_bad: indoc::indoc! {r"
                When an item is reachable through both `crate::a::b::Item` and `crate::a::Item`,
                imports across the codebase splinter between the two paths and module ownership
                becomes ambiguous. Making child implementation modules private (`mod b;`) when
                re-exporting at the parent keeps a single canonical import path."},
            known_problems: None,
            references: &[],
            examples: &[ProjectExample {
                language: Language::Rust,
                context: &[
                    ("src/lib.rs", "pub mod domain;\n"),
                    ("src/domain/order.rs", "pub struct Order;\n"),
                ],
                path: "src/domain.rs",
                flagged: indoc::indoc! {r"
                    pub mod order;
                    pub use self::order::Order;
                "},
                flagged_span: "pub use self::order::Order;",
                fixed: indoc::indoc! {r"
                    mod order;
                    pub use self::order::Order;
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
    findings
        .reexported_public_items
        .iter()
        .map(|finding| {
            rule.diagnostic_at_span(
                path,
                file,
                finding.span,
                &[
                    ("expression", &finding.expression),
                    ("module", &finding.module),
                ],
            )
        })
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Rust => {
            pass: [
                private_child_facade_reexport => {
                    "src/lib.rs" => "pub mod domain;",
                    "src/domain.rs" => "
                        mod order;
                        pub use self::order::Order;
                    ",
                },
                grouped_private_child_facade_reexport => {
                    "src/lib.rs" => "pub mod domain;",
                    "src/domain.rs" => "
                        mod order;
                        pub use self::order::{Order, OrderId};
                    ",
                },
                inline_private_child_facade_reexport => {
                    "src/lib.rs" => "
                        pub use abi::prepare_input;

                        mod abi {
                            pub fn prepare_input() {}
                        }
                    ",
                },
                crate_visible_facade_reexport_without_self_prefix => {
                    "src/lib.rs" => "pub mod domain;",
                    "src/domain.rs" => "
                        mod order;
                        pub(crate) use order::Order;
                    ",
                },
                local_macro_reexport => {
                    "src/lib.rs" => "pub mod domain;",
                    "src/domain.rs" => "
                        macro_rules! local_macro {
                            () => {};
                        }
                        pub(crate) use local_macro;
                    ",
                },
                reexport_text_in_comment => {
                    "src/lib.rs" => "pub mod domain;",
                    "src/domain.rs" => "
                        // pub use crate::billing::Invoice;
                        pub struct Order;
                    ",
                },
                reexport_text_in_string_literal => {
                    "src/lib.rs" => "pub mod domain;",
                    "src/domain.rs" => r#"
                        pub const FIXTURE: &str = "pub use crate::billing::Invoice;";
                    "#,
                },
                macro_export_at_crate_root => {
                    "src/lib.rs" => "
                        #[macro_export]
                        macro_rules! exported {
                            () => {};
                        }
                    ",
                },
            ],
            fail: [
                reexports_from_public_child_module => {
                    "src/lib.rs" => "pub mod domain;",
                    "src/domain/order.rs" => "pub struct Order;",
                    "src/domain.rs" => "
                        pub mod order;
                        pub use self::order::Order;
                    ",
                } => ("src/domain.rs", "pub use self::order::Order;"),
                reexports_from_another_module => {
                    "src/lib.rs" => "
                        pub mod billing;
                        pub mod domain;
                    ",
                    "src/domain.rs" => "pub use crate::billing::Invoice;",
                } => ("src/domain.rs", "pub use crate::billing::Invoice;"),
                crate_visible_reexport_from_another_module => {
                    "src/lib.rs" => "
                        pub mod billing;
                        pub mod domain;
                    ",
                    "src/domain.rs" => "pub(crate) use crate::billing::Invoice;",
                } => ("src/domain.rs", "pub(crate) use crate::billing::Invoice;"),
                macro_export_outside_crate_root => {
                    "src/lib.rs" => "pub mod domain;",
                    "src/domain.rs" => "
                        #[macro_export]
                        macro_rules! exported {
                            () => {};
                        }
                    ",
                } => ("src/domain.rs", "#[macro_export]"),
            ],
        },
    }
);
