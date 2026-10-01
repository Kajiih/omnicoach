//! Architecture DAG conformance tests enforcing modular boundaries and component encapsulation.
//!
//! See `decisions/006_architectural_dag_and_conformance.md` for architectural design rationale.

// Workaround for rust-lang/rust-clippy#13981 so clippy.toml `allow-*-in-tests` applies to the whole file.
#![cfg(test)]

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use omni::architecture::{ARCHITECTURE_GRAPH, ArchitectureComponent, ComponentDefinition};
use omni::code_lint::ast::ParsedFile;
use omni::code_lint::ast::rust::{
    ExternalModDeclaration, RustFileSummary, VisibleUseDeclaration, summarize_rust_file,
};
use strum::VariantArray;

/// The only modules allowed to handle raw ast-grep types.
const AST_GREP_OWNERS: &[&str] = &["code_lint::ast", "command_lint::rule", "bin::ast_dumper"];

/// Cached structural and dependency summary of a single `.rs` file under `src/`.
#[derive(Debug, Clone)]
struct SourceFileEntry {
    path: PathBuf,
    module_path: String,
    is_crate_root: bool,
    summary: RustFileSummary,
}

impl SourceFileEntry {
    fn from_source(path: impl Into<PathBuf>, module_path: impl Into<String>, source: &str) -> Self {
        let path = path.into();
        let module_path = module_path.into();
        let is_crate_root = module_path == "lib" || module_path.is_empty();
        let parsed = ParsedFile::rust(source);
        let summary = summarize_rust_file(&parsed);
        Self {
            path,
            module_path,
            is_crate_root,
            summary,
        }
    }
}

/// How a syntactic path in a Rust file was anchored before normalization.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PathOrigin {
    /// `self` / `super` relative path, recording the shallowest ancestor module reached
    /// while resolving `super` segments (`None` if `super` climbed past the crate root).
    Relative { shallowest_ancestor: Option<String> },
    /// Crate-absolute (`crate::`, `omni::`), local child module, or external/local path.
    NonRelative,
}

/// A referenced path normalized to a canonical crate-relative (or external crate) path.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedPath {
    origin: PathOrigin,
    canonical_path: String,
}

/// Normalizes `raw_path` relative to `enclosing_module` and its declared `external_mods`.
fn resolve_reference_path(
    enclosing_module: &str,
    external_mods: &[ExternalModDeclaration],
    raw_path: &str,
) -> ResolvedPath {
    let effective_enclosing = if enclosing_module == "lib" {
        ""
    } else {
        enclosing_module
    };

    if raw_path == "crate" || raw_path == "omni" {
        return ResolvedPath {
            origin: PathOrigin::NonRelative,
            canonical_path: String::new(),
        };
    }
    if let Some(rest) = raw_path
        .strip_prefix("crate::")
        .or_else(|| raw_path.strip_prefix("omni::"))
    {
        return ResolvedPath {
            origin: PathOrigin::NonRelative,
            canonical_path: rest.to_string(),
        };
    }

    let first_segment = raw_path.split("::").next().unwrap_or(raw_path);
    if matches!(first_segment, "self" | "super") {
        let mut module_parts: Vec<&str> = if effective_enclosing.is_empty() {
            Vec::new()
        } else {
            effective_enclosing.split("::").collect()
        };
        let mut shallowest_depth = module_parts.len();
        let mut underflow = false;
        for segment in raw_path.split("::") {
            match segment {
                "self" => {}
                "super" => {
                    if module_parts.pop().is_some() {
                        shallowest_depth = shallowest_depth.min(module_parts.len());
                    } else {
                        underflow = true;
                    }
                }
                other => module_parts.push(other),
            }
        }
        let shallowest_ancestor = (!underflow).then(|| module_parts[..shallowest_depth].join("::"));
        return ResolvedPath {
            origin: PathOrigin::Relative {
                shallowest_ancestor,
            },
            canonical_path: module_parts.join("::"),
        };
    }

    if external_mods
        .iter()
        .any(|declaration| declaration.name == first_segment)
    {
        let canonical_path = if effective_enclosing.is_empty() {
            raw_path.to_string()
        } else {
            format!("{effective_enclosing}::{raw_path}")
        };
        return ResolvedPath {
            origin: PathOrigin::NonRelative,
            canonical_path,
        };
    }

    ResolvedPath {
        origin: PathOrigin::NonRelative,
        canonical_path: raw_path.to_string(),
    }
}

/// Returns true if `path` equals `module_prefix` or is a descendant (`{module_prefix}::...`).
fn is_in_module_subtree(path: &str, module_prefix: &str) -> bool {
    if module_prefix.is_empty() {
        return true;
    }
    path == module_prefix
        || path
            .strip_prefix(module_prefix)
            .is_some_and(|rest| rest.starts_with("::"))
}

/// Native architectural dependency rule enforcing that modules under `subject_prefix`
/// (excluding `exempt_prefixes`) never reference any path under `forbidden_prefixes`.
#[derive(Debug, Clone)]
struct ForbiddenDependencyRule {
    subject_prefix: String,
    forbidden_prefixes: Vec<String>,
    exempt_prefixes: Vec<String>,
}

impl ForbiddenDependencyRule {
    fn must_not_depend_on<'a>(
        subject_prefix: &str,
        forbidden: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        Self {
            subject_prefix: subject_prefix.to_string(),
            forbidden_prefixes: forbidden.into_iter().map(str::to_owned).collect(),
            exempt_prefixes: Vec::new(),
        }
    }

    fn is_applicable(&self, module_path: &str) -> bool {
        is_in_module_subtree(module_path, &self.subject_prefix)
            && !self
                .exempt_prefixes
                .iter()
                .any(|exempt| is_in_module_subtree(module_path, exempt))
    }

    fn violations_in_file(&self, entry: &SourceFileEntry) -> Vec<String> {
        if !self.is_applicable(&entry.module_path) {
            return Vec::new();
        }
        let subject_label = if self.subject_prefix.is_empty() {
            "omni"
        } else {
            &self.subject_prefix
        };
        let mut violations = Vec::new();
        for reference in &entry.summary.referenced_paths {
            let resolved = resolve_reference_path(
                &entry.module_path,
                &entry.summary.external_mods,
                &reference.raw_path,
            );
            for forbidden in &self.forbidden_prefixes {
                if is_in_module_subtree(&resolved.canonical_path, forbidden) {
                    violations.push(format!(
                        "{}:{}: `{subject_label}` must not depend on `{forbidden}` (referenced `{}` -> `{}` in `{}`)",
                        entry.path.display(),
                        reference.line,
                        reference.raw_path,
                        resolved.canonical_path,
                        reference.statement_text,
                    ));
                }
            }
        }
        violations
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

fn source_root_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Recursively lists the `.rs` files under `directory`.
fn rust_files(directory: &Path) -> Vec<PathBuf> {
    let entries = std::fs::read_dir(directory).expect("source directory should be readable");
    let mut paths: Vec<PathBuf> = entries
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
        .collect();
    paths.sort();
    paths
}

/// Computes the crate-relative module path of a source file (e.g. `code_lint::rules::banned_abbreviations`).
fn module_path_for_file(path: &Path) -> String {
    let relative_path = path
        .strip_prefix(source_root_directory())
        .unwrap_or(path)
        .with_extension("");
    let mut parts: Vec<_> = relative_path
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect();
    if parts.last().is_some_and(|last| last == "mod") {
        parts.pop();
    }
    parts.join("::").replace('-', "_")
}

/// Cached `SourceFileEntry` list for all `.rs` files under `src/`, parsed once in a single pass.
static SOURCE_FILES: LazyLock<Vec<SourceFileEntry>> = LazyLock::new(|| {
    rust_files(&source_root_directory())
        .into_iter()
        .map(|path| {
            let source = std::fs::read_to_string(&path).expect("source files should be readable");
            let module_path = module_path_for_file(&path);
            SourceFileEntry::from_source(path, module_path, &source)
        })
        .collect()
});

/// Cached `ArchitectureComponent` declarations (`architecture_component!(...)`) in component root files under `src/`.
static DECLARED_COMPONENTS: LazyLock<BTreeMap<String, ArchitectureComponent>> =
    LazyLock::new(|| {
        SOURCE_FILES
            .iter()
            .filter_map(|entry| {
                let [name] = entry.summary.architecture_components.as_slice() else {
                    return None;
                };
                let component = name.parse::<ArchitectureComponent>().ok()?;
                Some((entry.module_path.clone(), component))
            })
            .collect()
    });

/// Resolves the `(component_root_module, ArchitectureComponent)` for `module` by climbing up its
/// module path to the nearest ancestor (or self) that declares `architecture_component!(...)`.
fn resolve_inherited_component_with_root<'a>(
    module: &'a str,
    declared_components: &'a BTreeMap<String, ArchitectureComponent>,
) -> Option<(&'a str, ArchitectureComponent)> {
    let mut current = module;
    loop {
        if let Some((root_key, &component)) = declared_components.get_key_value(current) {
            return Some((root_key.as_str(), component));
        }
        if let Some((parent, _)) = current.rsplit_once("::") {
            current = parent;
        } else {
            return None;
        }
    }
}

/// Resolves the `ArchitectureComponent` for `module` by climbing up its module path to the
/// nearest ancestor (or self) that declares `architecture_component!(...)`.
fn resolve_inherited_component(
    module: &str,
    declared_components: &BTreeMap<String, ArchitectureComponent>,
) -> Option<ArchitectureComponent> {
    resolve_inherited_component_with_root(module, declared_components)
        .map(|(_, component)| component)
}

/// Groups declared component root modules by `ArchitectureComponent`.
fn discover_component_roots(
    declared_components: &BTreeMap<String, ArchitectureComponent>,
) -> BTreeMap<ArchitectureComponent, Vec<String>> {
    let mut roots: BTreeMap<ArchitectureComponent, Vec<String>> = BTreeMap::new();
    for (module, &component) in declared_components {
        roots.entry(component).or_default().push(module.clone());
    }
    roots
}

/// Cached verification rules derived directly from `ARCHITECTURE_GRAPH`
/// and the component root `architecture_component!(...)` declarations in `src/`.
static ARCHITECTURE_CONFORMANCE_RULES: LazyLock<Vec<ForbiddenDependencyRule>> =
    LazyLock::new(|| {
        let component_roots = discover_component_roots(&DECLARED_COMPONENTS);
        let all_modules: Vec<&str> = SOURCE_FILES
            .iter()
            .map(|entry| entry.module_path.as_str())
            .collect();
        let mut rules = Vec::new();

        // 1. Cross-component DAG dependency rules
        for definition in ARCHITECTURE_GRAPH {
            let mut allowed =
                compute_transitive_reachability(definition.component, ARCHITECTURE_GRAPH);
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
                    rules.push(ForbiddenDependencyRule::must_not_depend_on(
                        subject_module,
                        forbidden_modules.iter().copied(),
                    ));
                }
            }
        }

        // 2. Universal intra-component sibling subtree isolation
        for definition in ARCHITECTURE_GRAPH {
            let Some(roots) = component_roots.get(&definition.component) else {
                continue;
            };

            // 2a. Multi-root sibling isolation (e.g. `FoundationPrimitives`, `ApplicationBinaries`)
            if roots.len() > 1 {
                for root in roots {
                    let siblings = roots
                        .iter()
                        .map(String::as_str)
                        .filter(|&other| other != root);
                    rules.push(ForbiddenDependencyRule::must_not_depend_on(root, siblings));
                }
            }

            // 2b. Direct-child subtree isolation under each component root
            for root in roots {
                let direct_children: Vec<&str> = all_modules
                    .iter()
                    .copied()
                    .filter(|module| {
                        module
                            .rsplit_once("::")
                            .is_some_and(|(parent, _)| parent == root)
                    })
                    .collect();

                if direct_children.len() > 1 {
                    for &child in &direct_children {
                        let siblings = direct_children
                            .iter()
                            .copied()
                            .filter(|&other| other != child);
                        rules.push(ForbiddenDependencyRule::must_not_depend_on(child, siblings));
                    }
                }
            }
        }

        rules
    });

fn ast_grep_encapsulation_rules() -> Vec<ForbiddenDependencyRule> {
    vec![ForbiddenDependencyRule {
        subject_prefix: String::new(),
        forbidden_prefixes: vec!["ast_grep_core".to_string()],
        exempt_prefixes: AST_GREP_OWNERS.iter().copied().map(str::to_owned).collect(),
    }]
}

fn violations_in(files: &[SourceFileEntry], rules: &[ForbiddenDependencyRule]) -> Vec<String> {
    files
        .iter()
        .flat_map(|entry| rules.iter().flat_map(|rule| rule.violations_in_file(entry)))
        .collect()
}

fn assert_src_complies_with(rules: &[ForbiddenDependencyRule]) {
    let violations = violations_in(&SOURCE_FILES, rules);
    assert!(
        violations.is_empty(),
        "Architecture violations:\n{}",
        violations.join("\n")
    );
}

/// Returns true if `visible_use` is an idiomatic private-child facade re-export
/// (`mod child; pub use self::child::Item;` or `mod child; pub use child::Item;`).
fn is_private_child_facade_reexport(
    visible_use: &VisibleUseDeclaration,
    external_mods: &[ExternalModDeclaration],
) -> bool {
    if visible_use.target_paths.is_empty() {
        return false;
    }
    visible_use.target_paths.iter().all(|target| {
        let stripped = target.strip_prefix("self::").unwrap_or(target);
        let Some((child_module, rest)) = stripped.split_once("::") else {
            return false;
        };
        !rest.is_empty()
            && external_mods
                .iter()
                .any(|declaration| declaration.is_private && declaration.name == child_module)
    })
}

/// Returns all declarations in `entry` that introduce an illegal second path to an item.
///
/// Every item keeps a single public path:
/// - `#[macro_export]` is allowed only in `src/lib.rs`.
/// - `pub(crate) use <local_macro>;` for a `macro_rules!` defined in the same file is allowed.
/// - `pub use` / `pub(crate) use` is allowed only when re-exporting items from a private direct
///   child module (`mod <child>;`), acting as a single-path component facade.
fn second_path_violations_in_entry(entry: &SourceFileEntry) -> Vec<String> {
    let mut violations = Vec::new();
    if !entry.is_crate_root {
        for export in &entry.summary.macro_exports {
            violations.push(export.clone());
        }
    }
    for visible_use in &entry.summary.visible_uses {
        if !is_private_child_facade_reexport(visible_use, &entry.summary.external_mods) {
            violations.push(visible_use.declaration_text.clone());
        }
    }
    violations
}

/// Returns `(line, violation_message)` for any relative path (`super::` or `self::`) in `entry`
/// that escapes the enclosing file's `ArchitectureComponent` root subtree.
fn relative_path_boundary_violations_in_entry(
    entry: &SourceFileEntry,
    declared_components: &BTreeMap<String, ArchitectureComponent>,
) -> Vec<(usize, String)> {
    let enclosing_root =
        resolve_inherited_component_with_root(&entry.module_path, declared_components);
    let mut violations = Vec::new();

    for reference in &entry.summary.referenced_paths {
        let resolved = resolve_reference_path(
            &entry.module_path,
            &entry.summary.external_mods,
            &reference.raw_path,
        );
        let PathOrigin::Relative {
            shallowest_ancestor,
        } = &resolved.origin
        else {
            continue;
        };

        let stays_in_component_root = enclosing_root
            .zip(shallowest_ancestor.as_deref())
            .is_some_and(|((root_module, _), ancestor)| {
                is_in_module_subtree(ancestor, root_module)
                    && is_in_module_subtree(&resolved.canonical_path, root_module)
            });

        if !stays_in_component_root {
            violations.push((
                reference.line,
                format!(
                    "`{}` resolves to `{}` outside component root (use `crate::{}` for cross-component references): {}",
                    reference.raw_path,
                    resolved.canonical_path,
                    resolved.canonical_path,
                    reference.statement_text,
                ),
            ));
        }
    }

    violations
}

/// Validates a slice of `SourceFileEntry` items against the component declaration, subtree
/// inheritance, and pure namespace router invariants, returning any human-readable violations.
fn validate_component_declarations_and_routers(
    entries: &[SourceFileEntry],
    require_all_variants_covered: bool,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut declared_components = BTreeMap::new();

    for entry in entries {
        match entry.summary.architecture_components.as_slice() {
            [] => {}
            [component_name] => match component_name.parse::<ArchitectureComponent>() {
                Err(_) => {
                    violations.push(format!(
                        "{}: unknown component '{component_name}'",
                        entry.path.display()
                    ));
                }
                Ok(component) => {
                    declared_components.insert(entry.module_path.clone(), component);
                }
            },
            multiple => {
                violations.push(format!(
                    "{}: multiple architecture_component! declarations found: {}",
                    entry.path.display(),
                    multiple.join(", ")
                ));
            }
        }
    }

    for (module, &component) in &declared_components {
        let mut current = module.as_str();
        while let Some((parent, _)) = current.rsplit_once("::") {
            if let Some(&ancestor_component) = declared_components.get(parent) {
                violations.push(format!(
                    "{module}: declares '{component}', but ancestor '{parent}' already declares '{ancestor_component}' (child modules inherit their ancestor's component automatically)"
                ));
                break;
            }
            current = parent;
        }
    }

    for entry in entries {
        if !entry.summary.architecture_components.is_empty()
            || resolve_inherited_component(&entry.module_path, &declared_components).is_some()
        {
            continue;
        }

        let routes_to_declared_component = entry.is_crate_root
            || declared_components
                .keys()
                .any(|root| is_in_module_subtree(root, &entry.module_path));

        if entry.summary.external_mods.is_empty() || !routes_to_declared_component {
            violations.push(format!(
                "{}: missing 'architecture_component!(<Variant>);' (and not a pure namespace router for any declared component)",
                entry.path.display()
            ));
            continue;
        }

        let disallowed_items =
            entry
                .summary
                .non_namespace_items
                .iter()
                .chain(if entry.is_crate_root {
                    &[][..]
                } else {
                    entry.summary.macro_definitions.as_slice()
                });
        for (line, item) in disallowed_items {
            violations.push(format!(
                "{}:{line}: pure namespace router without 'architecture_component!' may only contain external 'mod' declarations, found: {item}",
                entry.path.display()
            ));
        }
    }

    if require_all_variants_covered {
        let component_roots = discover_component_roots(&declared_components);
        for component in ArchitectureComponent::VARIANTS {
            if component_roots.get(component).is_none_or(Vec::is_empty) {
                violations.push(format!(
                    "Component '{component}' has no root source files declaring it in src/"
                ));
            }
        }
    }

    violations
}

#[test]
fn test_all_source_files_declare_architecture_component() {
    let violations = validate_component_declarations_and_routers(&SOURCE_FILES, true);
    assert!(
        violations.is_empty(),
        "Component declaration or namespace router violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_architecture_conformance() {
    assert_src_complies_with(&ARCHITECTURE_CONFORMANCE_RULES);
}

#[test]
fn test_ast_grep_is_encapsulated() {
    assert_src_complies_with(&ast_grep_encapsulation_rules());
}

#[test]
fn test_items_have_a_single_path() {
    let violations: Vec<String> = SOURCE_FILES
        .iter()
        .flat_map(|entry| {
            second_path_violations_in_entry(entry)
                .into_iter()
                .map(|declaration| format!("{}: {declaration}", entry.path.display()))
        })
        .collect();
    assert!(
        violations.is_empty(),
        "Second paths found:\n{}",
        violations.join("\n")
    );
}

/// Enforces that relative paths (`super::` / `self::`) stay within their enclosing
/// `ArchitectureComponent` root subtree, while cross-component references use `crate::`.
#[test]
fn test_relative_paths_stay_within_component() {
    let violations: Vec<String> = SOURCE_FILES
        .iter()
        .flat_map(|entry| {
            relative_path_boundary_violations_in_entry(entry, &DECLARED_COMPONENTS)
                .into_iter()
                .map(|(line, message)| format!("{}:{line}: {message}", entry.path.display()))
        })
        .collect();
    assert!(
        violations.is_empty(),
        "Cross-component relative paths found in production code:\n{}",
        violations.join("\n")
    );
}

/// Guards against vacuous passes: re-exports from public submodules or other components and
/// non-root `#[macro_export]` are flagged, while private-child `mod detail; pub use self::detail::Item;`
/// facades and local `macro_rules!` paths are allowed.
#[test]
fn test_second_path_detection_allows_private_child_facades_and_rejects_duplicates() {
    let source = indoc::indoc! {r#"
        mod private_detail;
        pub mod public_child;

        macro_rules! local_macro {
            () => {};
        }
        pub(crate) use local_macro;

        // Allowed: private-child facade re-exports keep a single public path
        pub use self::private_detail::{AllowedFacade, AnotherAllowed};
        pub(crate) use private_detail::CrateAllowedFacade;

        // Ignored: comments and string literals
        // pub use crate::commented::Out;
        const FIXTURE: &str = r"
        pub use crate::fake::ReExport;
        #[macro_export]
        ";

        // Rejected: re-exporting from a public child module creates a second public path
        pub use self::public_child::DuplicatePathItem;
        // Rejected: cross-component re-export hides the defining component
        pub use crate::diagnostic::RuleName;
        pub(crate) use crate::core::Tag;
        #[macro_export]
        macro_rules! exported {
            () => {};
        }
    "#};
    let entry = SourceFileEntry::from_source("src/code_lint/ast.rs", "code_lint::ast", source);
    assert_eq!(
        second_path_violations_in_entry(&entry),
        vec![
            "#[macro_export]",
            "pub use self::public_child::DuplicatePathItem;",
            "pub use crate::diagnostic::RuleName;",
            "pub(crate) use crate::core::Tag;",
        ]
    );
}

/// Guards against vacuous passes in relative path boundary checking: intra-component `super::`
/// and `self::` are allowed, while cross-component `super::` (both in `use` and inline) is rejected.
#[test]
fn test_relative_path_boundary_allows_intra_component_and_rejects_cross_component() {
    let declared_components = BTreeMap::from([(
        "code_lint::ast".to_owned(),
        ArchitectureComponent::CodeSyntaxAdapters,
    )]);
    let child_source = indoc::indoc! {r#"
        // Allowed: child `code_lint::ast::rust` referencing parent root `code_lint::ast`
        use super::{AstNode, ParsedFile};

        #[cfg(test)]
        fn test_helper() {
            use super::super::rule::AllowedInTest;
        }

        const FIXTURE: &str = r"
        use super::super::fake_string_import;
        ";

        // Rejected: climbing out of `code_lint::ast` into `code_lint::rule` (or re-entering `ast`)
        use super::super::rule::CodeDetector;
        use super::super::ast::AstNode as ReenteredAstNode;

        pub fn call_escaped() {
            let _ = super::super::diagnostic::Violation::new();
        }
    "#};
    let child_entry = SourceFileEntry::from_source(
        "src/code_lint/ast/rust.rs",
        "code_lint::ast::rust",
        child_source,
    );
    let child_lines: Vec<usize> =
        relative_path_boundary_violations_in_entry(&child_entry, &declared_components)
            .into_iter()
            .map(|(line, _)| line)
            .collect();
    assert_eq!(child_lines, vec![14, 15, 18]);

    let root_source = indoc::indoc! {r"
        pub mod rust;
        // Allowed: component root referencing its own subtree via `self::`
        use self::rust::collect_bindings;
        // Rejected: component root using `super::` or `self::super::` to escape its root
        use super::semantic::bindings;
        use self::super::ast::ParsedFile;
    "};
    let root_entry =
        SourceFileEntry::from_source("src/code_lint/ast.rs", "code_lint::ast", root_source);
    let root_lines: Vec<usize> =
        relative_path_boundary_violations_in_entry(&root_entry, &declared_components)
            .into_iter()
            .map(|(line, _)| line)
            .collect();
    assert_eq!(root_lines, vec![5, 6]);
}

/// Guards against vacuous passes: upward dependencies, cross-domain dependencies, sibling-unit
/// imports (including via `super::`), turbofish paths, and macro-argument paths must all be caught.
#[test]
fn test_architecture_rules_detect_forbidden_dependencies() {
    let offending_files = [
        SourceFileEntry::from_source(
            "src/code_lint/rules/offending.rs",
            "code_lint::rules::offending",
            "fn call() { let _ = crate::code_lint::runner::lint_file::<crate::command_lint::vcs::Vcs>(); }",
        ),
        SourceFileEntry::from_source(
            "src/code_lint/semantic/bindings.rs",
            "code_lint::semantic::bindings",
            "fn call_sibling() { assert!(super::calls::check()); }",
        ),
        SourceFileEntry::from_source("src/diff.rs", "diff", "use crate::diagnostic::Violation;"),
    ];

    let violations = violations_in(&offending_files, &ARCHITECTURE_CONFORMANCE_RULES);
    let matched_targets: Vec<&str> = [
        "code_lint::runner",
        "command_lint::vcs",
        "code_lint::semantic::calls",
        "diagnostic",
    ]
    .into_iter()
    .filter(|target| {
        violations
            .iter()
            .any(|violation| violation.contains(target))
    })
    .collect();

    assert_eq!(
        matched_targets,
        vec![
            "code_lint::runner",
            "command_lint::vcs",
            "code_lint::semantic::calls",
            "diagnostic",
        ]
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
        .map(|module| resolve_inherited_component(module, &declared_components))
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

#[test]
fn test_namespace_validator_accepts_pure_routers_and_rejects_code_or_orphan_routers() {
    let valid_entries = [
        SourceFileEntry::from_source(
            "src/code_lint.rs",
            "code_lint",
            "pub mod ast;\n#[cfg(test)]\nmod tests { fn allowed() {} }\n",
        ),
        SourceFileEntry::from_source(
            "src/code_lint/ast.rs",
            "code_lint::ast",
            "architecture_component!(CodeSyntaxAdapters);\npub struct ParsedFile;\n",
        ),
    ];
    assert!(validate_component_declarations_and_routers(&valid_entries, false).is_empty());

    let impure_and_orphan_entries = [
        SourceFileEntry::from_source(
            "src/code_lint.rs",
            "code_lint",
            "pub mod ast;\nuse crate::core::Config;\npub const SNEAKY: usize = 1;\n",
        ),
        SourceFileEntry::from_source(
            "src/code_lint/ast.rs",
            "code_lint::ast",
            "architecture_component!(CodeSyntaxAdapters);\n",
        ),
        SourceFileEntry::from_source(
            "src/orphan_router.rs",
            "orphan_router",
            "pub mod nothing;\n",
        ),
    ];
    let violations = validate_component_declarations_and_routers(&impure_and_orphan_entries, false);
    assert_eq!(
        violations,
        vec![
            "src/code_lint.rs:2: pure namespace router without 'architecture_component!' may only contain external 'mod' declarations, found: use crate::core::Config;",
            "src/code_lint.rs:3: pure namespace router without 'architecture_component!' may only contain external 'mod' declarations, found: pub const SNEAKY: usize = 1;",
            "src/orphan_router.rs: missing 'architecture_component!(<Variant>);' (and not a pure namespace router for any declared component)",
        ]
    );
}
