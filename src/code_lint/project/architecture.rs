//! Language-agnostic architectural component specification (`ArchitectureSpec`) and per-file
//! conformance evaluators (`CodeLintProject`).

use super::layout::{
    PathOrigin, ProjectLayout, ROOT_MODULE_TAG, is_in_module_subtree, is_python_package_init,
    is_rust_crate_root, resolve_inherited_component_with_root, resolve_relative_python_package,
    resolve_rust_reference_path,
};
use super::source::FileSource;
use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    PythonFileSummary, PythonImportReference, collect_python_import_references,
    summarize_python_file,
};
use crate::code_lint::ast::rust::{DefinedComponentEntry, RustFileSummary, summarize_rust_file};
use crate::config::{ArchitectureComponentConfig, ArchitectureConfig};
use crate::diagnostic::{Language, SourceSpan};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Canonical Rust module name (`src/architecture.rs` or `src/architecture/mod.rs`) where
/// `define_architecture!` is recognized.
pub(super) const RUST_ARCHITECTURE_MODULE: &str = "architecture";

const RUST_MODULE_SEPARATOR: &str = "::";
const PYTHON_MODULE_SEPARATOR: &str = ".";
const PREFIX_MATCH_RANK: usize = 3;

/// Errors raised when loading or validating a project's [`ArchitectureSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchitectureError {
    /// Both `.omnicoach.toml` `[[architecture.components]]` and Rust `define_architecture!` exist.
    ConflictingDeclarations,
    /// Two component declarations share the same component name.
    DuplicateComponent(String),
    /// Component `component` lists `dependency` which is undeclared or declared after `component`.
    ForwardOrUnknownDependency {
        /// Dependent component name.
        component: String,
        /// Invalid or out-of-order dependency name.
        dependency: String,
    },
    /// Configuration parsing failure.
    Parse(String),
}

impl std::fmt::Display for ArchitectureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConflictingDeclarations => write!(
                f,
                "conflicting architecture declarations: both `.omnicoach.toml` `[[architecture.components]]` and Rust `define_architecture!` are present"
            ),
            Self::DuplicateComponent(name) => {
                write!(f, "duplicate architecture component `{name}`")
            }
            Self::ForwardOrUnknownDependency {
                component,
                dependency,
            } => write!(
                f,
                "component `{component}` depends on `{dependency}`, which is undeclared or declared later in the topological component list"
            ),
            Self::Parse(message) => write!(f, "invalid `[architecture]` configuration: {message}"),
        }
    }
}

impl std::error::Error for ArchitectureError {}

/// Validated, language-agnostic architectural specification for a project.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent boolean policy switches from [architecture] config"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchitectureSpec {
    /// Python source roots relative to project root.
    pub source_roots: Vec<PathBuf>,
    /// Whether `depends_on` reachability is transitive.
    pub transitive_dependencies: bool,
    /// Whether unmapped production files are flagged.
    pub require_component_coverage: bool,
    /// Whether Python function-local imports are checked (default `true`).
    pub check_local_imports: bool,
    /// Whether Python imports inside `if TYPE_CHECKING:` blocks are exempt.
    pub ignore_type_checking_imports: bool,
    /// True when component roots were synthesized from `define_architecture!` (requiring every
    /// declared component to have at least one `architecture_component!(...)` root).
    pub from_rust_macros: bool,
    /// Topologically ordered components.
    pub components: Vec<ComponentSpec>,
    external_package_owners: BTreeMap<String, Vec<String>>,
    component_roots: BTreeMap<String, String>,
    allowed_dependencies: HashMap<String, HashSet<String>>,
    component_by_name: HashMap<String, usize>,
    known_components: HashSet<String>,
    rust_sibling_rules: Vec<SiblingIsolationRule>,
}

/// Validated component entry in [`ArchitectureSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentSpec {
    /// Component identifier.
    pub name: String,
    /// Canonical root module paths.
    pub roots: Vec<String>,
    /// Direct allowed component names.
    pub depends_on: Vec<String>,
    /// Whether sibling subtrees inside this component are isolated from each other.
    pub isolate_children: bool,
    /// External third-party crates or packages encapsulated by this component.
    pub owns_external: Vec<String>,
    /// Optional public interface export rules per root module (`None` = unrestricted submodules).
    pub public_exports: Option<Vec<ExportPattern>>,
}

/// Export allowlist glob pattern on a component root.
#[derive(Debug, Clone)]
pub struct ExportPattern {
    /// Configured glob symbol pattern (`build_*`, `main`).
    pub pattern: String,
    matcher: Option<globset::GlobMatcher>,
}

impl PartialEq for ExportPattern {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern
    }
}

impl Eq for ExportPattern {}

impl ExportPattern {
    /// Compiles a symbol export glob pattern (`build_*`, `main`) into a reusable matcher.
    #[must_use]
    pub fn new(raw: &str) -> Self {
        let matcher = crate::config::compile_glob(raw)
            .ok()
            .map(|glob| glob.compile_matcher());
        Self {
            pattern: raw.to_owned(),
            matcher,
        }
    }

    /// Returns true if `candidate` matches this export pattern.
    #[must_use]
    pub fn matches(&self, candidate: &str) -> bool {
        self.pattern == candidate
            || self
                .matcher
                .as_ref()
                .is_some_and(|compiled| compiled.is_match(candidate))
    }
}

impl ArchitectureSpec {
    /// Canonical root module paths mapped to their owning component name.
    #[must_use]
    pub const fn component_roots(&self) -> &BTreeMap<String, String> {
        &self.component_roots
    }

    /// Resolves the owning component name of `module_path` for `lang`, if mapped by this spec.
    #[must_use]
    pub fn resolve_component_of_module(&self, module_path: &str, lang: Language) -> Option<&str> {
        match lang {
            Language::Rust => {
                resolve_inherited_component_with_root(module_path, &self.component_roots)
                    .map(|inherited| inherited.component.as_str())
            }
            Language::Python => {
                resolve_python_component(module_path, self).map(|matched| matched.component_name)
            }
        }
    }

    fn component_spec(&self, name: &str) -> Option<&ComponentSpec> {
        self.component_by_name
            .get(name)
            .and_then(|&index| self.components.get(index))
    }
}

/// Violation finding for `undeclared-component-dependency`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndeclaredComponentDependencyFinding {
    /// AST byte span of the offending import or path reference.
    pub span: SourceSpan,
    /// Enclosing component name.
    pub component: String,
    /// Referenced target component name.
    pub dependency: String,
}

/// Violation finding for `sibling-module-dependency`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiblingModuleDependencyFinding {
    /// AST byte span of the offending import or path reference.
    pub span: SourceSpan,
    /// Enclosing module path.
    pub module: String,
    /// Forbidden sibling module subtree prefix.
    pub dependency: String,
    /// Enclosing component name.
    pub component: String,
}

/// Violation finding for `cross-component-relative-path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossComponentRelativePathFinding {
    /// AST byte span of the offending relative import or path reference.
    pub span: SourceSpan,
    /// Resolved target module path outside the component root.
    pub dependency: String,
    /// Enclosing component name.
    pub component: String,
}

/// Violation finding for `unowned-external-dependency`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnownedExternalDependencyFinding {
    /// AST byte span of the offending external import or path reference.
    pub span: SourceSpan,
    /// Enclosing module path.
    pub module: String,
    /// External crate or package name.
    pub dependency: String,
    /// Permitted owner module prefixes.
    pub owners: Vec<String>,
}

/// Violation finding for `private-component-import`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateComponentImportFinding {
    /// AST byte span of the offending import or path reference.
    pub span: SourceSpan,
    /// Enclosing component name.
    pub component: String,
    /// Imported symbol or submodule name, or `None` for wildcard (`*`) and bare root-module imports.
    pub name: Option<String>,
    /// Target component root module path.
    pub module: String,
    /// Target component name whose `public-exports` were bypassed.
    pub dependency: String,
}

/// Violation finding for `unmapped-architecture-module`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmappedArchitectureModuleFinding {
    /// AST byte span of the offending declaration or module header.
    pub span: SourceSpan,
    /// Unmapped item or declaration name.
    pub name: String,
    /// Unmapped or impure router module path.
    pub module: String,
}

/// Reason an `architecture_component!(...)` macro call in Rust source code has no effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidArchitectureComponentKind {
    /// No `define_architecture!` declaration exists in the crate's `architecture` module.
    MissingDefineArchitecture,
    /// `.omnicoach.toml` declares `[[architecture.components]]`, so Rust component macros are inactive.
    TomlArchitectureActive,
    /// `architecture_component!` appears inside an inner item instead of at the module root.
    NotAtModuleRoot,
    /// A second top-level `architecture_component!` appears in the same module.
    DuplicateInModule {
        /// Component name from the first top-level `architecture_component!` in the module.
        first_component: String,
    },
    /// Component identifier is not declared in `define_architecture!`.
    UnknownComponent,
    /// An ancestor module in the module tree already declares a component root.
    NestedUnderComponent {
        /// Ancestor module path that owns the subtree.
        ancestor_module: String,
        /// Component declared on `ancestor_module`.
        ancestor_component: String,
    },
}

/// Violation finding for `invalid-architecture-component`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidArchitectureComponentFinding {
    /// AST byte span of the ignored `architecture_component!(...)` macro call.
    pub span: SourceSpan,
    /// Raw component argument inside `architecture_component!(...)`.
    pub component: String,
    /// Enclosing Rust module path.
    pub module: String,
    /// Structured reason the declaration has no effect.
    pub kind: InvalidArchitectureComponentKind,
}

/// All architectural conformance findings evaluated for a single source file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FileArchitectureFindings {
    /// Findings for `undeclared-component-dependency`.
    pub undeclared_component_dependencies: Vec<UndeclaredComponentDependencyFinding>,
    /// Findings for `sibling-module-dependency`.
    pub sibling_module_dependencies: Vec<SiblingModuleDependencyFinding>,
    /// Findings for `cross-component-relative-path`.
    pub cross_component_relative_paths: Vec<CrossComponentRelativePathFinding>,
    /// Findings for `unowned-external-dependency`.
    pub unowned_external_dependencies: Vec<UnownedExternalDependencyFinding>,
    /// Findings for `private-component-import`.
    pub private_component_imports: Vec<PrivateComponentImportFinding>,
    /// Findings for `unmapped-architecture-module`.
    pub unmapped_architecture_modules: Vec<UnmappedArchitectureModuleFinding>,
    /// Findings for `invalid-architecture-component`.
    pub invalid_architecture_components: Vec<InvalidArchitectureComponentFinding>,
}

/// Evaluates the component-level architectural conformance rules for a single parsed `file` at
/// `path`.
#[must_use]
pub fn evaluate_file_architecture(
    file: &ParsedFile,
    path: &Path,
    layout: &ProjectLayout,
    source: &dyn FileSource,
    spec: &ArchitectureSpec,
) -> FileArchitectureFindings {
    evaluate_file_architecture_with_summary(file, path, layout, source, spec, None)
}

/// Evaluates the component-level architectural conformance rules for `file` (`path`), reusing an
/// optional pre-computed [`PythonFileSummary`] when `file` is a Python module.
#[must_use]
pub fn evaluate_file_architecture_with_summary(
    file: &ParsedFile,
    path: &Path,
    layout: &ProjectLayout,
    source: &dyn FileSource,
    spec: &ArchitectureSpec,
    python_summary: Option<&PythonFileSummary>,
) -> FileArchitectureFindings {
    match file.lang() {
        Language::Rust => evaluate_rust_file_architecture(file, path, layout, spec),
        Language::Python => {
            evaluate_python_file_architecture(file, path, layout, source, spec, python_summary)
        }
    }
}

/// Evaluates stray `architecture_component!(...)` macro calls in a Rust `file` (`path`) when
/// the project declares no [`ArchitectureSpec`].
#[must_use]
pub fn evaluate_stray_rust_component_markers(
    file: &ParsedFile,
    path: &Path,
    layout: &ProjectLayout,
) -> FileArchitectureFindings {
    if file.lang() != Language::Rust {
        return FileArchitectureFindings::default();
    }
    let Some(module_path) = layout.rust_module_of_path(path) else {
        return FileArchitectureFindings::default();
    };
    let summary = summarize_rust_file(file);
    let mut findings = FileArchitectureFindings {
        invalid_architecture_components: summary
            .architecture_components
            .into_iter()
            .map(|(span, component)| InvalidArchitectureComponentFinding {
                span,
                component,
                module: module_path.clone(),
                kind: InvalidArchitectureComponentKind::MissingDefineArchitecture,
            })
            .collect(),
        ..FileArchitectureFindings::default()
    };
    deduplicate_findings(&mut findings);
    findings
}

/// Builds and validates the project's [`ArchitectureSpec`].
///
/// A crate that declares `define_architecture!` in its `architecture` module (`src/architecture.rs`
/// or `src/architecture/mod.rs`) gets its spec from `define_architecture!` and per-module
/// `architecture_component!` headers; otherwise the spec comes from the `.omnicoach.toml`
/// `[architecture]` block.
///
/// # Errors
///
/// Returns [`ArchitectureError`] if both `.omnicoach.toml` `[[architecture.components]]` and Rust
/// `define_architecture!` in `architecture` are non-empty, or if `depends_on` violates forward
/// declaration order.
pub fn resolve_architecture_spec(
    project_root: &Path,
    toml_config: Option<&ArchitectureConfig>,
    rust_headers: &BTreeMap<String, Arc<RustFileSummary>>,
) -> Result<Option<ArchitectureSpec>, ArchitectureError> {
    let has_toml_components = toml_config.is_some_and(|config| !config.components.is_empty());
    let has_rust_define_macro = rust_headers
        .get(RUST_ARCHITECTURE_MODULE)
        .is_some_and(|summary| !summary.defined_architecture.is_empty());

    if has_rust_define_macro {
        if has_toml_components {
            return Err(ArchitectureError::ConflictingDeclarations);
        }
        let synthesized_config = architecture_config_from_rust_headers(rust_headers);
        return build_architecture_spec(project_root, &synthesized_config, true, rust_headers)
            .map(Some);
    }

    match toml_config {
        Some(config) if has_toml_components => {
            build_architecture_spec(project_root, config, false, rust_headers).map(Some)
        }
        _ => Ok(None),
    }
}

fn evaluate_rust_file_architecture(
    file: &ParsedFile,
    path: &Path,
    layout: &ProjectLayout,
    spec: &ArchitectureSpec,
) -> FileArchitectureFindings {
    let Some(module_path) = layout.rust_module_of_path(path) else {
        return FileArchitectureFindings::default();
    };
    if spec.components.is_empty() && !spec.from_rust_macros {
        return FileArchitectureFindings::default();
    }
    let summary = summarize_rust_file(file);
    let mut findings = FileArchitectureFindings::default();

    let has_invalid_macro = check_rust_invalid_component_markers(
        &module_path,
        &summary,
        spec,
        &mut findings.invalid_architecture_components,
    );
    if !(spec.from_rust_macros && has_invalid_macro) {
        check_rust_file_coverage_and_routers(
            file,
            &module_path,
            &summary,
            spec,
            &mut findings.unmapped_architecture_modules,
        );
    }

    let enclosing = resolve_inherited_component_with_root(&module_path, &spec.component_roots);

    for reference in &summary.referenced_paths {
        let resolved = resolve_rust_reference_path(
            &module_path,
            &summary.external_mods,
            &reference.raw_path,
            layout.rust_crate_name.as_deref(),
        );

        if let PathOrigin::Relative {
            shallowest_ancestor,
        } = &resolved.origin
            && let Some(ref inherited) = enclosing
            && !shallowest_ancestor.as_deref().is_some_and(|ancestor| {
                is_in_module_subtree(ancestor, inherited.root_module)
                    && is_in_module_subtree(&resolved.canonical_path, inherited.root_module)
            })
        {
            findings
                .cross_component_relative_paths
                .push(CrossComponentRelativePathFinding {
                    span: reference.span,
                    dependency: resolved.canonical_path.clone(),
                    component: inherited.component.clone(),
                });
        }

        if !resolved.is_crate_local() {
            check_rust_external_owner(
                &module_path,
                &resolved.canonical_path,
                reference.span,
                spec,
                &mut findings.unowned_external_dependencies,
            );
        }

        if let Some(ref from_component) = enclosing
            && let Some(target_component) = resolve_inherited_component_with_root(
                &resolved.canonical_path,
                &spec.component_roots,
            )
            && from_component.component != target_component.component
        {
            check_rust_cross_component_edge(
                from_component.component,
                target_component.component,
                target_component.root_module,
                &resolved.canonical_path,
                reference.is_glob,
                reference.span,
                spec,
                &mut findings,
            );
        }

        check_rust_sibling_isolation(
            &module_path,
            &resolved.canonical_path,
            reference.span,
            &spec.rust_sibling_rules,
            &mut findings.sibling_module_dependencies,
        );
    }

    deduplicate_findings(&mut findings);
    findings
}

#[expect(
    clippy::too_many_arguments,
    reason = "internal helper threading source/target component metadata and glob flag"
)]
fn check_rust_cross_component_edge(
    from_component: &str,
    target_component: &str,
    target_root_module: &str,
    canonical_path: &str,
    is_glob: bool,
    span: SourceSpan,
    spec: &ArchitectureSpec,
    findings: &mut FileArchitectureFindings,
) {
    let allowed = spec
        .allowed_dependencies
        .get(from_component)
        .is_some_and(|set| set.contains(target_component));
    if !allowed {
        findings
            .undeclared_component_dependencies
            .push(UndeclaredComponentDependencyFinding {
                span,
                component: from_component.to_owned(),
                dependency: target_component.to_owned(),
            });
    } else if let Some(target_spec) = spec.component_spec(target_component)
        && let Some(ref exports) = target_spec.public_exports
        && !is_rust_path_allowed_by_exports(target_root_module, canonical_path, is_glob, exports)
    {
        let symbol_name = if is_glob || canonical_path == target_root_module {
            None
        } else {
            Some(
                canonical_path
                    .rsplit(RUST_MODULE_SEPARATOR)
                    .next()
                    .unwrap_or(canonical_path)
                    .to_owned(),
            )
        };
        findings
            .private_component_imports
            .push(PrivateComponentImportFinding {
                span,
                component: from_component.to_owned(),
                name: symbol_name,
                module: target_root_module.to_owned(),
                dependency: target_component.to_owned(),
            });
    }
}

fn check_rust_external_owner(
    module_path: &str,
    canonical_path: &str,
    span: SourceSpan,
    spec: &ArchitectureSpec,
    out: &mut Vec<UnownedExternalDependencyFinding>,
) {
    for (package_name, allowed_prefixes) in &spec.external_package_owners {
        let is_allowed_owner = allowed_prefixes
            .iter()
            .any(|allowed| is_in_module_subtree(module_path, allowed));
        if is_in_module_subtree(canonical_path, package_name) && !is_allowed_owner {
            out.push(UnownedExternalDependencyFinding {
                span,
                module: module_path.to_owned(),
                dependency: package_name.clone(),
                owners: allowed_prefixes.clone(),
            });
        }
    }
}

fn check_rust_invalid_component_markers(
    module_path: &str,
    summary: &RustFileSummary,
    spec: &ArchitectureSpec,
    out: &mut Vec<InvalidArchitectureComponentFinding>,
) -> bool {
    if summary.architecture_components.is_empty() {
        return false;
    }
    if !spec.from_rust_macros {
        for (span, component) in &summary.architecture_components {
            out.push(InvalidArchitectureComponentFinding {
                span: *span,
                component: component.clone(),
                module: module_path.to_owned(),
                kind: InvalidArchitectureComponentKind::TomlArchitectureActive,
            });
        }
        return true;
    }

    let ancestor_root = module_path
        .rsplit_once(RUST_MODULE_SEPARATOR)
        .and_then(|(parent, _)| {
            resolve_inherited_component_with_root(parent, &spec.component_roots)
        });
    let mut first_top_level_component: Option<&str> = None;
    let before_len = out.len();

    for (span, component_name) in &summary.architecture_components {
        if summary.nested_architecture_components.contains(span) {
            out.push(InvalidArchitectureComponentFinding {
                span: *span,
                component: component_name.clone(),
                module: module_path.to_owned(),
                kind: InvalidArchitectureComponentKind::NotAtModuleRoot,
            });
            continue;
        }
        if let Some(first_component) = first_top_level_component {
            out.push(InvalidArchitectureComponentFinding {
                span: *span,
                component: component_name.clone(),
                module: module_path.to_owned(),
                kind: InvalidArchitectureComponentKind::DuplicateInModule {
                    first_component: first_component.to_owned(),
                },
            });
            continue;
        }
        first_top_level_component = Some(component_name.as_str());

        if !spec.known_components.contains(component_name.as_str()) {
            out.push(InvalidArchitectureComponentFinding {
                span: *span,
                component: component_name.clone(),
                module: module_path.to_owned(),
                kind: InvalidArchitectureComponentKind::UnknownComponent,
            });
        } else if let Some(ref inherited) = ancestor_root {
            out.push(InvalidArchitectureComponentFinding {
                span: *span,
                component: component_name.clone(),
                module: module_path.to_owned(),
                kind: InvalidArchitectureComponentKind::NestedUnderComponent {
                    ancestor_module: inherited.root_module.to_owned(),
                    ancestor_component: inherited.component.clone(),
                },
            });
        }
    }

    out.len() > before_len
}

fn check_rust_file_coverage_and_routers(
    file: &ParsedFile,
    module_path: &str,
    summary: &RustFileSummary,
    spec: &ArchitectureSpec,
    out: &mut Vec<UnmappedArchitectureModuleFinding>,
) {
    if !spec.require_component_coverage {
        return;
    }

    let component_roots = &spec.component_roots;
    if component_roots.contains_key(module_path)
        || resolve_inherited_component_with_root(module_path, component_roots).is_some()
    {
        return;
    }

    let is_crate_root = is_rust_crate_root(module_path);
    let header_span = default_file_header_span(file);
    let before_count = out.len();
    for external_mod in &summary.external_mods {
        let child_prefix = if is_crate_root {
            external_mod.name.clone()
        } else {
            format!("{module_path}::{}", external_mod.name)
        };
        let routes_child = (spec.from_rust_macros && child_prefix == RUST_ARCHITECTURE_MODULE)
            || component_roots
                .keys()
                .any(|root| is_in_module_subtree(root, &child_prefix));
        if !routes_child {
            out.push(UnmappedArchitectureModuleFinding {
                span: external_mod.span,
                name: external_mod.name.clone(),
                module: module_path.to_owned(),
            });
        }
    }

    let disallowed_spans = summary.non_namespace_items.iter().chain(if is_crate_root {
        &[][..]
    } else {
        summary.macro_definitions.as_slice()
    });
    for (span, item_name) in disallowed_spans {
        out.push(UnmappedArchitectureModuleFinding {
            span: *span,
            name: item_name.clone(),
            module: module_path.to_owned(),
        });
    }

    let is_pure_architecture_declaration =
        module_path == RUST_ARCHITECTURE_MODULE && !summary.defined_architecture.is_empty();
    let routes_to_declared = is_crate_root
        || is_pure_architecture_declaration
        || component_roots
            .keys()
            .any(|root| is_in_module_subtree(root, module_path));
    if out.len() == before_count
        && !is_pure_architecture_declaration
        && (summary.external_mods.is_empty() || !routes_to_declared)
    {
        out.push(UnmappedArchitectureModuleFinding {
            span: header_span,
            name: module_path.to_owned(),
            module: module_path.to_owned(),
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SiblingIsolationRule {
    component_name: String,
    subject_prefix: String,
    forbidden_siblings: Vec<String>,
}

fn check_rust_sibling_isolation(
    module_path: &str,
    canonical_path: &str,
    span: SourceSpan,
    sibling_rules: &[SiblingIsolationRule],
    out: &mut Vec<SiblingModuleDependencyFinding>,
) {
    for sibling_rule in sibling_rules {
        if is_in_module_subtree(module_path, &sibling_rule.subject_prefix)
            && let Some(forbidden_sibling) = sibling_rule
                .forbidden_siblings
                .iter()
                .find(|sibling| is_in_module_subtree(canonical_path, sibling))
        {
            out.push(SiblingModuleDependencyFinding {
                span,
                module: module_path.to_owned(),
                dependency: forbidden_sibling.clone(),
                component: sibling_rule.component_name.clone(),
            });
        }
    }
}

fn evaluate_python_file_architecture(
    file: &ParsedFile,
    path: &Path,
    layout: &ProjectLayout,
    source: &dyn FileSource,
    spec: &ArchitectureSpec,
    python_summary: Option<&PythonFileSummary>,
) -> FileArchitectureFindings {
    if path.extension().is_some_and(|extension| extension == "pyi") {
        return FileArchitectureFindings::default();
    }
    let Some(module_path) = layout.python_architecture_module_of_path(path) else {
        return FileArchitectureFindings::default();
    };
    let mut findings = FileArchitectureFindings::default();

    let enclosing = resolve_python_component(&module_path, spec);
    if spec.require_component_coverage && enclosing.is_none() {
        collect_python_unmapped_findings(
            file,
            path,
            &module_path,
            spec,
            python_summary,
            &mut findings.unmapped_architecture_modules,
        );
    }

    let is_package_init = is_python_package_init(path);
    for import_ref in collect_python_import_references(file) {
        if (import_ref.in_function && !spec.check_local_imports)
            || (import_ref.in_type_checking && spec.ignore_type_checking_imports)
        {
            continue;
        }
        let Some(resolved) = resolve_python_import_target_with_layout(
            &module_path,
            is_package_init,
            &import_ref,
            layout,
            source,
        ) else {
            continue;
        };

        if let Some(ref enclosing_match) = enclosing
            && import_ref.level > 0
            && !resolved.stays_within_root(enclosing_match.matched_root)
        {
            let dependency = if resolved.module_path == ROOT_MODULE_TAG {
                resolved.full_target_path.clone()
            } else {
                resolved.module_path.clone()
            };
            findings
                .cross_component_relative_paths
                .push(CrossComponentRelativePathFinding {
                    span: import_ref.span,
                    dependency,
                    component: enclosing_match.component_name.to_owned(),
                });
        }

        if !is_first_party_python_import(&resolved.module_path, layout, source, spec) {
            if import_ref.level == 0 {
                let top_package = resolved
                    .module_path
                    .split(PYTHON_MODULE_SEPARATOR)
                    .next()
                    .unwrap_or(&resolved.module_path);
                check_python_external_owner(
                    &module_path,
                    top_package,
                    import_ref.span,
                    spec,
                    &mut findings.unowned_external_dependencies,
                );
            }
            continue;
        }

        let Some(ref from_match) = enclosing else {
            continue;
        };
        check_python_first_party_edge(
            &module_path,
            from_match,
            &resolved,
            import_ref.span,
            spec,
            &mut findings,
        );
    }

    deduplicate_findings(&mut findings);
    findings
}

fn deduplicate_findings(findings: &mut FileArchitectureFindings) {
    sort_and_dedup(&mut findings.undeclared_component_dependencies, |f| f.span);
    sort_and_dedup(&mut findings.sibling_module_dependencies, |f| f.span);
    sort_and_dedup(&mut findings.cross_component_relative_paths, |f| f.span);
    sort_and_dedup(&mut findings.unowned_external_dependencies, |f| f.span);
    sort_and_dedup(&mut findings.private_component_imports, |f| f.span);
    sort_and_dedup(&mut findings.unmapped_architecture_modules, |f| f.span);
    sort_and_dedup(&mut findings.invalid_architecture_components, |f| f.span);
}

fn sort_and_dedup<T: PartialEq>(items: &mut Vec<T>, span_of: fn(&T) -> SourceSpan) {
    items.sort_by_key(|item| {
        let span = span_of(item);
        (span.start, span.end)
    });
    items.dedup();
}

#[derive(Debug, Clone, Copy)]
struct MatchedPythonComponent<'a> {
    component_name: &'a str,
    matched_root: &'a str,
}

#[derive(Debug, Clone)]
struct ResolvedPythonImport {
    module_path: String,
    full_target_path: String,
    imported_member: Option<String>,
    shallowest_relative_ancestor: Option<String>,
}

impl ResolvedPythonImport {
    fn stays_within_root(&self, component_root: &str) -> bool {
        if component_root == ROOT_MODULE_TAG || component_root == "*" {
            return self.shallowest_relative_ancestor.is_some();
        }
        let Some(ancestor) = self.shallowest_relative_ancestor.as_deref() else {
            return false;
        };
        let target_in_root = python_root_specificity(&self.module_path, component_root).is_some();
        let floor_base = component_root
            .strip_suffix(".**")
            .or_else(|| component_root.strip_suffix(".*"))
            .unwrap_or(component_root);
        let floor_in_root = python_root_specificity(ancestor, component_root).is_some()
            || (!component_root.ends_with(".*") && is_in_python_subtree(ancestor, floor_base));
        target_in_root && floor_in_root
    }
}

fn check_python_first_party_edge(
    module_path: &str,
    from_match: &MatchedPythonComponent<'_>,
    resolved: &ResolvedPythonImport,
    span: SourceSpan,
    spec: &ArchitectureSpec,
    findings: &mut FileArchitectureFindings,
) {
    let Some(target_match) = resolve_python_component(&resolved.module_path, spec) else {
        if spec.require_component_coverage {
            findings
                .undeclared_component_dependencies
                .push(UndeclaredComponentDependencyFinding {
                    span,
                    component: from_match.component_name.to_owned(),
                    dependency: resolved.full_target_path.clone(),
                });
        }
        return;
    };

    if from_match.component_name == target_match.component_name {
        if let Some(comp_spec) = spec.component_spec(from_match.component_name)
            && comp_spec.isolate_children
            && let Some(sibling_target) = python_sibling_violation(
                module_path,
                from_match.matched_root,
                &resolved.module_path,
                target_match.matched_root,
            )
        {
            findings
                .sibling_module_dependencies
                .push(SiblingModuleDependencyFinding {
                    span,
                    module: module_path.to_owned(),
                    dependency: sibling_target,
                    component: from_match.component_name.to_owned(),
                });
        }
        return;
    }

    let is_allowed = spec
        .allowed_dependencies
        .get(from_match.component_name)
        .is_some_and(|set| set.contains(target_match.component_name));
    if !is_allowed {
        findings
            .undeclared_component_dependencies
            .push(UndeclaredComponentDependencyFinding {
                span,
                component: from_match.component_name.to_owned(),
                dependency: target_match.component_name.to_owned(),
            });
    } else if let Some(target_spec) = spec.component_spec(target_match.component_name)
        && let Some(ref exports) = target_spec.public_exports
        && !is_python_import_allowed_by_exports(target_match.matched_root, resolved, exports)
    {
        let symbol_name = if resolved.imported_member.as_deref() == Some("*")
            || (resolved.module_path == target_match.matched_root
                && resolved.imported_member.is_none())
        {
            None
        } else {
            Some(resolved.imported_member.clone().unwrap_or_else(|| {
                resolved
                    .full_target_path
                    .rsplit(PYTHON_MODULE_SEPARATOR)
                    .next()
                    .unwrap_or(&resolved.full_target_path)
                    .to_owned()
            }))
        };
        findings
            .private_component_imports
            .push(PrivateComponentImportFinding {
                span,
                component: from_match.component_name.to_owned(),
                name: symbol_name,
                module: target_match.matched_root.to_owned(),
                dependency: target_match.component_name.to_owned(),
            });
    }
}

fn resolve_python_component<'a>(
    module_path: &str,
    spec: &'a ArchitectureSpec,
) -> Option<MatchedPythonComponent<'a>> {
    let mut best: Option<(SpecificityRank, MatchedPythonComponent<'a>)> = None;
    for component in &spec.components {
        for root in &component.roots {
            if let Some(rank) = python_root_specificity(module_path, root) {
                let candidate = MatchedPythonComponent {
                    component_name: component.name.as_str(),
                    matched_root: root.as_str(),
                };
                if best.as_ref().is_none_or(|(best_rank, _)| rank > *best_rank) {
                    best = Some((rank, candidate));
                }
            }
        }
    }
    best.map(|(_, matched)| matched)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SpecificityRank {
    kind_rank: usize,
    segment_count: usize,
}

fn python_root_specificity(module_path: &str, root_pattern: &str) -> Option<SpecificityRank> {
    if root_pattern == ROOT_MODULE_TAG {
        return (module_path == ROOT_MODULE_TAG || !module_path.contains(PYTHON_MODULE_SEPARATOR))
            .then_some(SpecificityRank {
                kind_rank: PREFIX_MATCH_RANK,
                segment_count: 1,
            });
    }
    if root_pattern == "*" {
        return (!module_path.contains(PYTHON_MODULE_SEPARATOR)).then_some(SpecificityRank {
            kind_rank: 1,
            segment_count: 1,
        });
    }
    if let Some(prefix) = root_pattern.strip_suffix(".**") {
        let segment_count = prefix.split(PYTHON_MODULE_SEPARATOR).count();
        return (module_path != prefix && is_in_python_subtree(module_path, prefix)).then_some(
            SpecificityRank {
                kind_rank: 2,
                segment_count,
            },
        );
    }
    if let Some(prefix) = root_pattern.strip_suffix(".*") {
        let segment_count = prefix.split(PYTHON_MODULE_SEPARATOR).count();
        let rest = module_path
            .strip_prefix(prefix)?
            .strip_prefix(PYTHON_MODULE_SEPARATOR)?;
        return (!rest.contains(PYTHON_MODULE_SEPARATOR)).then_some(SpecificityRank {
            kind_rank: 2,
            segment_count: segment_count + 1,
        });
    }
    if is_in_python_subtree(module_path, root_pattern) {
        let segment_count = root_pattern.split(PYTHON_MODULE_SEPARATOR).count();
        let exact_bonus = usize::from(module_path == root_pattern);
        return Some(SpecificityRank {
            kind_rank: PREFIX_MATCH_RANK + exact_bonus,
            segment_count,
        });
    }
    None
}

fn resolve_python_import_target_with_layout(
    enclosing_module: &str,
    is_package_init: bool,
    import_ref: &PythonImportReference,
    layout: &ProjectLayout,
    source: &dyn FileSource,
) -> Option<ResolvedPythonImport> {
    if import_ref.level == 0 {
        let base_module = import_ref.module.as_deref()?;
        return Some(resolve_imported_member_on_base(
            base_module.to_owned(),
            import_ref.imported_name.as_deref(),
            Some(base_module.to_owned()),
            layout,
            source,
        ));
    }

    let resolved_package =
        resolve_relative_python_package(enclosing_module, is_package_init, import_ref.level)?;
    let shallowest = Some(resolved_package.shallowest_ancestor);
    let Some(base_module) = (match (
        resolved_package.package_prefix,
        import_ref.module.as_deref(),
    ) {
        (Some(prefix), Some(suffix)) => Some(format!("{prefix}.{suffix}")),
        (Some(prefix), None) => Some(prefix),
        (None, Some(suffix)) => Some(suffix.to_owned()),
        (None, None) => None,
    }) else {
        let member = import_ref.imported_name.as_ref()?;
        if layout.python_module_file(member, source).is_some()
            || layout.is_python_namespace_dir(member, source)
        {
            return Some(ResolvedPythonImport {
                module_path: member.clone(),
                full_target_path: member.clone(),
                imported_member: None,
                shallowest_relative_ancestor: shallowest,
            });
        }
        return Some(ResolvedPythonImport {
            module_path: ROOT_MODULE_TAG.to_owned(),
            full_target_path: member.clone(),
            imported_member: Some(member.clone()),
            shallowest_relative_ancestor: shallowest,
        });
    };

    Some(resolve_imported_member_on_base(
        base_module,
        import_ref.imported_name.as_deref(),
        shallowest,
        layout,
        source,
    ))
}

fn resolve_imported_member_on_base(
    base_module: String,
    imported_name: Option<&str>,
    shallowest_relative_ancestor: Option<String>,
    layout: &ProjectLayout,
    source: &dyn FileSource,
) -> ResolvedPythonImport {
    if imported_name == Some("*") {
        return ResolvedPythonImport {
            module_path: base_module.clone(),
            full_target_path: base_module,
            imported_member: Some("*".to_owned()),
            shallowest_relative_ancestor,
        };
    }
    if let Some(member) = imported_name {
        let candidate_submodule = format!("{base_module}.{member}");
        if layout
            .python_module_file(&candidate_submodule, source)
            .is_some()
            || layout.is_python_namespace_dir(&candidate_submodule, source)
        {
            return ResolvedPythonImport {
                module_path: candidate_submodule.clone(),
                full_target_path: candidate_submodule,
                imported_member: None,
                shallowest_relative_ancestor,
            };
        }
        return ResolvedPythonImport {
            module_path: base_module,
            full_target_path: candidate_submodule,
            imported_member: Some(member.to_owned()),
            shallowest_relative_ancestor,
        };
    }
    ResolvedPythonImport {
        module_path: base_module.clone(),
        full_target_path: base_module,
        imported_member: None,
        shallowest_relative_ancestor,
    }
}

fn is_python_import_allowed_by_exports(
    target_root: &str,
    resolved: &ResolvedPythonImport,
    exports: &[ExportPattern],
) -> bool {
    if resolved.module_path == target_root {
        return match resolved.imported_member.as_deref() {
            None | Some("*") => exports
                .iter()
                .any(|export_pattern| export_pattern.matches("*")),
            Some(member) => exports
                .iter()
                .any(|export_pattern| export_pattern.matches(member)),
        };
    }
    let Some(subpath) = resolved
        .full_target_path
        .strip_prefix(target_root)
        .and_then(|rest| rest.strip_prefix(PYTHON_MODULE_SEPARATOR))
    else {
        return false;
    };
    if resolved.imported_member.as_deref() == Some("*") {
        let star_probe = format!("{subpath}.*");
        return exports
            .iter()
            .any(|export_pattern| export_pattern.matches(&star_probe));
    }
    exports
        .iter()
        .any(|export_pattern| export_pattern.matches(subpath))
}

fn is_first_party_python_import(
    resolved_module_path: &str,
    layout: &ProjectLayout,
    source: &dyn FileSource,
    spec: &ArchitectureSpec,
) -> bool {
    let top_package = resolved_module_path
        .split(PYTHON_MODULE_SEPARATOR)
        .next()
        .unwrap_or(resolved_module_path);
    layout.python_module_file(top_package, source).is_some()
        || layout.is_python_namespace_dir(top_package, source)
        || spec.components.iter().any(|comp| {
            comp.roots
                .iter()
                .any(|root| root.split(PYTHON_MODULE_SEPARATOR).next() == Some(top_package))
        })
}

fn collect_python_unmapped_findings(
    file: &ParsedFile,
    path: &Path,
    module_path: &str,
    spec: &ArchitectureSpec,
    cached_summary: Option<&PythonFileSummary>,
    out: &mut Vec<UnmappedArchitectureModuleFinding>,
) {
    let owned_summary;
    #[expect(
        clippy::option_if_let_else,
        reason = "closure cannot return reference to outer delayed-init binding"
    )]
    let py_summary = if let Some(existing) = cached_summary {
        existing
    } else {
        owned_summary = summarize_python_file(file);
        &owned_summary
    };
    if py_summary.non_routing_items.is_empty() {
        let is_pure_package_router = is_python_package_init(path)
            && (module_path == ROOT_MODULE_TAG
                || spec.components.iter().any(|comp| {
                    comp.roots
                        .iter()
                        .any(|root| is_in_python_subtree(root, module_path))
                }));
        if !is_pure_package_router {
            out.push(UnmappedArchitectureModuleFinding {
                span: default_file_header_span(file),
                name: module_path.to_owned(),
                module: module_path.to_owned(),
            });
        }
    } else {
        for item in &py_summary.non_routing_items {
            out.push(UnmappedArchitectureModuleFinding {
                span: item.span,
                name: item.name.clone(),
                module: module_path.to_owned(),
            });
        }
    }
}

fn default_file_header_span(file: &ParsedFile) -> SourceSpan {
    let source = file.source_text();
    let end = source.lines().next().map_or(0, str::len).min(source.len());
    SourceSpan::new(0, end)
}

fn check_python_external_owner(
    module_path: &str,
    top_package: &str,
    span: SourceSpan,
    spec: &ArchitectureSpec,
    out: &mut Vec<UnownedExternalDependencyFinding>,
) {
    if let Some(allowed_prefixes) = spec.external_package_owners.get(top_package) {
        let is_allowed = allowed_prefixes
            .iter()
            .any(|prefix| is_in_python_subtree(module_path, prefix));
        if !is_allowed {
            out.push(UnownedExternalDependencyFinding {
                span,
                module: module_path.to_owned(),
                dependency: top_package.to_owned(),
                owners: allowed_prefixes.clone(),
            });
        }
    }
}

fn python_sibling_violation(
    source_module: &str,
    source_root: &str,
    target_module: &str,
    target_root: &str,
) -> Option<String> {
    if source_root != target_root {
        return Some(target_root.to_owned());
    }
    let source_child = direct_child_in_python_root(source_module, source_root)?;
    let target_child = direct_child_in_python_root(target_module, target_root)?;
    (source_child != target_child).then(|| format!("{target_root}.{target_child}"))
}

fn direct_child_in_python_root<'a>(module_path: &'a str, root: &str) -> Option<&'a str> {
    let rest = module_path
        .strip_prefix(root)?
        .strip_prefix(PYTHON_MODULE_SEPARATOR)?;
    let child = rest.split(PYTHON_MODULE_SEPARATOR).next()?;
    (!child.is_empty()).then_some(child)
}

fn is_in_python_subtree(module_path: &str, prefix: &str) -> bool {
    module_path == prefix
        || module_path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with(PYTHON_MODULE_SEPARATOR))
}

fn is_rust_path_allowed_by_exports(
    target_root: &str,
    canonical_path: &str,
    is_glob: bool,
    exports: &[ExportPattern],
) -> bool {
    if canonical_path == target_root {
        return exports.iter().any(|export| export.matches("*"));
    }
    let Some(relative_symbol) = canonical_path
        .strip_prefix(target_root)
        .and_then(|rest| rest.strip_prefix(RUST_MODULE_SEPARATOR))
    else {
        return false;
    };
    if is_glob {
        let glob_probe = format!("{relative_symbol}::*");
        return exports.iter().any(|export| export.matches(&glob_probe));
    }
    exports.iter().any(|export| export.matches(relative_symbol))
}

fn build_architecture_spec(
    project_root: &Path,
    config: &ArchitectureConfig,
    from_rust_macros: bool,
    rust_headers: &BTreeMap<String, Arc<RustFileSummary>>,
) -> Result<ArchitectureSpec, ArchitectureError> {
    let mut seen = HashSet::with_capacity(config.components.len());
    let mut components = Vec::with_capacity(config.components.len());

    for entry in &config.components {
        for dependency in &entry.depends_on {
            if !seen.contains(dependency.as_str()) {
                return Err(ArchitectureError::ForwardOrUnknownDependency {
                    component: entry.name.clone(),
                    dependency: dependency.clone(),
                });
            }
        }
        if !seen.insert(entry.name.clone()) {
            return Err(ArchitectureError::DuplicateComponent(entry.name.clone()));
        }
        let public_exports = entry.public_exports.as_ref().map(|patterns| {
            patterns
                .iter()
                .map(|pattern| ExportPattern::new(pattern))
                .collect()
        });
        components.push(ComponentSpec {
            name: entry.name.clone(),
            roots: entry.roots.clone(),
            depends_on: entry.depends_on.clone(),
            isolate_children: entry.isolate_children,
            owns_external: entry.owns_external.clone(),
            public_exports,
        });
    }

    let source_roots = if config.source_roots.is_empty() {
        vec![project_root.to_path_buf()]
    } else {
        config
            .source_roots
            .iter()
            .map(|relative_root| {
                if relative_root == "." || relative_root.is_empty() {
                    project_root.to_path_buf()
                } else {
                    project_root.join(relative_root)
                }
            })
            .collect()
    };

    let require_component_coverage =
        (!components.is_empty() || from_rust_macros) && config.require_component_coverage;
    Ok(finalize_architecture_spec(
        ArchitectureSpec {
            source_roots,
            transitive_dependencies: config.transitive_dependencies,
            require_component_coverage,
            check_local_imports: true,
            ignore_type_checking_imports: config.ignore_type_checking_imports,
            from_rust_macros,
            components,
            external_package_owners: BTreeMap::new(),
            component_roots: BTreeMap::new(),
            allowed_dependencies: HashMap::new(),
            component_by_name: HashMap::new(),
            known_components: HashSet::new(),
            rust_sibling_rules: Vec::new(),
        },
        rust_headers,
    ))
}

/// Synthesizes the component DAG of a crate that declares `define_architecture!` in its
/// `architecture` module: components and dependencies from `define_architecture!`, roots from the
/// modules declaring `architecture_component!(...)`.
fn architecture_config_from_rust_headers(
    rust_headers: &BTreeMap<String, Arc<RustFileSummary>>,
) -> ArchitectureConfig {
    let defined_entries: &[DefinedComponentEntry] = rust_headers
        .get(RUST_ARCHITECTURE_MODULE)
        .map_or(&[], |summary| summary.defined_architecture.as_slice());
    let defined_names: HashSet<&str> = defined_entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    let mut accepted_roots: BTreeMap<String, String> = BTreeMap::new();
    let mut roots_by_component: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for (module_path, summary) in rust_headers {
        let Some((_, component_name)) = summary
            .architecture_components
            .iter()
            .find(|(span, _)| !summary.nested_architecture_components.contains(span))
        else {
            continue;
        };
        let has_ancestor_root = module_path
            .rsplit_once(RUST_MODULE_SEPARATOR)
            .and_then(|(parent, _)| resolve_inherited_component_with_root(parent, &accepted_roots))
            .is_some();
        if !defined_names.contains(component_name.as_str()) || has_ancestor_root {
            continue;
        }
        accepted_roots.insert(module_path.clone(), component_name.clone());
        roots_by_component
            .entry(component_name.clone())
            .or_default()
            .push(module_path.clone());
    }

    let components = defined_entries
        .iter()
        .map(|entry| {
            let roots = roots_by_component.remove(&entry.name).unwrap_or_default();
            ArchitectureComponentConfig {
                name: entry.name.clone(),
                roots,
                depends_on: entry.depends_on.clone(),
                isolate_children: true,
                owns_external: entry.owns_external.clone(),
                public_exports: None,
            }
        })
        .collect();

    ArchitectureConfig {
        source_roots: vec!["src".to_owned()],
        transitive_dependencies: true,
        require_component_coverage: true,
        ignore_type_checking_imports: false,
        components,
    }
}

fn finalize_architecture_spec(
    mut spec: ArchitectureSpec,
    rust_headers: &BTreeMap<String, Arc<RustFileSummary>>,
) -> ArchitectureSpec {
    spec.external_package_owners = build_external_package_owners(&spec.components);
    spec.component_roots = build_component_root_map(&spec.components);
    spec.allowed_dependencies =
        build_allowed_dependency_map(&spec.components, spec.transitive_dependencies);
    spec.component_by_name = spec
        .components
        .iter()
        .enumerate()
        .map(|(index, component)| (component.name.clone(), index))
        .collect();
    spec.known_components = spec
        .components
        .iter()
        .map(|component| component.name.clone())
        .collect();
    spec.rust_sibling_rules = build_rust_sibling_isolation_rules(rust_headers, &spec.components);
    spec
}

fn build_external_package_owners(components: &[ComponentSpec]) -> BTreeMap<String, Vec<String>> {
    let mut owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for component in components {
        for package in &component.owns_external {
            let allowed = owners.entry(package.clone()).or_default();
            for root in &component.roots {
                if !allowed.contains(root) {
                    allowed.push(root.clone());
                }
            }
        }
    }
    owners
}

fn build_component_root_map(components: &[ComponentSpec]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for component in components {
        for root in &component.roots {
            map.insert(root.clone(), component.name.clone());
        }
    }
    map
}

fn build_allowed_dependency_map(
    components: &[ComponentSpec],
    transitive_dependencies: bool,
) -> HashMap<String, HashSet<String>> {
    let direct: HashMap<&str, HashSet<&str>> = components
        .iter()
        .map(|component| {
            let dependencies: HashSet<&str> =
                component.depends_on.iter().map(String::as_str).collect();
            (component.name.as_str(), dependencies)
        })
        .collect();
    if !transitive_dependencies {
        return direct
            .into_iter()
            .map(|(name, set)| {
                (
                    name.to_owned(),
                    set.into_iter().map(str::to_owned).collect(),
                )
            })
            .collect();
    }
    let mut reachable_by_component = HashMap::with_capacity(components.len());
    for component in components {
        let mut reachable = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(component.name.as_str());
        while let Some(current) = queue.pop_front() {
            if let Some(dependencies) = direct.get(current) {
                for &dependency in dependencies {
                    if reachable.insert(dependency.to_owned()) {
                        queue.push_back(dependency);
                    }
                }
            }
        }
        reachable_by_component.insert(component.name.clone(), reachable);
    }
    reachable_by_component
}

fn build_rust_sibling_isolation_rules(
    rust_headers: &BTreeMap<String, Arc<RustFileSummary>>,
    components: &[ComponentSpec],
) -> Vec<SiblingIsolationRule> {
    let mut rules = Vec::new();
    for component in components {
        if !component.isolate_children {
            continue;
        }
        if component.roots.len() > 1 {
            for root in &component.roots {
                let forbidden_siblings: Vec<String> = component
                    .roots
                    .iter()
                    .filter(|other| *other != root)
                    .cloned()
                    .collect();
                rules.push(SiblingIsolationRule {
                    component_name: component.name.clone(),
                    subject_prefix: root.clone(),
                    forbidden_siblings,
                });
            }
        }
        for root in &component.roots {
            let Some(root_summary) = rust_headers.get(root) else {
                continue;
            };
            let direct_children: Vec<String> = root_summary
                .external_mods
                .iter()
                .filter(|declaration| !declaration.is_private)
                .map(|declaration| format!("{root}::{}", declaration.name))
                .collect();
            if direct_children.len() > 1 {
                for child in &direct_children {
                    let forbidden_siblings: Vec<String> = direct_children
                        .iter()
                        .filter(|other| *other != child)
                        .cloned()
                        .collect();
                    rules.push(SiblingIsolationRule {
                        component_name: component.name.clone(),
                        subject_prefix: child.clone(),
                        forbidden_siblings,
                    });
                }
            }
        }
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::super::source::MemoryFileSource;
    use super::*;

    #[test]
    fn rust_leading_colon_paths_enforce_external_owner_and_component_dependencies() {
        let root = Path::new("/repo");
        let app_source = indoc::indoc! {r"
            use ::ra_ap_syntax::SyntaxNode;
            use ::demo::secret::Token;
        "};
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                (
                    "Cargo.toml",
                    indoc::indoc! {r#"
                        [package]
                        name = "demo"
                        version = "0.1.0"
                    "#},
                ),
                (
                    "src/lib.rs",
                    indoc::indoc! {r"
                        mod ast;
                        mod secret;
                        mod app;
                    "},
                ),
                ("src/ast.rs", ""),
                ("src/secret.rs", "pub struct Token;\n"),
                ("src/app.rs", app_source),
            ],
        );
        let config: ArchitectureConfig = toml::from_str(indoc::indoc! {r#"
            [[components]]
            name = "Ast"
            roots = ["ast"]
            owns-external = ["ra_ap_syntax"]

            [[components]]
            name = "Secret"
            roots = ["secret"]

            [[components]]
            name = "App"
            roots = ["app"]
            depends-on = ["Ast"]
        "#})
        .unwrap();
        let spec = resolve_architecture_spec(root, Some(&config), &BTreeMap::new())
            .unwrap()
            .unwrap();
        let layout = ProjectLayout::discover(root, &source, &[]);
        let parsed = ParsedFile::rust(app_source);
        let findings =
            evaluate_rust_file_architecture(&parsed, &root.join("src/app.rs"), &layout, &spec);

        assert_eq!(findings.unowned_external_dependencies.len(), 1);
        assert_eq!(
            findings.unowned_external_dependencies[0].dependency,
            "ra_ap_syntax"
        );
        assert_eq!(findings.undeclared_component_dependencies.len(), 1);
        assert_eq!(
            findings.undeclared_component_dependencies[0].dependency,
            "Secret"
        );
    }
}
