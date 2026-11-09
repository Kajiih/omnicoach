//! Flags imports from a dependency component that bypass its declared `public-exports` allowlist.

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
    summary: "Import `{expression}` reaches into private internals of component `{dependency}`.",
    rationale: "Importing internal submodules bypasses the public facade of a component and couples callers to its internal file layout.",
    suggestion: "Import `{name}` from `{module}`, or expose it in `public-exports` of `{dependency}`.",
};

/// The rule's declaration.
pub const RULE: ProjectCodeRule = ProjectCodeRule {
    declaration: Declaration {
        name: RuleName("private-component-import"),
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
            summary: "Flags imports from an allowed dependency component that bypass its `public-exports` interface.",
            what_it_does: indoc::indoc! {r"
                When an architecture component declares `public-exports = [...]` in
                `.omnicoach.toml`, callers in other components may only import symbols matching that
                allowlist from the component's root module. Any import reaching directly into an
                internal submodule or importing an unexported root symbol is flagged."},
            why_is_this_bad: indoc::indoc! {r"
                Even when component `A` is allowed to depend on component `B`, reaching directly
                into `B`'s private submodules couples `A` to `B`'s internal decomposition and
                prevents refactoring `B` behind a stable facade."},
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
                                name = "Auth"
                                roots = ["pkg.auth"]
                                public-exports = ["verify_token"]

                                [[architecture.components]]
                                name = "Api"
                                roots = ["pkg.api"]
                                depends-on = ["Auth"]
                            "#},
                        ),
                        ("pkg/__init__.py", ""),
                        (
                            "pkg/auth/__init__.py",
                            "from pkg.auth.jwt import verify_token\n",
                        ),
                        (
                            "pkg/auth/jwt.py",
                            "def verify_token() -> bool: return True\n",
                        ),
                    ],
                    path: "pkg/api.py",
                    flagged: indoc::indoc! {r"
                        from pkg.auth.jwt import verify_token
                    "},
                    flagged_span: "from pkg.auth.jwt import verify_token",
                    fixed: indoc::indoc! {r"
                        from pkg.auth import verify_token
                    "},
                },
                ProjectExample {
                    language: Language::Rust,
                    context: &[
                        (
                            ".omnicoach.toml",
                            indoc::indoc! {r#"
                                [[architecture.components]]
                                name = "Auth"
                                roots = ["auth"]
                                public-exports = ["verify_token"]

                                [[architecture.components]]
                                name = "Api"
                                roots = ["api"]
                                depends-on = ["Auth"]
                            "#},
                        ),
                        ("src/lib.rs", "pub mod auth;\npub mod api;\n"),
                        (
                            "src/auth.rs",
                            "pub mod jwt;\npub use self::jwt::verify_token;\n",
                        ),
                        (
                            "src/auth/jwt.rs",
                            "pub fn verify_token() -> bool { true }\n",
                        ),
                    ],
                    path: "src/api.rs",
                    flagged: indoc::indoc! {r"
                        use crate::auth::jwt::verify_token;
                    "},
                    flagged_span: "use crate::auth::jwt::verify_token;",
                    fixed: indoc::indoc! {r"
                        use crate::auth::verify_token;
                    "},
                },
            ],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

const EXPORTED_ITEM_PLACEHOLDER: &str = "<exported_item>";

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
        .private_component_imports
        .iter()
        .map(|finding| {
            let expression = file.span_text(finding.span);
            let name = finding.name.as_deref().unwrap_or(EXPORTED_ITEM_PLACEHOLDER);
            rule.diagnostic_at_span(
                path,
                file,
                finding.span,
                &[
                    ("expression", expression),
                    ("dependency", &finding.dependency),
                    ("name", name),
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
        Python => {
            pass: [
                imports_declared_public_export_from_root => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Auth"
                        roots = ["pkg.auth"]
                        public-exports = ["verify_token"]

                        [[architecture.components]]
                        name = "Api"
                        roots = ["pkg.api"]
                        depends-on = ["Auth"]
                    "#,
                    "pkg/__init__.py" => "",
                    "pkg/auth/__init__.py" => "def verify_token() -> bool: return True",
                    "pkg/api.py" => "
                        from pkg.auth import verify_token
                    ",
                },
            ],
            fail: [
                imports_internal_submodule_directly => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Auth"
                        roots = ["pkg.auth"]
                        public-exports = ["verify_token"]

                        [[architecture.components]]
                        name = "Api"
                        roots = ["pkg.api"]
                        depends-on = ["Auth"]
                    "#,
                    "pkg/__init__.py" => "",
                    "pkg/auth/__init__.py" => "",
                    "pkg/auth/jwt.py" => "def decode_raw() -> None: pass",
                    "pkg/api.py" => "
                        from pkg.auth.jwt import decode_raw
                    ",
                } => ("pkg/api.py", "from pkg.auth.jwt import decode_raw"),
                wildcard_import_bypasses_public_exports => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Auth"
                        roots = ["pkg.auth"]
                        public-exports = ["verify_token"]

                        [[architecture.components]]
                        name = "Api"
                        roots = ["pkg.api"]
                        depends-on = ["Auth"]
                    "#,
                    "pkg/__init__.py" => "",
                    "pkg/auth/__init__.py" => "def verify_token() -> bool: return True",
                    "pkg/api.py" => "
                        from pkg.auth import *
                    ",
                } => ("pkg/api.py", "from pkg.auth import *"),
                bare_root_module_import_bypasses_public_exports => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Auth"
                        roots = ["pkg.auth"]
                        public-exports = ["verify_token"]

                        [[architecture.components]]
                        name = "Api"
                        roots = ["pkg.api"]
                        depends-on = ["Auth"]
                    "#,
                    "pkg/__init__.py" => "",
                    "pkg/auth/__init__.py" => "def verify_token() -> bool: return True",
                    "pkg/api.py" => "
                        import pkg.auth
                    ",
                } => ("pkg/api.py", "import pkg.auth"),
            ],
        },
        Rust => {
            pass: [
                uses_allowed_root_export => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Auth"
                        roots = ["auth"]
                        public-exports = ["verify_token"]

                        [[architecture.components]]
                        name = "Api"
                        roots = ["api"]
                        depends-on = ["Auth"]
                    "#,
                    "src/lib.rs" => "pub mod auth;\npub mod api;",
                    "src/auth.rs" => "pub fn verify_token() {}",
                    "src/api.rs" => "
                        use crate::auth::verify_token;
                    ",
                },
            ],
            fail: [
                reaches_into_private_submodule => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Auth"
                        roots = ["auth"]
                        public-exports = ["verify_token"]

                        [[architecture.components]]
                        name = "Api"
                        roots = ["api"]
                        depends-on = ["Auth"]
                    "#,
                    "src/lib.rs" => "pub mod auth;\npub mod api;",
                    "src/auth.rs" => "pub mod jwt;\npub fn verify_token() {}",
                    "src/auth/jwt.rs" => "pub fn decode_raw() {}",
                    "src/api.rs" => "
                        use crate::auth::jwt::decode_raw;
                    ",
                } => ("src/api.rs", "use crate::auth::jwt::decode_raw;"),
                wildcard_import_bypasses_public_exports => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Auth"
                        roots = ["auth"]
                        public-exports = ["verify_token"]

                        [[architecture.components]]
                        name = "Api"
                        roots = ["api"]
                        depends-on = ["Auth"]
                    "#,
                    "src/lib.rs" => "pub mod auth;\npub mod api;",
                    "src/auth.rs" => "pub fn verify_token() {}",
                    "src/api.rs" => "
                        use crate::auth::*;
                    ",
                } => ("src/api.rs", "use crate::auth::*;"),
                bare_root_module_import_bypasses_public_exports => {
                    ".omnicoach.toml" => r#"
                        [[architecture.components]]
                        name = "Auth"
                        roots = ["auth"]
                        public-exports = ["verify_token"]

                        [[architecture.components]]
                        name = "Api"
                        roots = ["api"]
                        depends-on = ["Auth"]
                    "#,
                    "src/lib.rs" => "pub mod auth;\npub mod api;",
                    "src/auth.rs" => "pub fn verify_token() {}",
                    "src/api.rs" => "
                        use crate::auth;
                    ",
                } => ("src/api.rs", "use crate::auth;"),
            ],
        },
    }
);
