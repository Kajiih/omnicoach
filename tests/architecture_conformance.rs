//! Architecture DAG conformance tests enforcing modular boundaries and component encapsulation.
//!
//! See `docs/dev/decisions/006_architectural_dag_and_conformance.md` for architectural design rationale.

// Workaround for rust-lang/rust-clippy#13981 so clippy.toml `allow-*-in-tests` applies to the whole file.
#![cfg(test)]

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use omnicoach::architecture::ArchitectureComponent;
use omnicoach::code_lint::project::{
    ArchitectureSpec, DiskFileSource, ProjectIndex, ProjectLayout,
    resolve_inherited_component_with_root,
};
use omnicoach::code_lint::rules::CODE_RULES;
use omnicoach::code_lint::runner::{LintOptions, run_code_lint};
use omnicoach::config::Config;
use omnicoach::diagnostic::RuleName;
use strum::VariantArray;

fn repo_root_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn source_root_directory() -> PathBuf {
    repo_root_directory().join("src")
}

fn repo_architecture_spec() -> &'static (ProjectLayout, ArchitectureSpec) {
    static REPO_SPEC: OnceLock<(ProjectLayout, ArchitectureSpec)> = OnceLock::new();
    REPO_SPEC.get_or_init(|| {
        let layout = ProjectLayout::discover(&source_root_directory(), &DiskFileSource, &[]);
        let index = ProjectIndex::new(layout.clone(), &DiskFileSource);
        let spec = index
            .architecture_spec()
            .expect("omnicoach should resolve an ArchitectureSpec from src/")
            .clone();
        (layout, spec)
    })
}

/// The `PascalCase` of a module path: `code_lint::ast` → `CodeLintAst`.
fn pascal_case(module_path: &str) -> String {
    module_path
        .split([':', '_'])
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut characters = segment.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + characters.as_str()
            })
        })
        .collect()
}

/// The module a component is named after: its root (or `bin` for `bin::*` entrypoints), or the
/// parent its roots share when it spans several files.
fn naming_module(roots: &[String]) -> Option<&str> {
    if let [root] = roots {
        return parent_module(root)
            .filter(|&parent| parent == "bin")
            .or(Some(root));
    }
    let shared = parent_module(roots.first()?)?;
    roots
        .iter()
        .all(|root| parent_module(root) == Some(shared))
        .then_some(shared)
}

fn parent_module(module_path: &str) -> Option<&str> {
    module_path.rsplit_once("::").map(|(parent, _)| parent)
}

#[test]
fn test_production_architecture_rules_on_src() {
    let architecture_rule_names: HashSet<RuleName> = [
        "undeclared-component-dependency",
        "sibling-module-dependency",
        "cross-component-relative-path",
        "unowned-external-dependency",
        "private-component-import",
        "reexported-public-item",
        "unmapped-architecture-module",
        "invalid-architecture-component",
    ]
    .into_iter()
    .map(RuleName)
    .collect();
    let disabled_rules: HashSet<RuleName> = CODE_RULES
        .iter()
        .map(|rule| rule.name())
        .filter(|name| !architecture_rule_names.contains(name))
        .collect();
    let config = Config {
        disabled_rules,
        ..Config::default()
    };
    let options = LintOptions {
        paths: vec![source_root_directory()],
        diff: false,
        diff_rev: None,
    };
    let report = run_code_lint(&options, &config).expect("run_code_lint should succeed on src/");
    assert!(
        report.diagnostics.is_empty(),
        "Production Topic::ARCHITECTURE rules reported diagnostics on src/:\n{:#?}",
        report.diagnostics
    );
}

/// Verifies that every variant in [`ArchitectureComponent::VARIANTS`] is declared by at least one
/// root module in `src/`.
#[test]
fn test_all_architecture_components_have_root_modules_in_src() {
    let (_, spec) = repo_architecture_spec();
    let uncovered_variants: Vec<ArchitectureComponent> = ArchitectureComponent::VARIANTS
        .iter()
        .copied()
        .filter(|variant| {
            !spec
                .components
                .iter()
                .any(|comp| comp.name == variant.as_ref() && !comp.roots.is_empty())
        })
        .collect();
    assert_eq!(uncovered_variants, Vec::<ArchitectureComponent>::new());
}

/// Verifies that every component name is the `PascalCase` of its root module path (or shared
/// parent module when it spans several files, such as `bin::*` → `Bin`).
#[test]
fn test_components_are_named_after_their_modules() {
    let (_, spec) = repo_architecture_spec();
    let naming_violations: Vec<String> = spec
        .components
        .iter()
        .filter(|comp| naming_module(&comp.roots).map(pascal_case).as_deref() != Some(&comp.name))
        .map(|comp| {
            format!(
                "`{}` is declared by `{}`; a component is the PascalCase of its module path",
                comp.name,
                comp.roots.join("`, `")
            )
        })
        .collect();
    assert!(
        naming_violations.is_empty(),
        "Components not named after their modules:\n{}",
        naming_violations.join("\n")
    );
}

#[test]
fn test_subtree_inheritance_resolves_leaf_modules_and_rejects_namespace_routers() {
    let declared_components = BTreeMap::from([
        (
            "domain::unit".to_owned(),
            ArchitectureComponent::CodeLintRules,
        ),
        ("other".to_owned(), ArchitectureComponent::CommandLintRules),
    ]);
    let queries = [
        "domain::unit",
        "domain::unit::leaf::deeper",
        "other::leaf",
        "domain",
        "domain::unit_sibling",
    ];
    let resolved: Vec<Option<ArchitectureComponent>> = queries
        .into_iter()
        .map(|module| {
            resolve_inherited_component_with_root(module, &declared_components)
                .map(|inherited| *inherited.component)
        })
        .collect();

    assert_eq!(
        resolved,
        vec![
            Some(ArchitectureComponent::CodeLintRules),
            Some(ArchitectureComponent::CodeLintRules),
            Some(ArchitectureComponent::CommandLintRules),
            None,
            None,
        ]
    );
}
