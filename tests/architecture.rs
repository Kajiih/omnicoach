//! Architecture DAG conformance tests enforcing modular boundaries and component encapsulation.
//!
//! See `decisions/006_architectural_dag_and_conformance.md` for architectural design rationale.
//!
//! `rust_arkitect`'s own engine recursively walks the whole crate directory (including
//! `scratch/` benchmark corpora), so these tests drive its rules over `src/` only.

#![allow(clippy::expect_used)]

use rust_arkitect::rule::Rule;
use rust_arkitect::rules::must_not_depend_on::MustNotDependOnRule;
use rust_arkitect::rules::utils::IsChild;
use rust_arkitect::rust_file::RustFile;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};

const CRATE_NAME: &str = "omni";

use omni::architecture::{ARCHITECTURE_GRAPH, ArchitectureComponent, ComponentDefinition};
use strum::VariantArray;

/// The only modules allowed to handle raw ast-grep types.
const AST_GREP_OWNERS: &[&str] = &["code_lint::ast", "command_lint::rule", "bin::ast_dumper"];

/// Returns the logical path `rust_arkitect` assigns to the crate-relative `module`.
fn logical_path(module: &str) -> String {
    format!("{CRATE_NAME}::{module}")
}

/// Returns every spelling under which `rust_arkitect` reports a dependency on `module`.
///
/// It resolves `crate::` to the crate name in `use` trees but keeps it verbatim in
/// expression and type paths, so both spellings must be forbidden.
fn dependency_spellings(module: &str) -> [String; 2] {
    [logical_path(module), format!("crate::{module}")]
}

fn must_not_depend_on<'a>(
    subject: &str,
    forbidden: impl IntoIterator<Item = &'a str>,
) -> MustNotDependOnRule {
    MustNotDependOnRule::new(
        logical_path(subject),
        forbidden
            .into_iter()
            .flat_map(dependency_spellings)
            .collect(),
    )
}

/// Restricts `inner` to files outside the `exempt` modules.
struct Except {
    inner: MustNotDependOnRule,
    exempt: Vec<String>,
}

impl fmt::Display for Except {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} (except in {})",
            self.inner,
            self.exempt.join(", ")
        )
    }
}

impl Rule for Except {
    fn apply(&self, file: &RustFile) -> Result<(), String> {
        self.inner.apply(file)
    }

    fn is_applicable(&self, file: &RustFile) -> bool {
        self.inner.is_applicable(file)
            && !self
                .exempt
                .iter()
                .any(|module| file.logical_path.is_child_of(module))
    }
}

/// Computes the set of all components transitively reachable from `from` along directed dependency edges.
fn compute_transitive_reachability<Component: Copy + Eq + std::hash::Hash + 'static>(
    from: Component,
    graph: &[ComponentDefinition<Component>],
) -> HashSet<Component> {
    let mut reachable = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(from);

    while let Some(current) = queue.pop_front() {
        if let Some(definition) = graph.iter().find(|item| item.component == current) {
            for &dependency in definition.depends_on {
                if reachable.insert(dependency) {
                    queue.push_back(dependency);
                }
            }
        }
    }
    reachable
}

/// Computes the crate-relative module path of a source file (e.g. `code_lint::rules::banned_abbreviations`).
fn module_path_for_file(path: &Path) -> String {
    let relative_path = path.strip_prefix(src_dir()).unwrap_or(path);
    let mut parts: Vec<String> = relative_path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect();
    if let Some(last) = parts.last_mut() {
        *last = last.strip_suffix(".rs").unwrap_or(last).to_string();
    }
    if parts.last().map(String::as_str) == Some("mod") {
        parts.pop();
    }
    parts.join("::").replace('-', "_")
}

/// Computes the logical path of a source file matching `rust_arkitect` conventions.
fn logical_path_for_file(path: &Path) -> String {
    logical_path(&module_path_for_file(path))
}

/// Returns true if `node` is outside all inline `#[cfg(test)]` / `#[test]` byte ranges.
fn is_in_production_code(
    node: &omni::code_lint::ast::AstNode<'_>,
    test_ranges: &[std::ops::Range<usize>],
) -> bool {
    let start = node.span().start;
    !test_ranges.iter().any(|range| range.contains(&start))
}

/// Extracts all `architecture_component!(<Variant>);` declarations from `source` using the Rust CST,
/// ignoring inline test blocks, comments, and string literals.
fn extract_architecture_components(source: &str) -> Vec<String> {
    let parsed = omni::code_lint::ast::ParsedFile::rust(source);
    let test_ranges = omni::code_lint::ast::rust::collect_inline_test_ranges(&parsed);
    omni::code_lint::ast::rust::collect_macro_invocations(&parsed)
        .into_iter()
        .filter(|node| {
            is_in_production_code(node, &test_ranges)
                && omni::code_lint::ast::rust::macro_terminal_name(node) == "architecture_component"
        })
        .map(|node| {
            omni::code_lint::ast::rust::extract_macro_arguments(&node)
                .into_iter()
                .map(|argument| argument.text().into_owned())
                .collect::<String>()
        })
        .collect()
}

/// Discovers the `ArchitectureComponent` declared via `architecture_component!(...)` for each module in `src/`.
fn discover_module_components() -> &'static BTreeMap<String, ArchitectureComponent> {
    static MODULE_COMPONENTS: std::sync::OnceLock<BTreeMap<String, ArchitectureComponent>> =
        std::sync::OnceLock::new();
    MODULE_COMPONENTS.get_or_init(|| {
        let mut map = BTreeMap::new();
        for path in rust_files(&src_dir()) {
            let source = std::fs::read_to_string(&path).expect("source files should be readable");
            if let [name] = extract_architecture_components(&source).as_slice()
                && let Ok(component) = name.parse::<ArchitectureComponent>()
            {
                map.insert(module_path_for_file(&path), component);
            }
        }
        map
    })
}

/// Computes the root modules for each `ArchitectureComponent`.
///
/// A module `M` assigned to component `C` is a root module of `C` if and only if
/// its parent module does not also belong to `C`.
fn discover_component_roots(
    module_components: &BTreeMap<String, ArchitectureComponent>,
) -> BTreeMap<ArchitectureComponent, Vec<String>> {
    let mut roots: BTreeMap<ArchitectureComponent, Vec<String>> = BTreeMap::new();
    for (module, &component) in module_components {
        let parent_component = module
            .rsplit_once("::")
            .and_then(|(parent, _)| module_components.get(parent))
            .copied();
        if parent_component != Some(component) {
            roots.entry(component).or_default().push(module.clone());
        }
    }
    roots
}

/// Constructs `rust_arkitect` verification rules derived directly from `ARCHITECTURE_GRAPH`
/// and the colocated `architecture_component!(...)` declarations in `src/`.
fn architecture_conformance_rules() -> Vec<Box<dyn Rule>> {
    let module_components = discover_module_components();
    let component_roots = discover_component_roots(module_components);
    let mut rules: Vec<Box<dyn Rule>> = Vec::new();

    // 1. Cross-component DAG dependency rules
    for definition in ARCHITECTURE_GRAPH {
        let mut allowed = compute_transitive_reachability(definition.component, ARCHITECTURE_GRAPH);
        allowed.insert(definition.component);

        let forbidden_modules: Vec<&str> = ARCHITECTURE_GRAPH
            .iter()
            .map(|item| item.component)
            .filter(|component| !allowed.contains(component))
            .filter_map(|component| component_roots.get(&component))
            .flatten()
            .map(String::as_str)
            .collect();

        if !forbidden_modules.is_empty()
            && let Some(subject_roots) = component_roots.get(&definition.component)
        {
            for subject_module in subject_roots {
                rules.push(Box::new(must_not_depend_on(
                    subject_module,
                    forbidden_modules.iter().copied(),
                )));
            }
        }
    }

    // 2. Intra-component leaf isolation for components with allow_internal_dependencies = false
    for definition in ARCHITECTURE_GRAPH {
        if definition.allow_internal_dependencies {
            continue;
        }

        let component_modules: Vec<&str> = module_components
            .iter()
            .filter_map(|(module, &component)| {
                (component == definition.component).then_some(module.as_str())
            })
            .collect();

        let leaf_modules: Vec<&str> = component_modules
            .iter()
            .copied()
            .filter(|&module| {
                !component_modules.iter().any(|other| {
                    other
                        .rsplit_once("::")
                        .is_some_and(|(parent, _)| parent == module)
                })
            })
            .collect();

        if leaf_modules.len() > 1 {
            for &leaf in &leaf_modules {
                let siblings = leaf_modules.iter().copied().filter(|&other| other != leaf);
                rules.push(Box::new(must_not_depend_on(leaf, siblings)));
            }
        }
    }

    rules
}

fn ast_grep_encapsulation_rules() -> Vec<Box<dyn Rule>> {
    vec![Box::new(Except {
        inner: MustNotDependOnRule::new(CRATE_NAME.to_owned(), vec!["ast_grep_core".to_owned()]),
        exempt: AST_GREP_OWNERS
            .iter()
            .map(|module| logical_path(module))
            .collect(),
    })]
}

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Recursively lists the `.rs` files under `dir`.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let entries = std::fs::read_dir(dir).expect("source directory should be readable");
    entries
        .map(|entry| entry.expect("directory entry should be readable").path())
        .flat_map(|path| {
            if path.is_dir() {
                rust_files(&path)
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                vec![path]
            } else {
                vec![]
            }
        })
        .collect()
}

fn violations_in(files: &[RustFile], rules: &[Box<dyn Rule>]) -> Vec<String> {
    files
        .iter()
        .flat_map(|file| {
            rules
                .iter()
                .filter(|rule| rule.is_applicable(file))
                .filter_map(|rule| {
                    rule.apply(file)
                        .err()
                        .map(|error| format!("{rule}: {error}"))
                })
        })
        .collect()
}

/// Strips inline test items (`#[cfg(test)]` modules/items and `#[test]` functions) using the
/// Rust CST so architectural rules evaluate production code only without truncating items that
/// appear after a `#[cfg(test)]` declaration.
fn strip_inline_tests(source: &str) -> String {
    let parsed = omni::code_lint::ast::ParsedFile::rust(source);
    let test_ranges = omni::code_lint::ast::rust::collect_inline_test_ranges(&parsed);
    let mut bytes = source.as_bytes().to_vec();
    for range in test_ranges {
        for byte in &mut bytes[range] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    }
    String::from_utf8(bytes).expect("blanking ASCII spaces preserves valid UTF-8")
}

fn assert_src_complies_with(rules: &[Box<dyn Rule>]) {
    let files: Vec<RustFile> = rust_files(&src_dir())
        .iter()
        .map(|path| {
            let source = std::fs::read_to_string(path).expect("source files should be readable");
            let production_source = strip_inline_tests(&source);
            RustFile::from_content(
                path.to_str().expect("source paths are UTF-8"),
                &logical_path_for_file(path),
                &production_source,
            )
        })
        .collect();
    let violations = violations_in(&files, rules);
    assert!(
        violations.is_empty(),
        "Architecture violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_all_source_files_declare_architecture_component() {
    let exempt_root_files: HashSet<PathBuf> = [
        src_dir().join("lib.rs"),
        src_dir().join("code_lint.rs"),
        src_dir().join("command_lint.rs"),
    ]
    .into_iter()
    .collect();

    let all_files = rust_files(&src_dir());
    let mut missing_declarations = Vec::new();
    let mut invalid_declarations = Vec::new();
    let mut module_components = BTreeMap::new();

    for path in all_files {
        if exempt_root_files.contains(&path) {
            continue;
        }

        let source = std::fs::read_to_string(&path).expect("source files should be readable");
        match extract_architecture_components(&source).as_slice() {
            [] => {
                missing_declarations.push(path.display().to_string());
            }
            [component_name] => match component_name.parse::<ArchitectureComponent>() {
                Err(_) => {
                    invalid_declarations.push(format!(
                        "{}: unknown component '{component_name}'",
                        path.display()
                    ));
                }
                Ok(component) => {
                    module_components.insert(module_path_for_file(&path), component);
                }
            },
            multiple => {
                invalid_declarations.push(format!(
                    "{}: multiple architecture_component! declarations found: {}",
                    path.display(),
                    multiple.join(", ")
                ));
            }
        }
    }

    let mut mismatched_submodules = Vec::new();
    for (module, &component) in &module_components {
        if let Some((parent, _)) = module.rsplit_once("::")
            && let Some(&parent_component) = module_components.get(parent)
            && component != parent_component
        {
            mismatched_submodules.push(format!(
                "{module}: declared as '{component}', but parent module '{parent}' belongs to '{parent_component}'"
            ));
        }
    }

    assert!(
        missing_declarations.is_empty(),
        "The following source files are missing 'architecture_component!(<Variant>);':\n{}",
        missing_declarations.join("\n")
    );

    assert!(
        invalid_declarations.is_empty(),
        "The following source files have invalid 'architecture_component!(...)' declarations:\n{}",
        invalid_declarations.join("\n")
    );

    assert!(
        mismatched_submodules.is_empty(),
        "The following submodules conflict with their parent module's component:\n{}",
        mismatched_submodules.join("\n")
    );

    let component_roots = discover_component_roots(&module_components);
    for component in ArchitectureComponent::VARIANTS {
        assert!(
            component_roots
                .get(component)
                .is_some_and(|roots| !roots.is_empty()),
            "Component '{component}' has no source files declaring it in src/"
        );
    }
}

#[test]
fn test_architecture_conformance() {
    assert_src_complies_with(&architecture_conformance_rules());
}

#[test]
fn test_ast_grep_is_encapsulated() {
    assert_src_complies_with(&ast_grep_encapsulation_rules());
}

/// Returns the CST declarations in `source` that give an item a second path.
///
/// Every item keeps one canonical path: a visible `use` (re-export) would hide the defining
/// component from the dependency rules above. The one exception is a `macro_rules!` macro
/// declaring its own path (`pub(crate) use name;`) next to its definition, because such macros
/// cannot carry a visibility modifier. `#[macro_export]` is banned in non-root modules because
/// it moves a macro to the crate root.
fn second_path_declarations(source: &str) -> Vec<String> {
    let parsed = omni::code_lint::ast::ParsedFile::rust(source);
    let test_ranges = omni::code_lint::ast::rust::collect_inline_test_ranges(&parsed);
    omni::code_lint::ast::rust::collect_second_path_declarations(&parsed)
        .into_iter()
        .filter(|node| is_in_production_code(node, &test_ranges))
        .map(|node| node.text().trim().to_string())
        .collect()
}

#[test]
fn test_items_have_a_single_path() {
    let crate_root = src_dir().join("lib.rs");
    let violations: Vec<String> = rust_files(&src_dir())
        .iter()
        .flat_map(|path| {
            let source = std::fs::read_to_string(path).expect("source files should be readable");
            second_path_declarations(&source)
                .into_iter()
                .filter(|declaration| !(path == &crate_root && declaration == "#[macro_export]"))
                .map(|declaration| format!("{}: {declaration}", path.display()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        violations.is_empty(),
        "Second paths found:\n{}",
        violations.join("\n")
    );
}

/// Returns `(line_number, declaration_text)` for all relative `use` declarations (`use super::...`)
/// in the production code of `source`.
fn relative_import_declarations(source: &str) -> Vec<(usize, String)> {
    let parsed = omni::code_lint::ast::ParsedFile::rust(source);
    let test_ranges = omni::code_lint::ast::rust::collect_inline_test_ranges(&parsed);
    omni::code_lint::ast::rust::collect_relative_use_declarations(&parsed)
        .into_iter()
        .filter(|node| is_in_production_code(node, &test_ranges))
        .map(|node| (node.start_line(), node.text().trim().to_string()))
        .collect()
}

/// Enforces that relative imports (`use super::...`) are never used in production code,
/// maintaining unambiguous `crate::` canonical paths across the crate.
#[test]
fn test_no_relative_imports_in_production_code() {
    let violations: Vec<String> = rust_files(&src_dir())
        .iter()
        .flat_map(|path| {
            let source = std::fs::read_to_string(path).expect("source files should be readable");
            relative_import_declarations(&source)
                .into_iter()
                .map(|(line, declaration)| format!("{}:{line}: {declaration}", path.display()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        violations.is_empty(),
        "Relative imports found in production code:\n{}",
        violations.join("\n")
    );
}

/// Guards against vacuous passes: re-exports and exported macros are caught, while a macro
/// declaring its own path or string literals/comments are not.
#[test]
fn test_second_path_declarations_are_detected() {
    let source = indoc::indoc! {r#"
        macro_rules! local {
            () => {};
        }
        pub(crate) use local;
        // pub use crate::commented::Out;
        const FIXTURE: &str = r"
        pub use crate::fake::ReExport;
        #[macro_export]
        ";
        pub use crate::diagnostic::RuleName;
        pub(crate) use crate::core::Tag;
        #[macro_export]
        macro_rules! exported {
            () => {};
        }
    "#};
    assert_eq!(
        second_path_declarations(source),
        vec![
            "pub use crate::diagnostic::RuleName;",
            "pub(crate) use crate::core::Tag;",
            "#[macro_export]",
        ]
    );
}

/// Guards against vacuous passes and false positives in `extract_architecture_components`
/// and `relative_import_declarations`.
#[test]
fn test_structural_extractors_ignore_strings_and_catch_after_conditional_test() {
    let source = indoc::indoc! {r#"
        // architecture_component!(FakeComment);
        architecture_component!(
            CodeLintRules
        );
        #[cfg(test)]
        fn helper() {
            use super::allowed_in_test;
        }
        const FIXTURE: &str = r"
        architecture_component!(FakeString);
        use super::fake_string_import;
        ";
        architecture_component!(CoreVocabulary);
        use super::real_relative_import;
    "#};

    assert_eq!(
        extract_architecture_components(source),
        vec!["CodeLintRules", "CoreVocabulary"]
    );
    assert_eq!(
        relative_import_declarations(source),
        vec![(14, "use super::real_relative_import;".to_string())]
    );
}

/// Guards against vacuous passes: upward dependencies, cross-domain dependencies, and
/// sibling-leaf imports within `no_internal_dependencies` components must all be caught.
#[test]
fn test_architecture_rules_detect_forbidden_dependencies() {
    let rules = architecture_conformance_rules();

    let upward_dependency = RustFile::from_content(
        "src/code_lint/rules/offending.rs",
        &logical_path("code_lint::rules::offending"),
        "use crate::code_lint::runner::lint_file;\nfn f() { crate::code_lint::runner::lint_file(); }\n",
    );
    let upward_violations = violations_in(&[upward_dependency], &rules);
    assert!(
        upward_violations
            .iter()
            .any(|violation| violation.contains("code_lint::runner")),
        "Expected upward dependency violation, got: {upward_violations:?}"
    );

    let cross_domain = RustFile::from_content(
        "src/command_lint/rules/jj.rs",
        &logical_path("command_lint::rules::jj"),
        "use crate::code_lint::ast::ParsedFile;\nfn f() { crate::code_lint::ast::ParsedFile; }\n",
    );
    let cross_domain_violations = violations_in(&[cross_domain], &rules);
    assert!(
        cross_domain_violations
            .iter()
            .any(|violation| violation.contains("code_lint::ast")),
        "Expected cross-domain dependency violation, got: {cross_domain_violations:?}"
    );

    let sibling_leaf = RustFile::from_content(
        "src/code_lint/bindings.rs",
        &logical_path("code_lint::bindings"),
        "use crate::code_lint::calls;\n",
    );
    let sibling_violations = violations_in(&[sibling_leaf], &rules);
    assert!(
        sibling_violations
            .iter()
            .any(|violation| violation.contains("code_lint::calls")),
        "Expected sibling-leaf isolation violation, got: {sibling_violations:?}"
    );
}

/// Guards against mid-file `#[cfg(test)]` truncation: a `#[cfg(test)]` helper item (or raw string
/// containing `#[cfg(test)]`) must not blind the architecture checker to forbidden dependencies
/// in production items that appear later in the same file.
#[test]
fn test_strip_inline_tests_preserves_production_code_after_conditional_test_item() {
    let source_with_mid_file_conditional_test = indoc::indoc! {r#"
        #[cfg(test)]
        fn test_helper() {
            let _ = crate::code_lint::runner::lint_file;
        }

        const RAW_FIXTURE: &str = r"
        #[cfg(test)]
        mod fake_tests {}
        ";

        pub fn production_fn() {
            crate::code_lint::runner::lint_file();
        }
    "#};

    let stripped = strip_inline_tests(source_with_mid_file_conditional_test);
    let file = RustFile::from_content(
        "src/code_lint/rules/offending.rs",
        &logical_path("code_lint::rules::offending"),
        &stripped,
    );
    let violations = violations_in(&[file], &architecture_conformance_rules());
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("code_lint::runner")),
        "Expected forbidden dependency after #[cfg(test)] item to be detected, got: {violations:?}"
    );
}
