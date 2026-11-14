//! Language-agnostic architectural component specification (`ArchitectureSpec`), finding models,
//! and component DAG builders.

use super::config::{
    ArchitectureConfig, ArchitectureConfigError, ArchitectureSubmoduleConfig, ProjectTomlError,
    compile_export_glob, validate_component_dag,
};
use crate::diagnostic::SourceSpan;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

/// Synthetic module identifier representing a root module directly inside a configured source root.
pub(in crate::code_lint::project) const ROOT_MODULE_TAG: &str = "<root>";

/// Errors raised when loading or validating a project's [`ArchitectureSpec`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArchitectureError {
    /// Both `.omnicoach.toml` `[[architecture.components]]` and in-source `define_architecture!` exist.
    #[error(
        "conflicting architecture declarations: both `.omnicoach.toml` `[[architecture.components]]` and Rust `define_architecture!` are present"
    )]
    ConflictingDeclarations,
    /// Invalid `[architecture]` configuration or synthesized component DAG.
    #[error(transparent)]
    InvalidConfig(#[from] ArchitectureConfigError),
    /// Invalid TOML syntax or unknown keys in `.omnicoach.toml`.
    #[error(transparent)]
    InvalidProjectToml(#[from] toml::de::Error),
}

impl From<ProjectTomlError> for ArchitectureError {
    fn from(error: ProjectTomlError) -> Self {
        match error {
            ProjectTomlError::Toml(toml_error) => Self::InvalidProjectToml(toml_error),
            ProjectTomlError::Architecture(architecture_error) => {
                Self::InvalidConfig(architecture_error)
            }
        }
    }
}

/// Validated, language-agnostic architectural specification for a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchitectureSpec {
    /// Whether unmapped production files are flagged.
    pub(in crate::code_lint::project) require_component_coverage: bool,
    /// Whether imports inside `if TYPE_CHECKING:` blocks are exempt.
    pub(in crate::code_lint::project) ignore_type_checking_imports: bool,
    /// True when component roots were synthesized from in-source `define_architecture!` and
    /// `architecture_component!(...)` module headers rather than `.omnicoach.toml`.
    pub(in crate::code_lint::project) from_macro_declarations: bool,
    /// Topologically ordered components.
    pub components: Vec<ComponentSpec>,
    external_package_owners: BTreeMap<String, ExternalPackageOwner>,
    component_roots: BTreeMap<String, String>,
    allowed_dependencies: HashMap<String, HashSet<String>>,
    component_by_name: HashMap<String, usize>,
}

impl ArchitectureSpec {
    /// Canonical root module paths mapped to their owning component name.
    #[must_use]
    pub const fn component_roots(&self) -> &BTreeMap<String, String> {
        &self.component_roots
    }

    /// Returns true if `from_component` is permitted to depend on `target_component` under this
    /// specification (honoring `transitive_dependencies`).
    #[must_use]
    pub fn allows_dependency(&self, from_component: &str, target_component: &str) -> bool {
        self.allowed_dependencies
            .get(from_component)
            .is_some_and(|allowed| allowed.contains(target_component))
    }

    #[must_use]
    pub(in crate::code_lint::project) fn component_spec(
        &self,
        name: &str,
    ) -> Option<&ComponentSpec> {
        self.component_by_name
            .get(name)
            .and_then(|&index| self.components.get(index))
    }

    #[must_use]
    pub(in crate::code_lint::project) fn contains_component(&self, name: &str) -> bool {
        self.component_by_name.contains_key(name)
    }

    /// Appends an [`UnownedExternalDependencyFinding`] to `out` for every encapsulated external
    /// package matched by `matches_package` when `enclosing_component` is not among its owners.
    pub(in crate::code_lint::project) fn collect_unowned_external_dependencies(
        &self,
        module_path: &str,
        enclosing_component: Option<&str>,
        span: SourceSpan,
        matches_package: impl Fn(&str) -> bool,
        out: &mut Vec<UnownedExternalDependencyFinding>,
    ) {
        for (package_name, owner) in &self.external_package_owners {
            let is_allowed_owner = enclosing_component.is_some_and(|component| {
                owner.components.iter().any(|allowed| allowed == component)
            });
            if !is_allowed_owner && matches_package(package_name) {
                out.push(UnownedExternalDependencyFinding {
                    span,
                    module: module_path.to_owned(),
                    dependency: package_name.clone(),
                    owners: owner.display_owners.clone(),
                });
            }
        }
    }
}

/// Validated intra-component submodule entry in [`ComponentSpec`] or a parent [`SubmoduleSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmoduleSpec {
    /// Submodule identifier relative to its parent (`"foundation"`, `"source"`).
    pub name: String,
    /// Precomputed allowed sibling submodule names (honoring `transitive_dependencies`).
    allowed_siblings: HashSet<String>,
    /// Recursive submodule DAG inside this submodule.
    pub submodules: Vec<Self>,
}

impl SubmoduleSpec {
    /// Returns true if this submodule is permitted to depend on `sibling` in the same parent scope.
    #[must_use]
    pub fn allows_sibling_dependency(&self, sibling: &str) -> bool {
        self.allowed_siblings.contains(sibling)
    }
}

/// Validated component entry in [`ArchitectureSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentSpec {
    /// Component identifier.
    pub name: String,
    /// Canonical root module paths.
    pub roots: Vec<String>,
    /// Direct allowed component names.
    pub(in crate::code_lint::project) depends_on: Vec<String>,
    /// Whether sibling subtrees inside this component are isolated from each other.
    pub(in crate::code_lint::project) isolate_children: bool,
    /// External third-party crates or packages encapsulated by this component.
    owns_external: Vec<String>,
    /// Optional public interface export rules per root module (`None` = unrestricted submodules).
    pub(in crate::code_lint::project) public_exports: Option<Vec<ExportPattern>>,
    /// Optional recursive submodule DAG inside this component.
    pub submodules: Vec<SubmoduleSpec>,
}

impl ComponentSpec {
    /// Returns the declared [`SubmoduleSpec`] slice at `rel_subpath` beneath a root of this
    /// component (`None` at the component root itself; `Some("foundation::source")` at a nested
    /// scope split by `separator`), or `&[]` when no submodules are declared at that scope.
    #[must_use]
    pub(in crate::code_lint::project) fn declared_submodules_at(
        &self,
        rel_subpath: Option<&str>,
        separator: &str,
    ) -> &[SubmoduleSpec] {
        let Some(subpath) = rel_subpath else {
            return &self.submodules;
        };
        subpath
            .split(separator)
            .try_fold(self.submodules.as_slice(), |current, segment| {
                current
                    .iter()
                    .find(|entry| entry.name == segment)
                    .map(|sub| sub.submodules.as_slice())
            })
            .unwrap_or(&[])
    }

    /// Returns true when `rel_subpath` beneath a root of this component is permitted by its
    /// declared `submodules` DAG (treating scopes with an empty `submodules` list as leaf scopes
    /// that allow arbitrary child modules).
    #[must_use]
    pub(in crate::code_lint::project) fn is_declared_submodule_path(
        &self,
        rel_subpath: Option<&str>,
        separator: &str,
    ) -> bool {
        let Some(subpath) = rel_subpath else {
            return true;
        };
        let mut current = self.submodules.as_slice();
        for segment in subpath.split(separator) {
            if current.is_empty() {
                return true;
            }
            let Some(sub) = current.iter().find(|entry| entry.name == segment) else {
                return false;
            };
            current = &sub.submodules;
        }
        true
    }

    /// Evaluates whether a reference from `source_rel` to `target_rel` (both relative to
    /// `root_prefix` and separated by `separator`) violates this component's recursive
    /// `submodules` DAG at their lowest common ancestor scope, returning the forbidden sibling
    /// prefix when disallowed.
    #[must_use]
    pub(in crate::code_lint::project) fn submodule_dag_violation(
        &self,
        source_rel: &str,
        target_rel: &str,
        root_prefix: &str,
        separator: &str,
    ) -> Option<String> {
        let mut current_prefix = root_prefix.to_owned();
        let mut current_submodules = self.submodules.as_slice();
        for (src_seg, tgt_seg) in source_rel.split(separator).zip(target_rel.split(separator)) {
            if src_seg != tgt_seg {
                if current_submodules.is_empty() {
                    return None;
                }
                let allowed = current_submodules
                    .iter()
                    .find(|sub| sub.name == src_seg)
                    .is_some_and(|sub| sub.allows_sibling_dependency(tgt_seg));
                return (!allowed).then(|| {
                    if current_prefix.is_empty() {
                        tgt_seg.to_owned()
                    } else {
                        format!("{current_prefix}{separator}{tgt_seg}")
                    }
                });
            }
            current_prefix = if current_prefix.is_empty() {
                src_seg.to_owned()
            } else {
                format!("{current_prefix}{separator}{src_seg}")
            };
            current_submodules = current_submodules
                .iter()
                .find(|sub| sub.name == src_seg)
                .map_or(&[], |sub| sub.submodules.as_slice());
        }
        None
    }
}

/// Export allowlist glob pattern on a component root.
#[derive(Debug, Clone)]
pub(in crate::code_lint::project) struct ExportPattern {
    matcher: globset::GlobMatcher,
}

impl PartialEq for ExportPattern {
    fn eq(&self, other: &Self) -> bool {
        self.matcher.glob().glob() == other.matcher.glob().glob()
    }
}

impl Eq for ExportPattern {}

impl ExportPattern {
    #[must_use]
    pub(in crate::code_lint::project) fn matches(&self, candidate: &str) -> bool {
        self.matcher.is_match(candidate)
    }

    const fn from_matcher(matcher: globset::GlobMatcher) -> Self {
        Self { matcher }
    }
}

/// Violation finding for `undeclared-component-dependency`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint) struct UndeclaredComponentDependencyFinding {
    /// AST byte span of the offending import or path reference.
    pub span: SourceSpan,
    /// Enclosing component name.
    pub component: String,
    /// Referenced target component name.
    pub dependency: String,
}

/// Violation finding for `sibling-module-dependency`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint) struct SiblingModuleDependencyFinding {
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
pub(in crate::code_lint) struct CrossComponentRelativePathFinding {
    /// AST byte span of the offending relative import or path reference.
    pub span: SourceSpan,
    /// Resolved target module path outside the component root.
    pub dependency: String,
    /// Enclosing component name.
    pub component: String,
}

/// Violation finding for `unowned-external-dependency`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint) struct UnownedExternalDependencyFinding {
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
pub(in crate::code_lint) struct PrivateComponentImportFinding {
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
pub(in crate::code_lint) struct UnmappedArchitectureModuleFinding {
    /// AST byte span of the offending declaration or module header.
    pub span: SourceSpan,
    /// Unmapped item or declaration name.
    pub name: String,
    /// Unmapped or impure router module path.
    pub module: String,
}

/// Reason an `architecture_component!(...)` macro call in source code has no effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint) enum InvalidArchitectureComponentKind {
    /// No `define_architecture!` declaration exists in the crate's `architecture` module.
    MissingDefineArchitecture,
    /// `.omnicoach.toml` declares `[[architecture.components]]`, so component macros are inactive.
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
pub(in crate::code_lint) struct InvalidArchitectureComponentFinding {
    /// AST byte span of the ignored `architecture_component!(...)` macro call.
    pub span: SourceSpan,
    /// Raw component argument inside `architecture_component!(...)`.
    pub component: String,
    /// Enclosing module path.
    pub module: String,
    /// Structured reason the declaration has no effect.
    pub kind: InvalidArchitectureComponentKind,
}

/// Violation finding for `reexported-public-item`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint) struct ReexportedPublicItemFinding {
    /// AST byte span of the offending `pub use` or `pub(...) use` item.
    pub span: SourceSpan,
    /// Source text of the re-export item.
    pub expression: String,
    /// Enclosing module path.
    pub module: String,
}

/// All architectural conformance findings evaluated for a single source file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(in crate::code_lint) struct FileArchitectureFindings {
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
    /// Findings for `invalid-architecture-component` (Rust macro declarations).
    pub invalid_architecture_components: Vec<InvalidArchitectureComponentFinding>,
    /// Findings for `reexported-public-item` (Rust `pub use` / `#[macro_export]`).
    pub reexported_public_items: Vec<ReexportedPublicItemFinding>,
}

/// Validates `config` and constructs a language-agnostic [`ArchitectureSpec`] with precomputed
/// reachability maps.
///
/// # Errors
///
/// Returns [`ArchitectureError`] if `config` fails validation or contains an invalid export glob.
pub(in crate::code_lint::project) fn build_architecture_spec(
    config: &ArchitectureConfig,
    from_macro_declarations: bool,
) -> Result<ArchitectureSpec, ArchitectureError> {
    validate_component_dag(&config.components)?;
    let mut components = Vec::with_capacity(config.components.len());
    for entry in &config.components {
        let public_exports = match &entry.public_exports {
            Some(patterns) => Some(
                patterns
                    .iter()
                    .map(|pattern| {
                        compile_export_glob(&entry.name, pattern).map(ExportPattern::from_matcher)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            None => None,
        };
        components.push(ComponentSpec {
            name: entry.name.clone(),
            roots: entry.roots.clone(),
            depends_on: entry.depends_on.clone(),
            isolate_children: entry.isolate_children,
            owns_external: entry
                .owns_external
                .iter()
                .map(|package| normalize_external_package_name(package))
                .collect(),
            public_exports,
            submodules: build_submodule_specs(&entry.submodules, config.transitive_dependencies),
        });
    }

    let require_component_coverage =
        (!components.is_empty() || from_macro_declarations) && config.require_component_coverage;
    let external_package_owners = build_external_package_owners(&components);
    let component_roots = build_component_root_map(&components);
    let allowed_dependencies =
        build_allowed_dependency_map(&components, config.transitive_dependencies);
    let component_by_name = components
        .iter()
        .enumerate()
        .map(|(index, component)| (component.name.clone(), index))
        .collect();
    Ok(ArchitectureSpec {
        require_component_coverage,
        ignore_type_checking_imports: config.ignore_type_checking_imports,
        from_macro_declarations,
        components,
        external_package_owners,
        component_roots,
        allowed_dependencies,
        component_by_name,
    })
}

pub(in crate::code_lint::project) fn deduplicate_findings(findings: &mut FileArchitectureFindings) {
    sort_and_dedup(&mut findings.undeclared_component_dependencies, |f| f.span);
    sort_and_dedup(&mut findings.sibling_module_dependencies, |f| f.span);
    sort_and_dedup(&mut findings.cross_component_relative_paths, |f| f.span);
    sort_and_dedup(&mut findings.unowned_external_dependencies, |f| f.span);
    sort_and_dedup(&mut findings.private_component_imports, |f| f.span);
    sort_and_dedup(&mut findings.unmapped_architecture_modules, |f| f.span);
    sort_and_dedup(&mut findings.invalid_architecture_components, |f| f.span);
    sort_and_dedup(&mut findings.reexported_public_items, |f| f.span);
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ExternalPackageOwner {
    components: Vec<String>,
    display_owners: Vec<String>,
}

fn build_external_package_owners(
    components: &[ComponentSpec],
) -> BTreeMap<String, ExternalPackageOwner> {
    let mut owners: BTreeMap<String, ExternalPackageOwner> = BTreeMap::new();
    for component in components {
        let has_wildcard_or_empty_roots = component.roots.is_empty()
            || component
                .roots
                .iter()
                .any(|root| root.is_empty() || root == ROOT_MODULE_TAG || root.contains('*'));
        for package in &component.owns_external {
            let entry = owners.entry(package.clone()).or_default();
            if !entry.components.contains(&component.name) {
                entry.components.push(component.name.clone());
            }
            if has_wildcard_or_empty_roots {
                if !entry.display_owners.contains(&component.name) {
                    entry.display_owners.push(component.name.clone());
                }
            } else {
                for root in &component.roots {
                    if !entry.display_owners.contains(root) {
                        entry.display_owners.push(root.clone());
                    }
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

fn build_submodule_specs(
    entries: &[ArchitectureSubmoduleConfig],
    transitive_dependencies: bool,
) -> Vec<SubmoduleSpec> {
    let nodes: Vec<(&str, &[String])> = entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.depends_on.as_slice()))
        .collect();
    let mut allowed_by_name = compute_allowed_dependency_map(&nodes, transitive_dependencies);
    entries
        .iter()
        .map(|entry| SubmoduleSpec {
            name: entry.name.clone(),
            allowed_siblings: allowed_by_name.remove(&entry.name).unwrap_or_default(),
            submodules: build_submodule_specs(&entry.submodules, transitive_dependencies),
        })
        .collect()
}

fn build_allowed_dependency_map(
    components: &[ComponentSpec],
    transitive_dependencies: bool,
) -> HashMap<String, HashSet<String>> {
    let nodes: Vec<(&str, &[String])> = components
        .iter()
        .map(|component| (component.name.as_str(), component.depends_on.as_slice()))
        .collect();
    compute_allowed_dependency_map(&nodes, transitive_dependencies)
}

fn compute_allowed_dependency_map(
    nodes: &[(&str, &[String])],
    transitive_dependencies: bool,
) -> HashMap<String, HashSet<String>> {
    if !transitive_dependencies {
        return nodes
            .iter()
            .map(|(name, depends_on)| ((*name).to_owned(), depends_on.iter().cloned().collect()))
            .collect();
    }
    let direct: HashMap<&str, HashSet<&str>> = nodes
        .iter()
        .map(|(name, depends_on)| {
            let dependencies: HashSet<&str> = depends_on.iter().map(String::as_str).collect();
            (*name, dependencies)
        })
        .collect();
    let mut reachable_by_node = HashMap::with_capacity(nodes.len());
    for (name, _) in nodes {
        let mut reachable = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(*name);
        while let Some(current) = queue.pop_front() {
            if let Some(dependencies) = direct.get(current) {
                for &dependency in dependencies {
                    if reachable.insert(dependency.to_owned()) {
                        queue.push_back(dependency);
                    }
                }
            }
        }
        reachable_by_node.insert((*name).to_owned(), reachable);
    }
    reachable_by_node
}

fn normalize_external_package_name(raw: &str) -> String {
    raw.trim().replace('-', "_")
}

fn sort_and_dedup<T: PartialEq>(items: &mut Vec<T>, span_of: fn(&T) -> SourceSpan) {
    items.sort_by_key(|item| {
        let span = span_of(item);
        (span.start, span.end)
    });
    items.dedup();
}
