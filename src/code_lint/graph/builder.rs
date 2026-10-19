//! Alpha / experimental Layer L2 builder: transforms [`ScopeFacts`] and [`ProjectFacts`] into a
//! validated [`AbstractionGraph`] (`docs/dev/abstraction_graph/03_design.md` §4.2 and §5.1).

use super::{
    AbstractionGraph, DeclarationKind, DeclarationNode, EdgeReason, GraphParts, GroupKind,
    GroupNode, ModelError, ModuleFact, NodeId, OrderingEdge, ProjectFacts, ReferenceEdge,
    ReflexionStatus, SourceLocation, SummaryEdge,
};
use crate::code_lint::ast::{
    DeclarationScopeKind, ItemFacts, RelationKind, ScopeFacts, ScopeItemKind, Visibility, levels,
    strongly_connected_components,
};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Configurable hierarchy depth for whole-project abstraction graphs (D28, D29).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProjectDepth {
    /// `Component -> Module` (modules are leaf-like empty groups).
    ModuleOnly,
    /// `Component -> Module -> Public declarations` (public surface only).
    PublicSurface,
    /// `Component -> Module -> Owner/Shared/Unowned -> Declarations -> TypeMembers`.
    Full,
}

/// Builds a validated whole-project [`AbstractionGraph`] at the requested [`ProjectDepth`].
///
/// # Errors
///
/// Returns [`ModelError`] if the resulting graph violates any Layer L1 invariant.
pub fn build_project_graph(
    project: &ProjectFacts,
    depth: ProjectDepth,
) -> Result<AbstractionGraph, ModelError> {
    let mut accumulated = accumulate_project_modules(project, depth)?;
    append_cross_module_decl_edges(project, &mut accumulated);
    let comp_deps = build_component_dep_sets(project);
    let mod_to_comp: BTreeMap<&str, &str> = project
        .modules
        .iter()
        .map(|module| (module.module_path.as_str(), module.component.as_str()))
        .collect();

    let mod_summaries = build_module_summary_edges(project, &mod_to_comp, &comp_deps);
    assign_intra_component_module_levels(
        &mut accumulated.module_groups,
        &mod_to_comp,
        &mod_summaries,
    );

    let built_components = build_component_groups_and_summaries(
        project,
        &accumulated.module_groups,
        &mod_to_comp,
        &comp_deps,
    );

    let mut groups = built_components.groups;
    groups.extend(accumulated.module_groups);
    groups.extend(accumulated.inner_groups);

    let mut summary_edges = built_components.summary_edges;
    summary_edges.extend(mod_summaries);
    summary_edges.extend(accumulated.inner_summaries);
    summary_edges.sort();

    AbstractionGraph::new(GraphParts {
        title: project.title.clone(),
        declarations: accumulated.declarations,
        groups,
        ordering_edges: accumulated.ordering_edges,
        reference_edges: accumulated.reference_edges,
        summary_edges,
    })
}

const MODULE_SCOPE_ID_STRIDE: usize = 10_000;

struct AccumulatedProjectModules {
    declarations: Vec<DeclarationNode>,
    module_groups: Vec<GroupNode>,
    inner_groups: Vec<GroupNode>,
    ordering_edges: Vec<OrderingEdge>,
    reference_edges: Vec<ReferenceEdge>,
    inner_summaries: Vec<SummaryEdge>,
    decl_by_mod_and_name: BTreeMap<(String, String), NodeId>,
}

fn accumulate_project_modules(
    project: &ProjectFacts,
    depth: ProjectDepth,
) -> Result<AccumulatedProjectModules, ModelError> {
    let mut acc = AccumulatedProjectModules {
        declarations: Vec::new(),
        module_groups: Vec::new(),
        inner_groups: Vec::new(),
        ordering_edges: Vec::new(),
        reference_edges: Vec::new(),
        inner_summaries: Vec::new(),
        decl_by_mod_and_name: BTreeMap::new(),
    };

    for (module_idx, module_fact) in project.modules.iter().enumerate() {
        let mod_group_id = NodeId::new(format!("mod:{}", module_fact.module_path));
        let children = match depth {
            ProjectDepth::ModuleOnly => Vec::new(),
            ProjectDepth::PublicSurface => {
                extract_public_surface_module(module_idx, module_fact, &mut acc)?
            }
            ProjectDepth::Full => extract_full_module(module_idx, module_fact, &mut acc)?,
        };
        acc.module_groups.push(GroupNode {
            id: mod_group_id,
            label: module_fact.module_path.clone(),
            kind: GroupKind::Module {
                path: module_fact.module_path.clone(),
            },
            recursion_group: 0,
            level: 0,
            children,
        });
    }

    sort_declarations(&mut acc.declarations);
    acc.ordering_edges.sort();
    acc.reference_edges.sort();
    Ok(acc)
}

fn extract_public_surface_module(
    module_idx: usize,
    module_fact: &ModuleFact,
    acc: &mut AccumulatedProjectModules,
) -> Result<Vec<NodeId>, ModelError> {
    if module_fact.scopes.is_empty() {
        return Ok(Vec::new());
    }
    let module_graph = build_scope_graph(Path::new(&module_fact.path), &module_fact.scopes, 0)?;
    let prefix = format!("m{module_idx}:");
    let scope_offset = (module_idx + 1) * MODULE_SCOPE_ID_STRIDE;

    let mut kept_old_to_new = BTreeMap::new();
    let mut pub_decls = Vec::new();
    for decl in module_graph.declarations() {
        if decl.visibility == Visibility::Public && decl.kind != DeclarationKind::Method {
            let new_id = prefix_node_id(&prefix, &decl.id);
            kept_old_to_new.insert(decl.id.clone(), new_id.clone());
            acc.decl_by_mod_and_name.insert(
                (module_fact.module_path.clone(), decl.name.clone()),
                new_id.clone(),
            );
            pub_decls.push(DeclarationNode {
                id: new_id.clone(),
                name: decl.name.clone(),
                signature: decl.signature.clone(),
                scope_id: scope_offset + decl.scope_id,
                kind: decl.kind,
                visibility: decl.visibility,
                is_constructor: decl.is_constructor,
                location: decl.location.clone(),
                owners: vec![new_id],
                recursion_group: 0,
                level: 0,
                findings: decl.findings.clone(),
            });
        }
    }

    let owner_root_by_group: BTreeMap<&NodeId, &NodeId> = module_graph
        .groups()
        .iter()
        .filter_map(|group| match &group.kind {
            GroupKind::Owner { root } => Some((&group.id, root)),
            _ => None,
        })
        .collect();

    let mut mod_ordering = Vec::new();
    for edge in module_graph.ordering_edges() {
        if let (Some(higher), Some(lower)) = (
            kept_old_to_new.get(&edge.higher),
            kept_old_to_new.get(&edge.lower),
        ) {
            mod_ordering.push(OrderingEdge {
                higher: higher.clone(),
                lower: lower.clone(),
                reason: edge.reason,
            });
        }
    }
    for summary in module_graph.summary_edges() {
        if let (Some(&higher_root), Some(&lower_root)) = (
            owner_root_by_group.get(&summary.higher),
            owner_root_by_group.get(&summary.lower),
        ) && let (Some(higher), Some(lower)) = (
            kept_old_to_new.get(higher_root),
            kept_old_to_new.get(lower_root),
        ) && higher != lower
        {
            let candidate = OrderingEdge {
                higher: higher.clone(),
                lower: lower.clone(),
                reason: summary.reason,
            };
            if !mod_ordering.contains(&candidate) {
                mod_ordering.push(candidate);
            }
        }
    }
    for edge in module_graph.reference_edges() {
        if let (Some(source), Some(target)) = (
            kept_old_to_new.get(&edge.source),
            kept_old_to_new.get(&edge.target),
        ) {
            acc.reference_edges.push(ReferenceEdge {
                source: source.clone(),
                target: target.clone(),
                kind: edge.kind,
            });
        }
    }

    let pub_ids: Vec<NodeId> = pub_decls.iter().map(|decl| decl.id.clone()).collect();
    let pairs: Vec<(NodeId, NodeId)> = mod_ordering
        .iter()
        .map(|edge| (edge.higher.clone(), edge.lower.clone()))
        .collect();
    let levels = compute_scc_and_levels(&pub_ids, &pairs);
    for decl in &mut pub_decls {
        (decl.recursion_group, decl.level) = levels.get(&decl.id).copied().unwrap_or((0, 0));
    }
    sort_declarations(&mut pub_decls);
    let children: Vec<NodeId> = pub_decls.iter().map(|decl| decl.id.clone()).collect();
    acc.declarations.extend(pub_decls);
    acc.ordering_edges.extend(mod_ordering);
    Ok(children)
}

fn extract_full_module(
    module_idx: usize,
    module_fact: &ModuleFact,
    acc: &mut AccumulatedProjectModules,
) -> Result<Vec<NodeId>, ModelError> {
    if module_fact.scopes.is_empty() {
        return Ok(Vec::new());
    }
    let module_graph = build_scope_graph(Path::new(&module_fact.path), &module_fact.scopes, 0)?;
    let prefix = format!("m{module_idx}:");
    let scope_offset = (module_idx + 1) * MODULE_SCOPE_ID_STRIDE;

    for decl in module_graph.declarations() {
        let new_id = prefix_node_id(&prefix, &decl.id);
        if decl.kind != DeclarationKind::Method {
            acc.decl_by_mod_and_name.insert(
                (module_fact.module_path.clone(), decl.name.clone()),
                new_id.clone(),
            );
        }
        acc.declarations.push(DeclarationNode {
            id: new_id,
            name: decl.name.clone(),
            signature: decl.signature.clone(),
            scope_id: scope_offset + decl.scope_id,
            kind: decl.kind,
            visibility: decl.visibility,
            is_constructor: decl.is_constructor,
            location: decl.location.clone(),
            owners: decl
                .owners
                .iter()
                .map(|owner| prefix_node_id(&prefix, owner))
                .collect(),
            recursion_group: decl.recursion_group,
            level: decl.level,
            findings: decl.findings.clone(),
        });
    }

    let mut top_children = Vec::new();
    for group in module_graph.groups() {
        let new_group_id = prefix_node_id(&prefix, &group.id);
        if module_graph.parent_of(&group.id).is_none() {
            top_children.push(new_group_id.clone());
        }
        acc.inner_groups.push(GroupNode {
            id: new_group_id,
            label: group.label.clone(),
            kind: prefix_group_kind(&prefix, &group.kind),
            recursion_group: group.recursion_group,
            level: group.level,
            children: group
                .children
                .iter()
                .map(|child| prefix_node_id(&prefix, child))
                .collect(),
        });
    }

    for edge in module_graph.ordering_edges() {
        acc.ordering_edges.push(OrderingEdge {
            higher: prefix_node_id(&prefix, &edge.higher),
            lower: prefix_node_id(&prefix, &edge.lower),
            reason: edge.reason,
        });
    }
    for edge in module_graph.reference_edges() {
        acc.reference_edges.push(ReferenceEdge {
            source: prefix_node_id(&prefix, &edge.source),
            target: prefix_node_id(&prefix, &edge.target),
            kind: edge.kind,
        });
    }
    for edge in module_graph.summary_edges() {
        acc.inner_summaries.push(SummaryEdge {
            higher: prefix_node_id(&prefix, &edge.higher),
            lower: prefix_node_id(&prefix, &edge.lower),
            reason: edge.reason,
            count: edge.count,
            reflexion: edge.reflexion,
        });
    }

    Ok(top_children)
}

fn prefix_group_kind(prefix: &str, kind: &GroupKind) -> GroupKind {
    match kind {
        GroupKind::Component {
            declared_dependencies,
        } => GroupKind::Component {
            declared_dependencies: declared_dependencies.clone(),
        },
        GroupKind::Module { path } => GroupKind::Module { path: path.clone() },
        GroupKind::Owner { root } => GroupKind::Owner {
            root: prefix_node_id(prefix, root),
        },
        GroupKind::Shared { owners } => GroupKind::Shared {
            owners: owners
                .iter()
                .map(|owner| prefix_node_id(prefix, owner))
                .collect(),
        },
        GroupKind::Unowned => GroupKind::Unowned,
        GroupKind::TypeMembers { type_id } => GroupKind::TypeMembers {
            type_id: prefix_node_id(prefix, type_id),
        },
        GroupKind::RecursionGroup { group_id } => GroupKind::RecursionGroup {
            group_id: *group_id,
        },
    }
}

fn prefix_node_id(prefix: &str, id: &NodeId) -> NodeId {
    NodeId::new(format!("{prefix}{id}"))
}

fn append_cross_module_decl_edges(project: &ProjectFacts, acc: &mut AccumulatedProjectModules) {
    for rel in &project.cross_module_refs {
        if rel.source_module == rel.target_module {
            continue;
        }
        let (Some(src_sym), Some(dst_sym)) = (&rel.source_symbol, &rel.target_symbol) else {
            continue;
        };
        let Some(source_id) = acc
            .decl_by_mod_and_name
            .get(&(rel.source_module.clone(), src_sym.clone()))
        else {
            continue;
        };
        let Some(target_id) = acc
            .decl_by_mod_and_name
            .get(&(rel.target_module.clone(), dst_sym.clone()))
        else {
            continue;
        };
        if source_id != target_id {
            acc.reference_edges.push(ReferenceEdge {
                source: source_id.clone(),
                target: target_id.clone(),
                kind: RelationKind::Body,
            });
            acc.ordering_edges.push(OrderingEdge {
                higher: source_id.clone(),
                lower: target_id.clone(),
                reason: EdgeReason::CrossModule,
            });
        }
    }
    acc.ordering_edges.sort();
    acc.ordering_edges.dedup();
    acc.reference_edges.sort();
    acc.reference_edges.dedup();
}

struct ComponentDepSets {
    direct: BTreeSet<(String, String)>,
    transitive: BTreeSet<(String, String)>,
}

fn build_component_dep_sets(project: &ProjectFacts) -> ComponentDepSets {
    let mut direct = BTreeSet::new();
    let mut adj: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for comp in &project.components {
        for dep in &comp.declared_dependencies {
            direct.insert((comp.name.clone(), dep.clone()));
            adj.entry(comp.name.as_str())
                .or_default()
                .push(dep.as_str());
        }
    }

    let mut transitive = BTreeSet::new();
    for comp in &project.components {
        let mut visited = BTreeSet::new();
        let mut stack = adj.get(comp.name.as_str()).cloned().unwrap_or_default();
        while let Some(target) = stack.pop() {
            if visited.insert(target) {
                transitive.insert((comp.name.clone(), target.to_owned()));
                if let Some(next_deps) = adj.get(target) {
                    stack.extend(next_deps.iter().copied());
                }
            }
        }
    }

    ComponentDepSets { direct, transitive }
}

fn build_module_summary_edges(
    project: &ProjectFacts,
    mod_to_comp: &BTreeMap<&str, &str>,
    comp_deps: &ComponentDepSets,
) -> Vec<SummaryEdge> {
    let mut mod_counts: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for rel in &project.cross_module_refs {
        let src = rel.source_module.as_str();
        let dst = rel.target_module.as_str();
        if src != dst && mod_to_comp.contains_key(src) && mod_to_comp.contains_key(dst) {
            *mod_counts.entry((src, dst)).or_insert(0) += 1;
        }
    }

    let mut summaries = Vec::new();
    for ((src_mod, dst_mod), count) in mod_counts {
        let src_comp = mod_to_comp[src_mod];
        let dst_comp = mod_to_comp[dst_mod];
        let reflexion = if !project.has_declared_architecture || src_comp == dst_comp {
            Some(ReflexionStatus::Inferred)
        } else if comp_deps
            .transitive
            .contains(&(src_comp.to_owned(), dst_comp.to_owned()))
        {
            Some(ReflexionStatus::Convergent)
        } else {
            Some(ReflexionStatus::Divergent)
        };
        summaries.push(SummaryEdge {
            higher: NodeId::new(format!("mod:{src_mod}")),
            lower: NodeId::new(format!("mod:{dst_mod}")),
            reason: EdgeReason::CrossModule,
            count,
            reflexion,
        });
    }
    summaries
}

fn assign_intra_component_module_levels(
    module_groups: &mut [GroupNode],
    mod_to_comp: &BTreeMap<&str, &str>,
    mod_summaries: &[SummaryEdge],
) {
    let mut mods_by_comp: BTreeMap<&str, Vec<NodeId>> = BTreeMap::new();
    for group in module_groups.iter() {
        if let GroupKind::Module { path } = &group.kind
            && let Some(&comp) = mod_to_comp.get(path.as_str())
        {
            mods_by_comp.entry(comp).or_default().push(group.id.clone());
        }
    }

    let mut level_by_mod_id: BTreeMap<NodeId, (usize, usize)> = BTreeMap::new();
    for mod_ids in mods_by_comp.values() {
        let intra_pairs: Vec<(NodeId, NodeId)> = mod_summaries
            .iter()
            .filter(|edge| mod_ids.contains(&edge.higher) && mod_ids.contains(&edge.lower))
            .map(|edge| (edge.higher.clone(), edge.lower.clone()))
            .collect();
        level_by_mod_id.extend(compute_scc_and_levels(mod_ids, &intra_pairs));
    }

    for group in module_groups.iter_mut() {
        if let Some(&(scc_id, level)) = level_by_mod_id.get(&group.id) {
            group.recursion_group = scc_id;
            group.level = level;
        }
    }
    module_groups.sort_by(|left, right| {
        (Reverse(left.level), &left.id).cmp(&(Reverse(right.level), &right.id))
    });
}

struct ComponentGraphParts {
    groups: Vec<GroupNode>,
    summary_edges: Vec<SummaryEdge>,
}

fn build_component_groups_and_summaries(
    project: &ProjectFacts,
    module_groups: &[GroupNode],
    mod_to_comp: &BTreeMap<&str, &str>,
    comp_deps: &ComponentDepSets,
) -> ComponentGraphParts {
    let declared_by_name: BTreeMap<&str, Vec<String>> = if project.has_declared_architecture {
        project
            .components
            .iter()
            .map(|comp| (comp.name.as_str(), comp.declared_dependencies.clone()))
            .collect()
    } else {
        BTreeMap::new()
    };
    let mut comp_names: Vec<String> = project
        .components
        .iter()
        .map(|comp| comp.name.clone())
        .collect();
    for &comp in mod_to_comp.values() {
        if !comp_names.iter().any(|existing| existing == comp) {
            comp_names.push(comp.to_owned());
        }
    }

    let comp_summaries = build_component_reflexion_summaries(project, mod_to_comp, comp_deps);

    let comp_ids: Vec<NodeId> = comp_names
        .iter()
        .map(|name| NodeId::new(format!("comp:{name}")))
        .collect();
    let non_divergent_pairs: Vec<(NodeId, NodeId)> = comp_summaries
        .iter()
        .filter(|edge| edge.reflexion != Some(ReflexionStatus::Divergent))
        .map(|edge| (edge.higher.clone(), edge.lower.clone()))
        .collect();
    let comp_levels = compute_scc_and_levels(&comp_ids, &non_divergent_pairs);

    let mut comp_groups = Vec::new();
    for comp_name in comp_names {
        let comp_id = NodeId::new(format!("comp:{comp_name}"));
        let children: Vec<NodeId> = module_groups
            .iter()
            .filter(|group| {
                matches!(
                    &group.kind,
                    GroupKind::Module { path }
                        if mod_to_comp.get(path.as_str()) == Some(&comp_name.as_str())
                )
            })
            .map(|group| group.id.clone())
            .collect();
        let (recursion_group, level) = comp_levels.get(&comp_id).copied().unwrap_or((0, 0));
        let declared_dependencies = declared_by_name.get(comp_name.as_str()).cloned();
        comp_groups.push(GroupNode {
            id: comp_id,
            label: comp_name,
            kind: GroupKind::Component {
                declared_dependencies,
            },
            recursion_group,
            level,
            children,
        });
    }
    comp_groups.sort_by(|left, right| {
        (Reverse(left.level), &left.id).cmp(&(Reverse(right.level), &right.id))
    });

    ComponentGraphParts {
        groups: comp_groups,
        summary_edges: comp_summaries,
    }
}

fn build_component_reflexion_summaries(
    project: &ProjectFacts,
    mod_to_comp: &BTreeMap<&str, &str>,
    comp_deps: &ComponentDepSets,
) -> Vec<SummaryEdge> {
    let mut actual_comp_counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    for rel in &project.cross_module_refs {
        if let (Some(&src_comp), Some(&dst_comp)) = (
            mod_to_comp.get(rel.source_module.as_str()),
            mod_to_comp.get(rel.target_module.as_str()),
        ) && src_comp != dst_comp
        {
            *actual_comp_counts
                .entry((src_comp.to_owned(), dst_comp.to_owned()))
                .or_insert(0) += 1;
        }
    }

    let mut comp_summaries = Vec::new();
    if project.has_declared_architecture {
        for (src_comp, dst_comp) in &comp_deps.direct {
            let count = actual_comp_counts
                .remove(&(src_comp.clone(), dst_comp.clone()))
                .unwrap_or(0);
            comp_summaries.push(SummaryEdge {
                higher: NodeId::new(format!("comp:{src_comp}")),
                lower: NodeId::new(format!("comp:{dst_comp}")),
                reason: EdgeReason::CrossModule,
                count,
                reflexion: Some(if count > 0 {
                    ReflexionStatus::Convergent
                } else {
                    ReflexionStatus::Absent
                }),
            });
        }
        for ((src_comp, dst_comp), count) in actual_comp_counts {
            let status = if comp_deps
                .transitive
                .contains(&(src_comp.clone(), dst_comp.clone()))
            {
                ReflexionStatus::Convergent
            } else {
                ReflexionStatus::Divergent
            };
            comp_summaries.push(SummaryEdge {
                higher: NodeId::new(format!("comp:{src_comp}")),
                lower: NodeId::new(format!("comp:{dst_comp}")),
                reason: EdgeReason::CrossModule,
                count,
                reflexion: Some(status),
            });
        }
    } else {
        for ((src_comp, dst_comp), count) in actual_comp_counts {
            comp_summaries.push(SummaryEdge {
                higher: NodeId::new(format!("comp:{src_comp}")),
                lower: NodeId::new(format!("comp:{dst_comp}")),
                reason: EdgeReason::CrossModule,
                count,
                reflexion: Some(ReflexionStatus::Inferred),
            });
        }
    }
    comp_summaries
}

/// Builds a validated [`AbstractionGraph`] for a single file's primary scope (`scopes[0]`),
/// returning an empty graph if `scopes` is empty.
///
/// # Errors
///
/// Returns [`ModelError`] if the resulting graph violates any Layer L1 invariant.
pub fn build_file_graph(
    path: &Path,
    scopes: &[ScopeFacts],
) -> Result<AbstractionGraph, ModelError> {
    build_scope_graph(path, scopes, 0)
}

fn build_scope_graph(
    path: &Path,
    scopes: &[ScopeFacts],
    scope_index: usize,
) -> Result<AbstractionGraph, ModelError> {
    let file_path = path.to_string_lossy().into_owned();
    let Some(scope) = scopes.get(scope_index) else {
        return AbstractionGraph::new(GraphParts {
            title: file_path,
            declarations: Vec::new(),
            groups: Vec::new(),
            ordering_edges: Vec::new(),
            reference_edges: Vec::new(),
            summary_edges: Vec::new(),
        });
    };
    let title = format_scope_title(&file_path, &scope.scope);
    let mut extracted = extract_scope_items(scope, scope_index, &file_path, None);

    apply_local_owner_levels(&mut extracted.declarations, &extracted.ordering_edges);

    let type_members = extract_type_member_groups(scopes, scope_index, &file_path, &mut extracted);
    let built_groups = build_ownership_groups(
        &extracted.declarations,
        scope_index,
        &type_members.groups,
        &extracted.ordering_edges,
    );
    extracted.declarations.extend(type_members.declarations);
    sort_declarations(&mut extracted.declarations);

    AbstractionGraph::new(GraphParts {
        title,
        declarations: extracted.declarations,
        groups: built_groups.groups,
        ordering_edges: extracted.ordering_edges,
        reference_edges: extracted.reference_edges,
        summary_edges: built_groups.summary_edges,
    })
}

fn format_scope_title(file_path: &str, label: &DeclarationScopeKind) -> String {
    match label {
        DeclarationScopeKind::Module => file_path.to_owned(),
        DeclarationScopeKind::Class { name } => format!("{file_path}::class {name}"),
        DeclarationScopeKind::Impl { type_name } => format!("{file_path}::impl {type_name}"),
    }
}

struct ExtractedScope {
    declarations: Vec<DeclarationNode>,
    ordering_edges: Vec<OrderingEdge>,
    reference_edges: Vec<ReferenceEdge>,
}

fn apply_local_owner_levels(declarations: &mut [DeclarationNode], ordering_edges: &[OrderingEdge]) {
    let mut buckets: BTreeMap<Vec<NodeId>, Vec<NodeId>> = BTreeMap::new();
    for decl in declarations.iter() {
        buckets
            .entry(decl.owners.clone())
            .or_default()
            .push(decl.id.clone());
    }

    let mut updated_by_id: BTreeMap<NodeId, (usize, usize)> = BTreeMap::new();
    for member_ids in buckets.values() {
        let intra_edges: Vec<(NodeId, NodeId)> = ordering_edges
            .iter()
            .filter(|edge| member_ids.contains(&edge.higher) && member_ids.contains(&edge.lower))
            .map(|edge| (edge.higher.clone(), edge.lower.clone()))
            .collect();
        let local_levels = compute_scc_and_levels(member_ids, &intra_edges);
        updated_by_id.extend(local_levels);
    }

    for decl in declarations.iter_mut() {
        if let Some(&(scc_id, level)) = updated_by_id.get(&decl.id) {
            decl.recursion_group = scc_id;
            decl.level = level;
        }
    }
    sort_declarations(declarations);
}

struct ExtractedTypeMembers {
    groups: BTreeMap<NodeId, Vec<GroupNode>>,
    declarations: Vec<DeclarationNode>,
}

fn extract_type_member_groups(
    scopes: &[ScopeFacts],
    primary_scope_index: usize,
    file_path: &str,
    extracted: &mut ExtractedScope,
) -> ExtractedTypeMembers {
    let mut groups = BTreeMap::new();
    let mut declarations = Vec::new();
    if scopes[primary_scope_index].scope != DeclarationScopeKind::Module {
        return ExtractedTypeMembers {
            groups,
            declarations,
        };
    }

    for type_decl in &extracted.declarations {
        if type_decl.kind != DeclarationKind::Type {
            continue;
        }
        let mut method_decls = Vec::new();
        for (idx, candidate) in scopes.iter().enumerate() {
            if idx == primary_scope_index || candidate.items.is_empty() {
                continue;
            }
            let matches_type = match &candidate.scope {
                DeclarationScopeKind::Class { name } => name == &type_decl.name,
                DeclarationScopeKind::Impl { type_name } => type_name == &type_decl.name,
                DeclarationScopeKind::Module => false,
            };
            if matches_type {
                let prefix = TypeMemberPrefix {
                    type_name: &type_decl.name,
                    owners: &type_decl.owners,
                };
                let member_extracted = extract_scope_items(candidate, idx, file_path, Some(prefix));
                method_decls.extend(member_extracted.declarations);
                extracted
                    .ordering_edges
                    .extend(member_extracted.ordering_edges);
                extracted
                    .reference_edges
                    .extend(member_extracted.reference_edges);
            }
        }
        if !method_decls.is_empty() {
            sort_declarations(&mut method_decls);
            let group_id = NodeId::new(format!("members:{}", type_decl.id));
            let mut wrapped = wrap_cycles_in_container(&group_id, &method_decls);
            let mut groups_for_type = Vec::with_capacity(1 + wrapped.nested_groups.len());
            groups_for_type.push(GroupNode {
                id: group_id,
                label: format!("{} methods", type_decl.name),
                kind: GroupKind::TypeMembers {
                    type_id: type_decl.id.clone(),
                },
                recursion_group: 0,
                level: type_decl.level,
                children: wrapped.children,
            });
            groups_for_type.append(&mut wrapped.nested_groups);
            declarations.extend(method_decls);
            groups.insert(type_decl.id.clone(), groups_for_type);
        }
    }

    extracted.ordering_edges.sort();
    extracted.reference_edges.sort();
    ExtractedTypeMembers {
        groups,
        declarations,
    }
}

#[derive(Clone, Copy)]
struct TypeMemberPrefix<'a> {
    type_name: &'a str,
    owners: &'a [NodeId],
}

fn extract_scope_items(
    scope: &ScopeFacts,
    scope_index: usize,
    file_path: &str,
    member_prefix: Option<TypeMemberPrefix<'_>>,
) -> ExtractedScope {
    let item_ids: Vec<NodeId> = scope
        .items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            member_prefix.map_or_else(
                || NodeId::new(format!("s{scope_index}:i{idx}:{}", item.name)),
                |prefix| {
                    NodeId::new(format!(
                        "s{scope_index}:i{idx}:{}::{}",
                        prefix.type_name, item.name
                    ))
                },
            )
        })
        .collect();

    let mut declarations: Vec<DeclarationNode> = scope
        .items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            build_declaration_node(
                item,
                item_ids[idx].clone(),
                scope_index,
                file_path,
                &item_ids,
                member_prefix,
            )
        })
        .collect();
    sort_declarations(&mut declarations);

    let mut ordering_edges: Vec<OrderingEdge> = scope
        .ordering
        .iter()
        .map(|edge| OrderingEdge {
            higher: item_ids[edge.higher].clone(),
            lower: item_ids[edge.lower].clone(),
            reason: EdgeReason::from(edge.reason),
        })
        .collect();
    ordering_edges.sort();

    let mut reference_edges: Vec<ReferenceEdge> = scope
        .relations
        .iter()
        .map(|rel| ReferenceEdge {
            source: item_ids[rel.source].clone(),
            target: item_ids[rel.target].clone(),
            kind: rel.kind,
        })
        .collect();
    reference_edges.sort();

    ExtractedScope {
        declarations,
        ordering_edges,
        reference_edges,
    }
}

fn build_declaration_node(
    item: &ItemFacts,
    id: NodeId,
    scope_index: usize,
    file_path: &str,
    scope_item_ids: &[NodeId],
    member_prefix: Option<TypeMemberPrefix<'_>>,
) -> DeclarationNode {
    let mapped_owners: Vec<NodeId> = item
        .unit_owners
        .iter()
        .map(|&unit_idx| scope_item_ids[unit_idx].clone())
        .collect();
    let (name, kind, owners) = if let Some(prefix) = member_prefix {
        let method_owners = if mapped_owners.is_empty() {
            prefix.owners.to_vec()
        } else {
            mapped_owners
        };
        (
            format!("{}::{}", prefix.type_name, item.name),
            DeclarationKind::Method,
            method_owners,
        )
    } else {
        (
            item.name.clone(),
            match item.kind {
                ScopeItemKind::Callable => DeclarationKind::Callable,
                ScopeItemKind::Type => DeclarationKind::Type,
            },
            mapped_owners,
        )
    };
    DeclarationNode {
        id,
        name,
        signature: item.signature.clone(),
        scope_id: scope_index,
        kind,
        visibility: item.visibility,
        is_constructor: item.is_constructor,
        location: SourceLocation {
            file: file_path.to_owned(),
            line: item.line,
            column: item.column,
            end_line: item.end_line,
        },
        owners,
        recursion_group: item.recursion_group,
        level: item.level,
        findings: item.findings.clone(),
    }
}

fn sort_declarations(declarations: &mut [DeclarationNode]) {
    declarations.sort_by(|left, right| {
        (
            Reverse(left.is_constructor),
            Reverse(left.visibility == Visibility::Public),
            Reverse(left.level),
            left.location.line,
            left.location.column,
            &left.id,
        )
            .cmp(&(
                Reverse(right.is_constructor),
                Reverse(right.visibility == Visibility::Public),
                Reverse(right.level),
                right.location.line,
                right.location.column,
                &right.id,
            ))
    });
}

struct BuiltGroups {
    groups: Vec<GroupNode>,
    summary_edges: Vec<SummaryEdge>,
}

fn build_ownership_groups(
    primary_declarations: &[DeclarationNode],
    scope_index: usize,
    member_groups: &BTreeMap<NodeId, Vec<GroupNode>>,
    ordering_edges: &[OrderingEdge],
) -> BuiltGroups {
    let mut top_groups = Vec::new();
    let mut nested_groups = Vec::new();
    collect_single_owner_groups(
        primary_declarations,
        member_groups,
        &mut top_groups,
        &mut nested_groups,
    );
    collect_shared_and_unowned_groups(
        primary_declarations,
        scope_index,
        member_groups,
        &mut top_groups,
        &mut nested_groups,
    );

    let top_group_ids: Vec<NodeId> = top_groups.iter().map(|group| group.id.clone()).collect();
    let mut all_groups_temp = top_groups.clone();
    all_groups_temp.extend(nested_groups.iter().cloned());
    let summary_edges = compute_summary_edges(&all_groups_temp, &top_group_ids, ordering_edges);

    let summary_pairs: Vec<(NodeId, NodeId)> = summary_edges
        .iter()
        .map(|edge| (edge.higher.clone(), edge.lower.clone()))
        .collect();
    let group_levels = compute_scc_and_levels(&top_group_ids, &summary_pairs);
    for group in &mut top_groups {
        if let Some(&(scc_id, level)) = group_levels.get(&group.id) {
            group.recursion_group = scc_id;
            group.level = level;
        }
    }

    top_groups.sort_by(|left, right| {
        (Reverse(left.level), group_kind_rank(&left.kind), &left.id).cmp(&(
            Reverse(right.level),
            group_kind_rank(&right.kind),
            &right.id,
        ))
    });

    let mut groups = top_groups;
    groups.extend(nested_groups);
    BuiltGroups {
        groups,
        summary_edges,
    }
}

fn collect_single_owner_groups(
    primary_declarations: &[DeclarationNode],
    member_groups: &BTreeMap<NodeId, Vec<GroupNode>>,
    top_groups: &mut Vec<GroupNode>,
    nested_groups: &mut Vec<GroupNode>,
) {
    for pub_decl in primary_declarations.iter().filter(|decl| {
        decl.visibility == Visibility::Public && decl.owners.as_slice() == [decl.id.clone()]
    }) {
        let members: Vec<DeclarationNode> = primary_declarations
            .iter()
            .filter(|decl| decl.owners.as_slice() == [pub_decl.id.clone()])
            .cloned()
            .collect();
        let group_id = NodeId::new(format!("owner:{}", pub_decl.id));
        let mut assembled = assemble_container_children(&group_id, &members, member_groups);
        nested_groups.append(&mut assembled.nested_groups);
        top_groups.push(GroupNode {
            id: group_id,
            label: pub_decl.name.clone(),
            kind: GroupKind::Owner {
                root: pub_decl.id.clone(),
            },
            recursion_group: 0,
            level: 0,
            children: assembled.children,
        });
    }
}

fn collect_shared_and_unowned_groups(
    primary_declarations: &[DeclarationNode],
    scope_index: usize,
    member_groups: &BTreeMap<NodeId, Vec<GroupNode>>,
    top_groups: &mut Vec<GroupNode>,
    nested_groups: &mut Vec<GroupNode>,
) {
    let decl_by_id: BTreeMap<NodeId, &DeclarationNode> = primary_declarations
        .iter()
        .map(|decl| (decl.id.clone(), decl))
        .collect();
    let mut shared_buckets: BTreeMap<Vec<NodeId>, Vec<DeclarationNode>> = BTreeMap::new();
    let mut unowned_members = Vec::new();
    for decl in primary_declarations
        .iter()
        .filter(|decl| decl.visibility == Visibility::Private)
    {
        if decl.owners.len() >= 2 {
            shared_buckets
                .entry(decl.owners.clone())
                .or_default()
                .push(decl.clone());
        } else if decl.owners.is_empty() {
            unowned_members.push(decl.clone());
        }
    }

    for (owners, members) in shared_buckets {
        let owner_names: Vec<&str> = owners
            .iter()
            .filter_map(|id| decl_by_id.get(id).map(|decl| decl.name.as_str()))
            .collect();
        let owner_ids_key: Vec<&str> = owners.iter().map(NodeId::as_str).collect();
        let group_id = NodeId::new(format!("shared:{}", owner_ids_key.join("+")));
        let mut assembled = assemble_container_children(&group_id, &members, member_groups);
        nested_groups.append(&mut assembled.nested_groups);
        top_groups.push(GroupNode {
            id: group_id,
            label: format!("Shared ({})", owner_names.join(", ")),
            kind: GroupKind::Shared { owners },
            recursion_group: 0,
            level: 0,
            children: assembled.children,
        });
    }

    if !unowned_members.is_empty() {
        let group_id = NodeId::new(format!("unowned:s{scope_index}"));
        let mut assembled = assemble_container_children(&group_id, &unowned_members, member_groups);
        nested_groups.append(&mut assembled.nested_groups);
        top_groups.push(GroupNode {
            id: group_id,
            label: "Unowned private items".to_owned(),
            kind: GroupKind::Unowned,
            recursion_group: 0,
            level: 0,
            children: assembled.children,
        });
    }
}

struct AssembledChildren {
    children: Vec<NodeId>,
    nested_groups: Vec<GroupNode>,
}

fn assemble_container_children(
    parent_group_id: &NodeId,
    members: &[DeclarationNode],
    member_groups: &BTreeMap<NodeId, Vec<GroupNode>>,
) -> AssembledChildren {
    let wrapped = wrap_cycles_in_container(parent_group_id, members);
    let mut nested_groups = wrapped.nested_groups;
    let mut children = Vec::new();
    for child_id in wrapped.children {
        children.push(child_id.clone());
        if let Some(type_groups) = member_groups.get(&child_id) {
            if let Some(first_group) = type_groups.first() {
                children.push(first_group.id.clone());
            }
            nested_groups.extend(type_groups.iter().cloned());
        }
    }
    for decl in members {
        if !children.contains(&decl.id)
            && let Some(type_groups) = member_groups.get(&decl.id)
        {
            if let Some(first_group) = type_groups.first() {
                children.push(first_group.id.clone());
            }
            nested_groups.extend(type_groups.iter().cloned());
        }
    }
    AssembledChildren {
        children,
        nested_groups,
    }
}

fn wrap_cycles_in_container(
    parent_group_id: &NodeId,
    members: &[DeclarationNode],
) -> AssembledChildren {
    let mut counts_by_group: BTreeMap<(usize, usize), Vec<&DeclarationNode>> = BTreeMap::new();
    for decl in members {
        counts_by_group
            .entry((decl.scope_id, decl.recursion_group))
            .or_default()
            .push(decl);
    }

    let mut children = Vec::new();
    let mut nested_groups = Vec::new();
    let mut emitted_cycles = BTreeMap::new();

    for decl in members {
        let key = (decl.scope_id, decl.recursion_group);
        let cycle_members = &counts_by_group[&key];
        if cycle_members.len() < 2 {
            children.push(decl.id.clone());
            continue;
        }
        if emitted_cycles.contains_key(&key) {
            continue;
        }
        let cycle_id = NodeId::new(format!(
            "cycle:{parent_group_id}:s{}:g{}",
            decl.scope_id, decl.recursion_group
        ));
        let names: Vec<&str> = cycle_members
            .iter()
            .map(|item| item.name.as_str())
            .collect();
        let max_level = cycle_members
            .iter()
            .map(|item| item.level)
            .max()
            .unwrap_or(0);
        nested_groups.push(GroupNode {
            id: cycle_id.clone(),
            label: format!("Cycle ({})", names.join(" <-> ")),
            kind: GroupKind::RecursionGroup {
                group_id: decl.recursion_group,
            },
            recursion_group: decl.recursion_group,
            level: max_level,
            children: cycle_members.iter().map(|item| item.id.clone()).collect(),
        });
        emitted_cycles.insert(key, cycle_id.clone());
        children.push(cycle_id);
    }

    AssembledChildren {
        children,
        nested_groups,
    }
}

const fn group_kind_rank(kind: &GroupKind) -> u8 {
    match kind {
        GroupKind::Component { .. } => 0,
        GroupKind::Module { .. } => 1,
        GroupKind::Owner { .. } => 2,
        GroupKind::Shared { .. } => 3,
        GroupKind::Unowned => 4,
        GroupKind::RecursionGroup { .. } => 5,
        GroupKind::TypeMembers { .. } => 6,
    }
}

fn compute_summary_edges(
    all_groups: &[GroupNode],
    top_group_ids: &[NodeId],
    ordering_edges: &[OrderingEdge],
) -> Vec<SummaryEdge> {
    let mut parent_of = BTreeMap::new();
    for group in all_groups {
        for child in &group.children {
            parent_of.insert(child.clone(), group.id.clone());
        }
    }

    let mut counts: BTreeMap<(NodeId, NodeId, EdgeReason), usize> = BTreeMap::new();
    for edge in ordering_edges {
        let Some(higher_top) = find_top_group(&edge.higher, &parent_of, top_group_ids) else {
            continue;
        };
        let Some(lower_top) = find_top_group(&edge.lower, &parent_of, top_group_ids) else {
            continue;
        };
        if higher_top != lower_top {
            *counts
                .entry((higher_top, lower_top, edge.reason))
                .or_insert(0) += 1;
        }
    }

    let mut summaries: Vec<SummaryEdge> = counts
        .into_iter()
        .map(|((higher, lower, reason), count)| SummaryEdge {
            higher,
            lower,
            reason,
            count,
            reflexion: None,
        })
        .collect();
    summaries.sort();
    summaries
}

fn find_top_group(
    node_id: &NodeId,
    parent_of: &BTreeMap<NodeId, NodeId>,
    top_group_ids: &[NodeId],
) -> Option<NodeId> {
    let mut cursor = parent_of.get(node_id)?;
    while let Some(next_parent) = parent_of.get(cursor) {
        cursor = next_parent;
    }
    top_group_ids
        .iter()
        .find(|&top_id| top_id == cursor)
        .cloned()
}

fn compute_scc_and_levels(
    node_ids: &[NodeId],
    edges: &[(NodeId, NodeId)],
) -> BTreeMap<NodeId, (usize, usize)> {
    let index_by_id: BTreeMap<&NodeId, usize> = node_ids
        .iter()
        .enumerate()
        .map(|(idx, id)| (id, idx))
        .collect();
    let mut successors = vec![Vec::new(); node_ids.len()];
    for (higher, lower) in edges {
        if let (Some(&higher_idx), Some(&lower_idx)) =
            (index_by_id.get(higher), index_by_id.get(lower))
        {
            successors[higher_idx].push(lower_idx);
        }
    }
    let component = strongly_connected_components(&successors);
    let level = levels(&successors, &component);
    node_ids
        .iter()
        .enumerate()
        .map(|(idx, id)| (id.clone(), (component[idx], level[idx])))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::{ComponentFact, CrossModuleRefFact};
    use super::*;
    use crate::code_lint::ast::{
        FindingFact, OrderingEdge as AstOrderingEdge, OrderingReason, Ownership, PlacementAnchor,
        RelationFact,
    };

    fn sample_scopes() -> Vec<ScopeFacts> {
        vec![sample_module_scope(), sample_impl_scope()]
    }

    fn sample_module_scope() -> ScopeFacts {
        ScopeFacts {
            scope: DeclarationScopeKind::Module,
            items: vec![
                sample_item(
                    "entry_a",
                    ScopeItemKind::Callable,
                    Visibility::Public,
                    10,
                    &[0],
                    4,
                    2,
                ),
                sample_item(
                    "entry_b",
                    ScopeItemKind::Callable,
                    Visibility::Public,
                    20,
                    &[1],
                    3,
                    2,
                ),
                ItemFacts {
                    findings: vec![FindingFact {
                        rule: "callee-before-caller",
                        related: "entry_a".to_owned(),
                        reason: Some(OrderingReason::Encapsulation),
                        anchor: PlacementAnchor {
                            name: "entry_a".to_owned(),
                            line: 10,
                        },
                    }],
                    ..sample_item(
                        "Spec",
                        ScopeItemKind::Type,
                        Visibility::Private,
                        30,
                        &[0],
                        2,
                        1,
                    )
                },
                sample_item(
                    "only_a",
                    ScopeItemKind::Callable,
                    Visibility::Private,
                    40,
                    &[0],
                    1,
                    0,
                ),
                sample_item(
                    "shared_ab",
                    ScopeItemKind::Callable,
                    Visibility::Private,
                    50,
                    &[0, 1],
                    0,
                    0,
                ),
            ],
            relations: vec![
                sample_rel(0, 2, RelationKind::Body),
                sample_rel(0, 3, RelationKind::Body),
                sample_rel(0, 4, RelationKind::Body),
                sample_rel(1, 4, RelationKind::Body),
                sample_rel(2, 3, RelationKind::Behavior),
            ],
            ordering: vec![
                sample_ord(0, 2, OrderingReason::Encapsulation),
                sample_ord(0, 3, OrderingReason::Call),
                sample_ord(0, 4, OrderingReason::Call),
                sample_ord(1, 4, OrderingReason::Call),
                sample_ord(2, 3, OrderingReason::ImplBehavior),
            ],
        }
    }

    fn sample_impl_scope() -> ScopeFacts {
        ScopeFacts {
            scope: DeclarationScopeKind::Impl {
                type_name: "Spec".to_owned(),
            },
            items: vec![
                ItemFacts {
                    is_constructor: true,
                    ..sample_item(
                        "new",
                        ScopeItemKind::Callable,
                        Visibility::Public,
                        32,
                        &[0],
                        1,
                        1,
                    )
                },
                sample_item(
                    "validate",
                    ScopeItemKind::Callable,
                    Visibility::Private,
                    36,
                    &[0],
                    0,
                    0,
                ),
            ],
            relations: vec![sample_rel(0, 1, RelationKind::Body)],
            ordering: vec![sample_ord(0, 1, OrderingReason::Call)],
        }
    }

    fn sample_item(
        name: &str,
        kind: ScopeItemKind,
        visibility: Visibility,
        line: usize,
        roots: &[usize],
        recursion_group: usize,
        level: usize,
    ) -> ItemFacts {
        let ownership = match roots {
            [] => Ownership::Unowned,
            [only] => Ownership::Owner(*only),
            _ => Ownership::Shared,
        };
        ItemFacts {
            name: name.to_owned(),
            kind,
            visibility,
            is_constructor: false,
            line,
            end_line: Some(line),
            column: 1,
            signature: None,
            roots: roots.to_vec(),
            ownership,
            unit_owners: roots.to_vec(),
            recursion_group,
            level,
            findings: Vec::new(),
        }
    }

    const fn sample_rel(source: usize, target: usize, kind: RelationKind) -> RelationFact {
        RelationFact {
            source,
            target,
            kind,
        }
    }

    const fn sample_ord(higher: usize, lower: usize, reason: OrderingReason) -> AstOrderingEdge {
        AstOrderingEdge {
            higher,
            lower,
            reason,
        }
    }

    #[test]
    fn ownership_groups_exclusive_helpers_and_shared_items_with_summaries() {
        let scopes = sample_scopes();

        let graph =
            build_file_graph(Path::new("src/sample.rs"), &scopes).expect("valid canonical graph");

        assert_eq!(
            graph.parent_of(&NodeId::new("s0:i3:only_a")),
            Some(&NodeId::new("owner:s0:i0:entry_a"))
        );
        assert_eq!(
            graph.summary_edges().len(),
            2,
            "expected summary Call edges from owner:entry_a and owner:entry_b to shared(entry_a, entry_b)"
        );
    }

    #[test]
    fn attaches_type_methods_group_under_owning_component() {
        let scopes = sample_scopes();

        let graph =
            build_file_graph(Path::new("src/sample.rs"), &scopes).expect("valid canonical graph");

        assert_eq!(graph.declarations().len(), 7);
        assert!(graph.groups().iter().any(|group| {
            group.kind
                == GroupKind::TypeMembers {
                    type_id: NodeId::new("s0:i2:Spec"),
                }
        }));
        assert!(
            graph
                .ordering_edges()
                .iter()
                .any(|edge| edge.higher == NodeId::new("s1:i0:Spec::new")
                    && edge.reason == EdgeReason::Call)
        );
    }

    #[test]
    fn empty_scopes_produce_empty_valid_graph() {
        let graph = build_file_graph(Path::new("src/lib.rs"), &[]).expect("empty graph is valid");

        assert_eq!(graph.title(), "src/lib.rs");
        assert_eq!(graph.declarations(), []);
        assert_eq!(graph.groups(), []);
    }

    #[test]
    fn project_graph_preserves_declared_dependencies_and_reflexion_edges() {
        let project = ProjectFacts {
            title: "sample".to_owned(),
            has_declared_architecture: true,
            components: vec![
                ComponentFact {
                    name: "Core".to_owned(),
                    declared_dependencies: Vec::new(),
                },
                ComponentFact {
                    name: "Unused".to_owned(),
                    declared_dependencies: Vec::new(),
                },
                ComponentFact {
                    name: "Runner".to_owned(),
                    declared_dependencies: vec!["Core".to_owned(), "Unused".to_owned()],
                },
            ],
            modules: vec![
                ModuleFact {
                    module_path: "core".to_owned(),
                    path: "src/core.rs".to_owned(),
                    component: "Core".to_owned(),
                    scopes: sample_scopes(),
                },
                ModuleFact {
                    module_path: "runner".to_owned(),
                    path: "src/runner.rs".to_owned(),
                    component: "Runner".to_owned(),
                    scopes: sample_scopes(),
                },
            ],
            cross_module_refs: vec![CrossModuleRefFact {
                source_module: "runner".to_owned(),
                source_symbol: Some("entry_a".to_owned()),
                target_module: "core".to_owned(),
                target_symbol: Some("entry_a".to_owned()),
                line: 12,
            }],
        };

        let mod_only =
            build_project_graph(&project, ProjectDepth::ModuleOnly).expect("ModuleOnly valid");
        let pub_surface = build_project_graph(&project, ProjectDepth::PublicSurface)
            .expect("PublicSurface valid");
        let full = build_project_graph(&project, ProjectDepth::Full).expect("Full valid");

        assert!(pub_surface.declarations().len() > mod_only.declarations().len());
        assert!(full.declarations().len() > pub_surface.declarations().len());
        assert!(mod_only.groups().iter().any(|group| {
            group.id == NodeId::new("comp:Runner")
                && group.kind
                    == GroupKind::Component {
                        declared_dependencies: Some(vec!["Core".to_owned(), "Unused".to_owned()]),
                    }
        }));
        assert!(mod_only.summary_edges().iter().any(|edge| {
            edge.higher == NodeId::new("comp:Runner")
                && edge.lower == NodeId::new("comp:Unused")
                && edge.reflexion == Some(ReflexionStatus::Absent)
        }));
    }
}
