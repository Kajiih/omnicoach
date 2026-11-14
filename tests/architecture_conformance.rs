//! Architecture DAG conformance tests enforcing modular boundaries and component encapsulation.
//!
//! See `docs/dev/decisions/006_architectural_dag_and_conformance.md` for architectural design rationale.

// Workaround for rust-lang/rust-clippy#13981 so clippy.toml `allow-*-in-tests` applies to the whole file.
#![cfg(test)]

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use omnicoach::architecture::ArchitectureComponent;
use omnicoach::code_lint::ast::rust::{collect_inline_test_ranges, summarize_rust_file};
use omnicoach::code_lint::ast::{ParsedFile, collect_bindings};
use omnicoach::code_lint::project::context::{ProjectIndex, ProjectLayout};
use omnicoach::code_lint::project::foundation::{
    ArchitectureSpec, DiskFileSource, FileSource as _,
};
use omnicoach::code_lint::rules::CODE_RULES;
use omnicoach::code_lint::runner::{LintOptions, run_code_lint};
use omnicoach::config::Config;
use omnicoach::diagnostic::{Language, RuleName};
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

const ALLOWED_CONTEXT_DELEGATION_BINDINGS: &[&str] = &[
    "python",
    "rust",
    "Python",
    "Rust",
    "PythonLayout",
    "RustLayout",
    "PythonIndex",
    "RustIndex",
    "PythonModuleContext",
    "collect_python_project_facts",
    "collect_rust_project_facts",
];

struct ProjectLanguageViolations {
    cross_language_ast_paths: Vec<String>,
    foundation_language_bindings: Vec<String>,
    context_language_bindings: Vec<String>,
    sibling_language_bindings: Vec<String>,
}

fn collect_project_language_violations() -> ProjectLanguageViolations {
    let code_lint_root = source_root_directory().join("code_lint");
    let mut paths = DiskFileSource
        .walk_files(&code_lint_root.join("project"))
        .expect("walk src/code_lint/project");
    paths.push(code_lint_root.join("project.rs"));
    paths.sort();

    let mut violations = ProjectLanguageViolations {
        cross_language_ast_paths: Vec::new(),
        foundation_language_bindings: Vec::new(),
        context_language_bindings: Vec::new(),
        sibling_language_bindings: Vec::new(),
    };

    for path in paths {
        let content = DiskFileSource
            .read_to_string(&path)
            .expect("read project source file");
        let parsed = ParsedFile::new(&content, Language::Rust);
        let summary = summarize_rust_file(&parsed);
        let inline_test_ranges = collect_inline_test_ranges(&parsed);
        let relative = path
            .strip_prefix(&code_lint_root)
            .expect("project file under src/code_lint")
            .to_string_lossy()
            .replace('\\', "/");

        let is_foundation_or_router =
            relative == "project.rs" || relative.starts_with("project/foundation");
        let is_context = relative.starts_with("project/context");
        if is_foundation_or_router || is_context {
            for reference in &summary.referenced_paths {
                if reference.raw_path.contains("ast::python")
                    || reference.raw_path.contains("ast::rust")
                {
                    violations.cross_language_ast_paths.push(format!(
                        "{relative}:{} -> {}",
                        reference.line, reference.raw_path
                    ));
                }
            }
        }

        for binding in collect_bindings(&parsed) {
            let node = binding.node();
            let start = node.span().start;
            if inline_test_ranges
                .iter()
                .any(|range| range.contains(&start))
            {
                continue;
            }
            let name = node.text();
            let lower = name.to_ascii_lowercase();
            let entry = format!("{relative}: {name}");
            if is_foundation_or_router {
                if ["python", "rust", "cargo", "pyproject"]
                    .iter()
                    .any(|term| lower.contains(term))
                {
                    violations.foundation_language_bindings.push(entry);
                }
            } else if is_context {
                let has_manifest_term = lower.contains("cargo") || lower.contains("pyproject");
                let has_unapproved_language_term = (lower.contains("python")
                    || lower.contains("rust"))
                    && !ALLOWED_CONTEXT_DELEGATION_BINDINGS.contains(&name.as_ref());
                if has_manifest_term || has_unapproved_language_term {
                    violations.context_language_bindings.push(entry);
                }
            } else if relative.starts_with("project/python") {
                if lower.contains("rust") || lower.contains("cargo") {
                    violations.sibling_language_bindings.push(entry);
                }
            } else if relative.starts_with("project/rust")
                && (lower.contains("python") || lower.contains("pyproject"))
            {
                violations.sibling_language_bindings.push(entry);
            }
        }
    }

    violations
}

/// Verifies that cross-language modules (`project.rs`, `foundation`, `context`) never reference
/// `code_lint::ast::{python,rust}`, `foundation` declares zero language- or manifest-specific
/// bindings beyond `FileSource`'s `is_cargo_target_dir` build-directory filter, `context` declares
/// only its explicit `python`/`rust` delegation bindings, and `python` / `rust` declare zero
/// bindings naming each other's language or manifest.
#[test]
fn test_code_lint_project_enforces_language_isolation_across_ast_and_bindings() {
    let violations = collect_project_language_violations();

    assert_eq!(violations.cross_language_ast_paths, Vec::<String>::new());
    assert_eq!(
        violations.foundation_language_bindings,
        vec!["project/foundation/source.rs: is_cargo_target_dir".to_owned()]
    );
    assert_eq!(violations.context_language_bindings, Vec::<String>::new());
    assert_eq!(violations.sibling_language_bindings, Vec::<String>::new());
}
